use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use csv::ReaderBuilder;
use tempfile::NamedTempFile;

use crate::git_util::is_git;

#[derive(Debug, Clone)]
pub struct RepoProp {
    pub path: String,
    pub repo_type: String,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GroupProp {
    pub repos: Vec<String>,
    pub path: String,
}

pub fn workspace_root() -> PathBuf {
    if let Some(home) = std::env::var_os("GITA_PROJECT_HOME") {
        return PathBuf::from(home).join("gita");
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("gita");
    }
    dirs::home_dir()
        .map(|h| h.join(".config").join("gita"))
        .unwrap_or_else(|| PathBuf::from(".config/gita"))
}

pub fn config_dir() -> PathBuf {
    let root = workspace_root();
    if let Some(name) = current_workspace_name(&root) {
        let ws = root.join("workspaces").join(&name);
        if ws.is_dir() {
            return ws;
        }
    }
    root
}

pub fn config_path(name: &str) -> PathBuf {
    config_dir().join(name)
}

fn current_workspace_name(root: &Path) -> Option<String> {
    let p = root.join("workspace");
    if !p.is_file() {
        return None;
    }
    fs::read_to_string(&p).ok().and_then(|s| {
        let name = s.trim();
        if name.is_empty() || name == "default" {
            return None;
        }
        let ws = root.join("workspaces").join(name);
        if ws.is_dir() {
            Some(name.to_string())
        } else {
            // stale pointer; clear it so we don't fall back while still reporting it
            let _ = fs::remove_file(&p);
            None
        }
    })
}

pub fn current_workspace() -> Option<String> {
    current_workspace_name(&workspace_root())
}

pub fn list_workspaces() -> Result<Vec<String>> {
    let root = workspace_root();
    let ws_root = root.join("workspaces");
    if !ws_root.is_dir() {
        return Ok(vec![]);
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&ws_root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}

pub fn workspace_dir(name: &str) -> PathBuf {
    workspace_root().join("workspaces").join(name)
}

pub fn workspace_active_file() -> PathBuf {
    workspace_root().join("workspace")
}

pub fn validate_workspace_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("workspace name cannot be empty");
    }
    if name == "default" {
        anyhow::bail!("'default' is reserved for the root workspace");
    }
    if name == "." || name == ".." {
        anyhow::bail!("workspace name cannot be '.' or '..'");
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        anyhow::bail!("workspace name cannot contain path separators");
    }
    let root = workspace_root();
    if root.join(name).exists() && !root.join("workspaces").join(name).is_dir() {
        anyhow::bail!("workspace name conflicts with an existing file in the config directory");
    }
    Ok(())
}

pub fn create_workspace(name: &str, from_current: bool) -> Result<()> {
    validate_workspace_name(name)?;
    let target = workspace_dir(name);
    if target.is_dir() {
        anyhow::bail!("workspace already exists: {name}");
    }
    fs::create_dir_all(&target)?;
    if from_current {
        copy_config_files(&config_dir(), &target)?;
    }
    Ok(())
}

fn copy_config_files(src: &Path, dst: &Path) -> Result<()> {
    if !src.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let fname = entry.file_name();
        if fname == "workspace" {
            // active-workspace pointer belongs to the root only
            continue;
        }
        fs::copy(entry.path(), dst.join(&fname))?;
    }
    Ok(())
}

pub fn remove_workspace(name: &str) -> Result<()> {
    if current_workspace().as_deref() == Some(name) {
        anyhow::bail!("cannot remove the active workspace; switch to another workspace first");
    }
    let target = workspace_dir(name);
    if !target.is_dir() {
        anyhow::bail!("workspace not found: {name}");
    }
    fs::remove_dir_all(&target)?;
    Ok(())
}

pub fn rename_workspace(old: &str, new: &str) -> Result<()> {
    if old == "default" {
        anyhow::bail!("cannot rename the default workspace");
    }
    validate_workspace_name(new)?;
    let src = workspace_dir(old);
    if !src.is_dir() {
        anyhow::bail!("workspace not found: {old}");
    }
    let dst = workspace_dir(new);
    if dst.exists() {
        anyhow::bail!("workspace already exists: {new}");
    }
    let is_active = current_workspace().as_deref() == Some(old);
    fs::rename(&src, &dst)?;
    if is_active {
        fs::write(workspace_active_file(), new)?;
    }
    Ok(())
}

