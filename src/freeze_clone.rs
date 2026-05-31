use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use csv::ReaderBuilder;

use crate::config::{self, GroupProp, RepoProp};
use crate::git_util;
use crate::ll::repo_status::read_head_oid;

pub fn cmd_freeze(group: Option<String>) -> Result<()> {
    let repos = config::load_repos(true)?;
    let groups = config::load_groups(&repos)?;
    let mut group = group;
    if group.is_none() {
        group = config::get_context(&groups)?;
    }

    let mut filtered: HashMap<String, RepoProp> = repos;
    let group_repos = if let Some(ref g) = group {
        let Some(prop) = groups.get(g) else {
            return Ok(());
        };
        filtered.retain(|k, _| prop.repos.contains(k));
        Some(prop.repos.clone())
    } else {
        None
    };

    let mut seen_urls = std::collections::HashSet::new();
    let mut names: Vec<_> = filtered.keys().cloned().collect();
    names.sort();
    for name in names {
        let prop = &filtered[&name];
        let url = git_remote_url(&prop.path, &prop.flags)?;
        if url.is_empty() || !seen_urls.insert(url.clone()) {
            continue;
        }
        let branch = read_head_oid(&prop.path, &prop.flags).unwrap_or_else(|| "HEAD".into());
        let flags = prop.flags.join(" ");
        println!(
            "{},{},{},{},{},{}",
            url, name, prop.path, prop.repo_type, flags, branch
        );
    }

    if let Some(g) = group {
        if let Some(prop) = groups.get(&g) {
            let repos_join = group_repos.unwrap_or_default().join("|");
            println!(",{},{},{}", g, prop.path, repos_join);
        }
    } else {
        let mut gnames: Vec<_> = groups.keys().cloned().collect();
        gnames.sort();
        for g in gnames {
            let prop = &groups[&g];
            let repos_join = prop.repos.join("|");
            println!(",{},{},{}", g, prop.path, repos_join);
        }
    }
    Ok(())
}

