use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::config::RepoProp;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoSnapshot {
    pub branch: String,
    pub dirty: String,
    pub staged: String,
    pub untracked: String,
    pub stashed: String,
    pub situ: String,
    pub commit_msg: String,
    pub commit_time: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    head: String,
    index_mtime: u64,
    stash_mtime: u64,
    fetch_head_mtime: u64,
    snap: RepoSnapshot,
}

const LL_CACHE_VERSION: u32 = 3;

#[derive(Debug, Serialize, Deserialize)]
pub struct LlCache {
    #[serde(default)]
    version: u32,
    repos: std::collections::HashMap<String, CacheEntry>,
}

impl Default for LlCache {
    fn default() -> Self {
        Self {
            version: LL_CACHE_VERSION,
            repos: std::collections::HashMap::new(),
        }
    }
}

impl LlCache {
    pub fn load() -> Self {
        let path = cache_path();
        if !path.is_file() {
            return Self::default();
        }
        let loaded: Self = fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if loaded.version < LL_CACHE_VERSION {
            return Self::default();
        }
        loaded
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string(self) {
            let _ = fs::write(cache_path(), json);
        }
    }

    pub fn get_snap(&self, path: &str, fp: &RepoFingerprint) -> Option<RepoSnapshot> {
        let e = self.repos.get(path)?;
        if e.head == fp.head
            && e.index_mtime == fp.index_mtime
            && e.stash_mtime == fp.stash_mtime
            && e.fetch_head_mtime == fp.fetch_head_mtime
        {
            Some(e.snap.clone())
        } else {
            None
        }
    }

    pub fn insert_snap(&mut self, path: String, fp: RepoFingerprint, snap: RepoSnapshot) {
        self.repos.insert(
            path,
            CacheEntry {
                head: fp.head,
                index_mtime: fp.index_mtime,
                stash_mtime: fp.stash_mtime,
                fetch_head_mtime: fp.fetch_head_mtime,
                snap,
            },
        );
    }
}

#[derive(Debug, Clone)]
pub struct RepoFingerprint {
    pub head: String,
    pub index_mtime: u64,
    pub stash_mtime: u64,
    pub fetch_head_mtime: u64,
}

impl RepoFingerprint {
    pub fn read(prop: &RepoProp) -> Option<Self> {
        let head = read_head_oid(&prop.path, &prop.flags)?;
        let (index_path, stash_path, fetch_head_path) = git_dir_paths(&prop.path, &prop.flags);
        Some(Self {
            head,
            index_mtime: mtime_secs(&index_path),
            stash_mtime: mtime_secs(&stash_path),
            fetch_head_mtime: mtime_secs(&fetch_head_path),
        })
    }