pub fn set_workspace(name: &str) -> Result<()> {
    if name == "default" {
        clear_workspace()?;
        return Ok(());
    }
    let target = workspace_dir(name);
    if !target.is_dir() {
        anyhow::bail!("workspace not found: {name}");
    }
    fs::write(workspace_active_file(), name)?;
    Ok(())
}

pub fn clear_workspace() -> Result<()> {
    let p = workspace_active_file();
    if p.is_file() {
        fs::remove_file(&p)?;
    }
    Ok(())
}

pub fn load_repos(skip_validation: bool) -> Result<HashMap<String, RepoProp>> {
    let path = config_path("repos.csv");
    let mut repos = HashMap::new();
    if !path.is_file() {
        return Ok(repos);
    }
    let f = fs::File::open(&path).with_context(|| format!("open {}", path.display()))?;
    if f.metadata()?.len() == 0 {
        return Ok(repos);
    }
    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(f);
    for result in rdr.records() {
        let record = result?;
        let path = record.get(0).unwrap_or("").to_string();
        let name = record.get(1).unwrap_or("").to_string();
        let repo_type = record.get(2).unwrap_or("").to_string();
        let flags_str = record.get(3).unwrap_or("");
        if name.is_empty() || path.is_empty() {
            continue;
        }
        if skip_validation || is_git(&path, true, false) {
            let flags: Vec<String> = flags_str
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            repos.insert(
                name,
                RepoProp {
                    path,
                    repo_type,
                    flags,
                },
            );
        }
    }
    Ok(repos)
}

pub fn write_repos(repos: &HashMap<String, RepoProp>) -> Result<()> {
    let path = config_path("repos.csv");
    fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    let mut rows: Vec<(String, String, String, String)> = repos
        .iter()
        .map(|(name, prop)| {
            (
                prop.path.clone(),
                name.clone(),
                prop.repo_type.clone(),
                prop.flags.join(" "),
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    atomic_write_csv(&path, |w| {
        let mut writer = csv::Writer::from_writer(w);
        for (p, name, ty, flags) in &rows {
            writer.write_record([p, name, ty, flags])?;
        }
        writer.flush()?;
        Ok(())
    })
}

pub fn load_groups(repos: &HashMap<String, RepoProp>) -> Result<HashMap<String, GroupProp>> {
    let path = config_path("groups.csv");
    let mut groups = HashMap::new();
    if !path.is_file() {
        return Ok(groups);
    }
    if fs::metadata(&path)?.len() == 0 {
        return Ok(groups);
    }
    let content = fs::read_to_string(&path)?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        if parts.len() < 2 {
            continue;
        }
        let name = parts[0].to_string();
        let repo_names: Vec<String> = parts[1]
            .split_whitespace()
            .filter(|r| repos.contains_key(*r))
            .map(|s| s.to_string())
            .collect();
        let gpath = parts.get(2).unwrap_or(&"").to_string();
        groups.insert(
            name,
            GroupProp {
                repos: repo_names,
                path: gpath,
            },
        );
    }
    Ok(groups)
}

pub fn write_groups(groups: &HashMap<String, GroupProp>) -> Result<()> {
    let path = config_path("groups.csv");
    fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    let mut groups = groups.clone();
    groups.retain(|_, g| !g.repos.is_empty());
    if groups.is_empty() {
        fs::write(&path, "")?;
        return Ok(());
    }
    atomic_write_csv(&path, |w| {
        let mut names: Vec<_> = groups.keys().cloned().collect();
        names.sort();
        for name in names {
            let prop = &groups[&name];
            let repos = prop.repos.join(" ");
            writeln!(w, "{name}:{repos}:{}", prop.path)?;
        }
        Ok(())
    })
}

pub fn get_context(groups: &HashMap<String, GroupProp>) -> Result<Option<String>> {
    let dir = config_dir();
    if !dir.is_dir() {
        return Ok(None);
    }
    let mut matches = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".context") {
            matches.push(entry.path());
        }
    }
    if matches.len() > 1 {
        anyhow::bail!("Cannot have multiple .context file");
    }
    if matches.is_empty() {
        return Ok(None);
    }
    let ctx = &matches[0];
    let stem = ctx
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    if stem == "auto" {
        let cwd = std::env::current_dir()?;
        let mut candidate: Option<String> = None;
        let mut min_dist = usize::MAX;
        for (gname, prop) in groups {
            if let Some(rel) = relative_path(&cwd, &prop.path) {
                let d = rel.len();
                if d < min_dist {
                    min_dist = d;
                    candidate = Some(gname.clone());
                }
            }
        }
        return Ok(candidate);
    }
    Ok(Some(stem))
}

