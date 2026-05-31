use std::path::{Path, PathBuf};
use std::process::Command;

pub fn is_git(path: &str, include_bare: bool, exclude_submodule: bool) -> bool {
    let p = Path::new(path);
    if !p.exists() {
        return false;
    }
    let dot_git = p.join(".git");
    if dot_git.exists() {
        if exclude_submodule && is_submodule_gitfile(&dot_git) {
            return false;
        }
        return true;
    }
    if !include_bare {
        return false;
    }
    Command::new("git")
        .args(["rev-parse", "--is-bare-repository"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success() && o.stdout == b"true\n")
        .unwrap_or(false)
}

fn is_submodule_gitfile(dot_git: &Path) -> bool {
    if dot_git.is_file() {
        if let Ok(text) = std::fs::read_to_string(dot_git) {
            return text.contains(".git/modules");
        }
    }
    false
}

pub fn open_repository(path: &str, flags: &[String]) -> Result<git2::Repository, String> {
    if flags.is_empty() {
        let p = Path::new(path);
        if p.join(".git").exists() {
            return git2::Repository::open(p).map_err(|e| e.message().to_string());
        }
        return git2::Repository::discover(p).map_err(|e| e.message().to_string());
    }
    let mut git_dir: Option<PathBuf> = None;
    let mut work_tree: Option<PathBuf> = None;
    for flag in flags {
        if let Some(v) = flag.strip_prefix("--git-dir=") {
            git_dir = Some(PathBuf::from(v));
        } else if let Some(v) = flag.strip_prefix("--work-tree=") {
            work_tree = Some(PathBuf::from(v));
        }
    }
    if let Some(gd) = git_dir {
        if let Some(wt) = work_tree {
            if let Ok(repo) = git2::Repository::open(&wt) {
                return Ok(repo);
            }
            let _ = wt;
        }
        return git2::Repository::open(&gd).map_err(|e| e.message().to_string());
    }
    let p = Path::new(path);
    if p.join(".git").exists() {
        return git2::Repository::open(p).map_err(|e| e.message().to_string());
    }
    git2::Repository::discover(path).map_err(|e| e.message().to_string())
}

pub fn relative_path_depth(kid: &Path, parent: &str) -> Option<Vec<String>> {
    if parent.is_empty() {
        return None;
    }
    let parent = Path::new(parent);
    kid.strip_prefix(parent).ok().map(|rel| {
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .filter(|s| s != ".")
            .collect()
    })
}