    pub fn empty() -> Self {
        Self {
            head: String::new(),
            index_mtime: 0,
            stash_mtime: 0,
            fetch_head_mtime: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SnapshotOpts {
    pub no_untracked: bool,
}

fn cache_path() -> PathBuf {
    crate::config::config_path("ll-cache.json")
}

fn mtime_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn read_head_oid(repo_path: &str, flags: &[String]) -> Option<String> {
    if flags.is_empty() {
        return read_head_oid_at(Path::new(repo_path));
    }
    let mut cmd = Command::new("git");
    for f in flags {
        cmd.arg(f);
    }
    cmd.current_dir(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve `.git` directory for a worktree (handles linked worktrees).
pub fn resolve_git_dir(repo_path: &str, flags: &[String]) -> Option<PathBuf> {
    if let Some(gd) = flags.iter().find_map(|f| f.strip_prefix("--git-dir=")) {
        return Some(PathBuf::from(gd));
    }
    let root = Path::new(repo_path);
    let dot = root.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    if dot.is_file() {
        let text = fs::read_to_string(&dot).ok()?;
        let gitdir = text.strip_prefix("gitdir: ")?.trim();
        return Some(PathBuf::from(gitdir));
    }
    None
}

fn read_head_oid_at(root: &Path) -> Option<String> {
    let git_dir = resolve_git_dir(root.to_str()?, &[])?;
    let head_path = git_dir.join("HEAD");
    let head = fs::read_to_string(&head_path).ok()?;
    let head = head.trim();
    if let Some(rest) = head.strip_prefix("ref: ") {
        let ref_path = git_dir.join(rest);
        return fs::read_to_string(ref_path)
            .ok()
            .map(|s| s.trim().to_string());
    }
    Some(head.to_string())
}

pub fn git_dir_paths(repo_path: &str, flags: &[String]) -> (PathBuf, PathBuf, PathBuf) {
    if let Some(gd) = resolve_git_dir(repo_path, flags) {
        return (
            gd.join("index"),
            gd.join("logs").join("refs").join("stash"),
            gd.join("FETCH_HEAD"),
        );
    }
    let root = Path::new(repo_path);
    let git_dir = root.join(".git");
    (
        git_dir.join("index"),
        git_dir.join("logs").join("refs").join("stash"),
        git_dir.join("FETCH_HEAD"),
    )
}

pub fn collect_snapshot(prop: &RepoProp, opts: SnapshotOpts) -> RepoSnapshot {
    let path = &prop.path;
    if !Path::new(path).exists() {
        return error_snap(format!("path not found: {path}"));
    }
    collect_snapshot_inner(prop, opts)
}

fn collect_snapshot_inner(prop: &RepoProp, opts: SnapshotOpts) -> RepoSnapshot {
    let (_index_path, stash_path, _fetch_head_path) = git_dir_paths(&prop.path, &prop.flags);
    let stashed = if mtime_secs(&stash_path) > 0 {
        "stashed".into()
    } else {
        String::new()
    };

    let combined = match run_git_snapshot(prop, opts) {
        Ok(o) => o,
        Err(e) => return error_snap(e),
    };

    let (status_out, commit_msg, commit_time) = match parse_git_snapshot(&combined) {
        Ok(v) => v,
        Err(e) => return error_snap(e),
    };

    let (branch, dirty, staged, untracked, situ) = parse_porcelain(&status_out);

    RepoSnapshot {
        branch,
        dirty,
        staged,
        untracked,
        stashed,
        situ,
        commit_msg,
        commit_time,
        error: None,
    }
}

/// One shell invocation per repo: porcelain status + show-branch subject + relative commit time.
fn run_git_snapshot(prop: &RepoProp, opts: SnapshotOpts) -> Result<String, String> {
    let flags = git_flags_shell(&prop.flags);
    let status_cmd = if opts.no_untracked {
        "status --porcelain=v1 -b -uno --ignore-submodules=all"
    } else {
        "status --porcelain=v1 -b --ignore-submodules=all"
    };
    let script = format!(
        r#"git {flags}-c status.aheadBehind=false {status_cmd}
printf '\n---GITA---\n'
git {flags}show-branch --no-name HEAD 2>/dev/null || git {flags}log -1 --format=%s
printf '\n---GITA---\n'
git {flags}log -1 --format=%cd --date=relative 2>/dev/null || true
"#,
        flags = flags,
        status_cmd = status_cmd,
    );
    run_shell(prop, &script)
}

fn git_flags_shell(flags: &[String]) -> String {
    if flags.is_empty() {
        String::new()
    } else {
        format!("{} ", flags.join(" "))
    }
}

fn run_shell(prop: &RepoProp, script: &str) -> Result<String, String> {
    let out = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(&prop.path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() && out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "git command failed".into()
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn parse_git_snapshot(out: &str) -> Result<(String, String, String), String> {
    let mut parts = out.splitn(3, "---GITA---");
    let status = parts.next().unwrap_or("").trim().to_string();
    if status.is_empty() {
        return Err("empty status".into());
    }
    let commit_msg = parts.next().unwrap_or("").trim().to_string();
    if commit_msg.is_empty() {
        return Err("no commit".into());
    }
    let rel = parts.next().unwrap_or("").trim();
    let commit_time = if rel.is_empty() {
        String::new()
    } else {
        format!("({rel})")
    };
    Ok((status, commit_msg, commit_time))
}

fn parse_porcelain(out: &str) -> (String, String, String, String, String) {
    let mut branch = "HEAD".to_string();
    let mut dirty = String::new();
    let mut staged = String::new();
    let mut untracked = String::new();
    let mut situ = "no_remote".to_string();
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut has_upstream = false;

    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            parse_branch_header(
                rest,
                &mut branch,
                &mut situ,
                &mut has_upstream,
                &mut ahead,
                &mut behind,
            );
            continue;
        }
        if line.starts_with("??") {
            untracked = "untracked".into();
            continue;
        }
        if line.len() < 2 {
            continue;
        }
        let x = line.as_bytes()[0] as char;
        let y = line.as_bytes()[1] as char;
        if x != ' ' && x != '?' {
            staged = "staged".into();
        }
        if y != ' ' {
            dirty = "dirty".into();
        }
    }

    if has_upstream {
        situ = situ_from_ahead_behind(ahead, behind);
    }

    (branch, dirty, staged, untracked, situ)
}

fn parse_branch_header(
    rest: &str,
    branch: &mut String,
    situ: &mut String,
    has_upstream: &mut bool,
    ahead: &mut u32,
    behind: &mut u32,
) {
    let (head_part, bracket) = match rest.split_once(" [") {
        Some((h, b)) => (h, Some(b.trim_end_matches(']'))),
        None => (rest, None),
    };

    if let Some((local, _upstream)) = head_part.split_once("...") {
        *branch = local.to_string();
        *has_upstream = true;
        *situ = "in_sync".into();
    } else if head_part.contains("(no branch)") {
        *branch = "HEAD".into();
        *has_upstream = false;
        *situ = "no_remote".into();
    } else {
        *branch = head_part.to_string();
        *has_upstream = false;
        *situ = "no_remote".into();
    }

    if let Some(b) = bracket {
        for part in b.split(',') {
            let part = part.trim();
            if let Some(n) = part.strip_prefix("ahead ") {
                *ahead = n.trim().parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                *behind = n.trim().parse().unwrap_or(0);
            }
        }
    }
}

fn situ_from_ahead_behind(ahead: u32, behind: u32) -> String {
    match (ahead, behind) {
        (0, 0) => "in_sync".into(),
        (_, 0) => "local_ahead".into(),
        (0, _) => "remote_ahead".into(),
        _ => "diverged".into(),
    }
}

fn error_snap(msg: String) -> RepoSnapshot {
    RepoSnapshot {
        branch: String::new(),
        dirty: String::new(),
        staged: String::new(),
        untracked: String::new(),
        stashed: String::new(),
        situ: String::new(),
        commit_msg: String::new(),
        commit_time: String::new(),
        error: Some(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_branch_upstream_sync() {
        let (b, d, s, u, situ) = parse_porcelain("## main...origin/main\n");
        assert_eq!(b, "main");
        assert_eq!(d, "");
        assert_eq!(s, "");
        assert_eq!(u, "");
        assert_eq!(situ, "in_sync");
    }

    #[test]
    fn parse_dirty_staged_untracked() {
        let out = "## dev...origin/dev [ahead 1, behind 2]\nMM file\n?? new\n";
        let (b, d, s, u, situ) = parse_porcelain(out);
        assert_eq!(b, "dev");
        assert_eq!(d, "dirty");
        assert_eq!(s, "staged");
        assert_eq!(u, "untracked");
        assert_eq!(situ, "diverged");
    }

    #[test]
    fn parse_git_snapshot_output() {
        let out = "## main...origin/main\n\n---GITA---\nfeat: thing\n---GITA---\n2 days ago\n";
        let (status, msg, time) = parse_git_snapshot(out).unwrap();
        assert!(status.contains("main"));
        assert_eq!(msg, "feat: thing");
        assert_eq!(time, "(2 days ago)");
    }
}