fn git_remote_url(path: &str, flags: &[String]) -> Result<String> {
    let mut cmd = Command::new("git");
    for f in flags {
        cmd.arg(f);
    }
    let out = cmd
        .current_dir(path)
        .args(["remote", "-v"])
        .output()
        .with_context(|| format!("git remote in {path}"))?;
    if !out.status.success() {
        return Ok(String::new());
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().next().unwrap_or("");
    let parts: Vec<_> = line.split_whitespace().collect();
    if parts.len() > 1 {
        Ok(parts[1].to_string())
    } else {
        Ok(String::new())
    }
}

#[derive(Debug, Clone)]
struct CloneRepo {
    url: String,
    path: String,
    repo_type: String,
    flags: Vec<String>,
    branch: String,
}

#[derive(Debug, Clone)]
struct CloneGroup {
    path: String,
    repos: Vec<String>,
}

fn parse_clone_config(
    fname: &Path,
) -> Result<(HashMap<String, CloneRepo>, HashMap<String, CloneGroup>)> {
    let mut repos = HashMap::new();
    let mut groups = HashMap::new();
    if !fname.is_file() {
        bail!("file not found: {}", fname.display());
    }
    let f = std::fs::File::open(fname)?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(f);
    for row in rdr.records() {
        let record = row?;
        let url = record.get(0).unwrap_or("").to_string();
        let name = record.get(1).unwrap_or("").to_string();
        let path = record.get(2).unwrap_or("").to_string();
        let repo_type = record.get(3).unwrap_or("").to_string();
        let flags_str = record.get(4).unwrap_or("");
        let branch = record.get(5).unwrap_or("").to_string();
        if url.is_empty() {
            if !name.is_empty() {
                let members: Vec<String> = repo_type
                    .split('|')
                    .filter(|r| repos.contains_key(*r))
                    .map(|s| s.to_string())
                    .collect();
                groups.insert(
                    name,
                    CloneGroup {
                        path,
                        repos: members,
                    },
                );
            }
        } else if !name.is_empty() {
            repos.insert(
                name,
                CloneRepo {
                    url,
                    path,
                    repo_type,
                    flags: flags_str
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect(),
                    branch,
                },
            );
        }
    }
    Ok((repos, groups))
}

pub fn cmd_clone(
    clonee: String,
    directory: Option<String>,
    preserve_path: bool,
    dry_run: bool,
    group: Option<String>,
    from_file: bool,
) -> Result<()> {
    let cwd = directory
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    if !from_file {
        if dry_run {
            println!("git clone {clonee}");
            return Ok(());
        }
        let status = Command::new("git")
            .args(["clone", &clonee])
            .current_dir(&cwd)
            .status()
            .context("git clone")?;
        if !status.success() {
            bail!("git clone failed");
        }
        let base = clonee
            .split('/')
            .next_back()
            .unwrap_or(&clonee)
            .trim_end_matches(".git");
        let cloned = cwd.join(base);
        let mut repos = config::load_repos(true)?;
        if repos.values().any(|r| r.path == cloned.to_string_lossy()) {
            println!("{clonee} already in gita.");
            return Ok(());
        }
        if git_util::is_git(cloned.to_string_lossy().as_ref(), false, false) {
            let path_s = cloned.to_string_lossy().into_owned();
            let base = cloned
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("repo")
                .to_string();
            let counts = HashMap::from([(base.clone(), 1usize)]);
            let name = config::make_repo_name(&path_s, &repos, &counts);
            repos.insert(
                name,
                RepoProp {
                    path: cloned.to_string_lossy().into_owned(),
                    repo_type: String::new(),
                    flags: vec![],
                },
            );
            config::write_repos(&repos)?;
            if let Some(g) = group {
                add_repos_to_group(&g, repos.keys().last().cloned().into_iter().collect())?;
            }
        }
        return Ok(());
    }

    let (to_clone, groups) = parse_clone_config(Path::new(&clonee))?;
    let existing: std::collections::HashSet<_> = config::load_repos(true)?
        .into_values()
        .map(|r| r.path)
        .collect();

    for (name, prop) in &to_clone {
        let mut git_cmd = vec!["git".to_string(), "clone".to_string(), prop.url.clone()];
        let target = if preserve_path {
            prop.path.clone()
        } else {
            name.clone()
        };
        git_cmd.push(target.clone());
        if dry_run {
            println!("{}", git_cmd.join(" "));
            continue;
        }
        if preserve_path {
            if let Some(parent) = Path::new(&prop.path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let status = Command::new("git")
            .args(["clone", &prop.url, &target])
            .current_dir(&cwd)
            .status()
            .with_context(|| format!("clone {name}"))?;
        if !status.success() {
            eprintln!("clone failed for {name}");
            continue;
        }
        if !prop.branch.is_empty() && prop.branch != "HEAD" {
            let repo_path = if preserve_path {
                prop.path.clone()
            } else {
                cwd.join(&target).to_string_lossy().into_owned()
            };
            let _ = Command::new("git")
                .current_dir(&repo_path)
                .args(["checkout", &prop.branch])
                .status();
        }
    }

    if dry_run {
        return Ok(());
    }

    let mut new_repos = HashMap::new();
    for (name, prop) in to_clone {
        if existing.contains(&prop.path) {
            continue;
        }
        new_repos.insert(
            name,
            RepoProp {
                path: prop.path,
                repo_type: prop.repo_type,
                flags: prop.flags,
            },
        );
    }
    if !new_repos.is_empty() {
        let mut repos = config::load_repos(true)?;
        for (k, v) in new_repos {
            repos.insert(k, v);
        }
        config::write_repos(&repos)?;
    }

    if !groups.is_empty() {
        let repos_now = config::load_repos(true)?;
        let mut groups_map = config::load_groups(&repos_now)?;
        for (gname, gprop) in groups {
            let entry = groups_map.entry(gname).or_insert_with(|| GroupProp {
                repos: vec![],
                path: gprop.path,
            });
            for r in gprop.repos {
                if !entry.repos.contains(&r) {
                    entry.repos.push(r);
                }
            }
            entry.repos.sort();
        }
        config::write_groups(&groups_map)?;
    }

    if let Some(g) = group {
        let names: Vec<String> = config::load_repos(true)?.into_keys().collect();
        add_repos_to_group(&g, names)?;
    }

    Ok(())
}

fn add_repos_to_group(gname: &str, repo_names: Vec<String>) -> Result<()> {
    let repos = config::load_repos(true)?;
    let mut groups = config::load_groups(&repos)?;
    let entry = groups
        .entry(gname.to_string())
        .or_insert_with(|| GroupProp {
            repos: vec![],
            path: String::new(),
        });
    for r in repo_names {
        if !entry.repos.contains(&r) {
            entry.repos.push(r);
        }
    }
    entry.repos.sort();
    config::write_groups(&groups)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_freeze_file_shape() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("frozen.csv");
        std::fs::write(
            &f,
            "https://example.com/a.git,aname,/path/a,,,\n,grp1,/gpath,aname\n",
        )
        .unwrap();
        let (repos, groups) = parse_clone_config(&f).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos["aname"].url, "https://example.com/a.git");
        assert_eq!(groups["grp1"].repos, vec!["aname".to_string()]);
    }
}