pub fn replace_context(old: Option<&Path>, new: &str) -> Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    let auto = dir.join("auto.context");
    let mut old_path = old.map(|p| p.to_path_buf());
    if auto.exists() {
        old_path = Some(auto);
    }
    if new == "none" {
        if let Some(p) = old_path {
            let _ = fs::remove_file(p);
        }
        return Ok(());
    }
    if let Some(p) = old_path {
        let dest = dir.join(format!("{new}.context"));
        fs::rename(p, dest)?;
    } else {
        fs::write(dir.join(format!("{new}.context")), "")?;
    }
    Ok(())
}

pub fn context_file_path() -> Option<PathBuf> {
    let dir = config_dir();
    let mut matches = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".context") {
                matches.push(entry.path());
            }
        }
    }
    if matches.len() == 1 {
        Some(matches.remove(0))
    } else {
        None
    }
}

fn relative_path(kid: &Path, parent: &str) -> Option<Vec<String>> {
    if parent.is_empty() {
        return None;
    }
    let parent = Path::new(parent);
    let rel = kid.strip_prefix(parent).ok()?;
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts == ["."] {
        parts.clear();
    }
    Some(parts)
}

fn atomic_write_csv(path: &Path, write: impl FnOnce(&mut dyn Write) -> Result<()>) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    write(tmp.as_file_mut())?;
    tmp.persist(path)?;
    Ok(())
}

pub fn delete_repo_from_groups(repo: &str, groups: &mut HashMap<String, GroupProp>) -> bool {
    let mut deleted = false;
    for prop in groups.values_mut() {
        if let Some(pos) = prop.repos.iter().position(|r| r == repo) {
            prop.repos.remove(pos);
            deleted = true;
        }
    }
    deleted
}

pub fn make_repo_name(
    path: &str,
    repos: &HashMap<String, RepoProp>,
    counts: &HashMap<String, usize>,
) -> String {
    let path = Path::new(path);
    let base = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo")
        .to_string();
    if repos.contains_key(&base) || counts.get(&base).copied().unwrap_or(0) > 1 {
        let parent = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("");
        format!("{parent}/{base}")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    #[serial]
    fn parse_repos_csv_no_header() {
        let tmp = TempDir::new().unwrap();
        let gita_dir = tmp.path().join("gita");
        fs::create_dir_all(&gita_dir).unwrap();
        let csv = gita_dir.join("repos.csv");
        let mut f = fs::File::create(&csv).unwrap();
        writeln!(f, "/tmp/a,repo-a,,").unwrap();
        writeln!(f, "/tmp/b,repo-b,work,").unwrap();
        std::env::set_var("GITA_PROJECT_HOME", tmp.path());
        let repos = load_repos(true).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos["repo-a"].path, "/tmp/a");
        assert!(repos["repo-b"].flags.is_empty());
        std::env::remove_var("GITA_PROJECT_HOME");
    }

    #[test]
    #[serial]
    fn parse_groups_colon_delim() {
        let tmp = TempDir::new().unwrap();
        let gita_dir = tmp.path().join("gita");
        fs::create_dir_all(&gita_dir).unwrap();
        std::env::set_var("GITA_PROJECT_HOME", tmp.path());
        let mut repos = HashMap::new();
        repos.insert(
            "a".into(),
            RepoProp {
                path: "/x".into(),
                repo_type: String::new(),
                flags: vec![],
            },
        );
        repos.insert(
            "b".into(),
            RepoProp {
                path: "/y".into(),
                repo_type: String::new(),
                flags: vec![],
            },
        );
        fs::write(gita_dir.join("groups.csv"), "g1:a b:/parent/path\n").unwrap();
        let groups = load_groups(&repos).unwrap();
        assert_eq!(groups["g1"].repos, vec!["a", "b"]);
        assert_eq!(groups["g1"].path, "/parent/path");
        std::env::remove_var("GITA_PROJECT_HOME");
    }
}
