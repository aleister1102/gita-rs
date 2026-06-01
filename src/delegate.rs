use std::collections::HashMap;
use std::process::Stdio;

use anyhow::Result;
use serde::Deserialize;
use tokio::process::Command;

use crate::config::{GroupProp, RepoProp};

#[derive(Debug, Deserialize)]
pub struct CmdDef {
    pub cmd: String,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub disable_async: bool,
    #[serde(default)]
    pub allow_all: bool,
    #[serde(default)]
    pub shell: bool,
}

pub fn load_cmds() -> Result<HashMap<String, CmdDef>> {
    let mut cmds: HashMap<String, CmdDef> = serde_json::from_str(include_str!("../cmds.json"))?;
    let custom = crate::config::config_path("cmds.json");
    if custom.is_file() {
        let content = std::fs::read_to_string(&custom)?;
        if !content.trim().is_empty() {
            let custom_cmds: HashMap<String, CmdDef> = serde_json::from_str(&content)?;
            cmds.extend(custom_cmds);
        }
    }
    Ok(cmds)
}

pub fn async_blacklist(cmds: &HashMap<String, CmdDef>) -> Vec<String> {
    cmds.iter()
        .filter(|(_, d)| d.disable_async)
        .map(|(k, _)| k.clone())
        .collect()
}

pub fn parse_repos_and_rest(
    input: &[String],
    repos: &HashMap<String, RepoProp>,
    groups: &HashMap<String, GroupProp>,
    context: Option<&str>,
) -> (HashMap<String, RepoProp>, Vec<String>) {
    let mut names = Vec::new();
    let mut i = 0;
    while i < input.len() {
        let word = &input[i];
        if repos.contains_key(word) || groups.contains_key(word) {
            names.push(word.clone());
            i += 1;
        } else {
            break;
        }
    }
    if i == input.len() && !input.is_empty() {
        i += 1;
    }
    if names.is_empty() {
        if let Some(ctx) = context {
            names.push(ctx.to_string());
        }
    }
    let mut chosen = HashMap::new();
    if !names.is_empty() {
        for k in names {
            if let Some(prop) = repos.get(&k) {
                chosen.insert(k.clone(), prop.clone());
            }
            if let Some(g) = groups.get(&k) {
                for r in &g.repos {
                    if let Some(prop) = repos.get(r) {
                        chosen.insert(r.clone(), prop.clone());
                    }
                }
            }
        }
    } else {
        chosen = repos.clone();
    }
    let rest = input[i..].to_vec();
    (chosen, rest)
}

pub fn format_output(s: &str, prefix: &str) -> String {
    s.lines()
        .map(|line| format!("{prefix}: {line}"))
        .collect::<Vec<_>>()
        .join("\n")
        + if s.ends_with('\n') { "\n" } else { "" }
}

pub async fn run_async(repo_name: &str, path: &str, cmds: &[String]) -> Option<String> {
    let mut cmd = Command::new(&cmds[0]);
    for arg in &cmds[1..] {
        cmd.arg(arg);
    }
    cmd.current_dir(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn().ok()?;
    let output = child.wait_with_output().await.ok()?;
    if let Ok(stdout) = String::from_utf8(output.stdout) {
        if !stdout.is_empty() {
            print!("{}", format_output(&stdout, repo_name));
        }
    }
    if let Ok(stderr) = String::from_utf8(output.stderr) {
        if !stderr.is_empty() {
            print!("{}", format_output(&stderr, repo_name));
        }
    }
    if !output.status.success() {
        return Some(path.to_string());
    }
    None
}

pub fn run_sync(path: &str, cmds: &[String], shell: bool) -> Result<()> {
    println!("{path}");
    if shell {
        let cmdline = cmds.join(" ");
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmdline)
            .current_dir(path)
            .status()?;
        if !status.success() {
            anyhow::bail!("command failed");
        }
    } else {
        let status = std::process::Command::new(&cmds[0])
            .args(&cmds[1..])
            .current_dir(path)
            .status()?;
        if !status.success() {
            anyhow::bail!("command failed");
        }
    }
    Ok(())
}

pub async fn exec_git_cmd(
    repos: &HashMap<String, RepoProp>,
    base_cmd: &[String],
    disable_async: bool,
    shell: bool,
) -> Result<()> {
    let mut per_repo_cmds: Vec<(String, String, Vec<String>)> = Vec::new();
    for (name, prop) in repos {
        let mut cmds = base_cmd.to_vec();
        if !shell && cmds.first().map(|s| s.as_str()) == Some("git") && !prop.flags.is_empty() {
            let mut with_flags = vec![cmds[0].clone()];
            with_flags.extend(prop.flags.clone());
            with_flags.extend(cmds[1..].iter().cloned());
            cmds = with_flags;
        }
        per_repo_cmds.push((name.clone(), prop.path.clone(), cmds));
    }

    if repos.len() == 1 || disable_async {
        for (_, path, cmds) in &per_repo_cmds {
            run_sync(path, cmds, shell)?;
        }
        return Ok(());
    }

    let mut handles = Vec::new();
    for (name, path, cmds) in per_repo_cmds {
        handles.push(tokio::task::spawn(
            async move { run_async(&name, &path, &cmds).await },
        ));
    }
    let mut errors = Vec::new();
    for h in handles {
        if let Ok(Some(path)) = h.await {
            errors.push(path);
        }
    }
    for path in errors {
        println!("{path}");
        let _ = run_sync(&path, base_cmd, shell);
    }
    Ok(())
}
