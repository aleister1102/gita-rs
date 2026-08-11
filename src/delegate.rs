use std::collections::HashMap;
use std::io::Write;
use std::process::Stdio;

use anyhow::Result;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncReadExt};
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

fn write_records<W: Write>(
    writer: &mut W,
    repo_name: &str,
    pending: &mut Vec<u8>,
    trailing_cr: &mut bool,
    chunk: &[u8],
    eof: bool,
) -> std::io::Result<()> {
    if eof {
        if !pending.is_empty() {
            writer.write_all(repo_name.as_bytes())?;
            writer.write_all(b": ")?;
            writer.write_all(pending)?;
            pending.clear();
        }
        return Ok(());
    }
    let mut start = 0;
    if *trailing_cr {
        if chunk.first() == Some(&b'\n') {
            writer.write_all(b"\n")?;
            start = 1;
        }
        *trailing_cr = false;
    }
    let mut i = start;
    while i < chunk.len() {
        if chunk[i] == b'\n' {
            writer.write_all(repo_name.as_bytes())?;
            writer.write_all(b": ")?;
            writer.write_all(pending)?;
            writer.write_all(&chunk[start..=i])?;
            pending.clear();
            start = i + 1;
        } else if chunk[i] == b'\r' {
            let end = if i + 1 < chunk.len() && chunk[i + 1] == b'\n' {
                i + 1
            } else {
                i
            };
            writer.write_all(repo_name.as_bytes())?;
            writer.write_all(b": ")?;
            writer.write_all(pending)?;
            writer.write_all(&chunk[start..=end])?;
            pending.clear();
            start = end + 1;
            i = end;
        }
        i += 1;
    }
    if start < chunk.len() {
        pending.extend_from_slice(&chunk[start..]);
    }
    if chunk.last() == Some(&b'\r') {
        *trailing_cr = true;
    }
    Ok(())
}

async fn stream_output<R: AsyncRead + Unpin>(reader: R, repo_name: &str) {
    let mut reader = reader;
    let mut buf = [0u8; 8192];
    let mut pending: Vec<u8> = Vec::new();
    let mut trailing_cr = false;
    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return,
        };
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        if write_records(
            &mut lock,
            repo_name,
            &mut pending,
            &mut trailing_cr,
            &buf[..n],
            false,
        )
        .is_err()
        {
            return;
        }
        if lock.flush().is_err() {
            return;
        }
        drop(lock);
    }
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    if write_records(
        &mut lock,
        repo_name,
        &mut pending,
        &mut trailing_cr,
        &[],
        true,
    )
    .is_err()
    {
        return;
    }
    let _ = lock.flush();
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
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take()?;
    let (_, _, status) = tokio::join!(
        stream_output(stdout, repo_name),
        stream_output(stderr, repo_name),
        child.wait(),
    );
    if let Ok(status) = status {
        if !status.success() {
            return Some(path.to_string());
        }
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
        handles.push(tokio::task::spawn(async move {
            run_async(&name, &path, &cmds).await
        }));
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

#[cfg(test)]
mod tests {
    use super::write_records;

    #[test]
    fn write_records_handles_cr_lf_chunk_boundaries_and_eof() {
        let mut out = Vec::new();
        let mut pending = Vec::new();
        let mut trailing_cr = false;

        write_records(
            &mut out,
            "alpha",
            &mut pending,
            &mut trailing_cr,
            b"line\n",
            false,
        )
        .unwrap();
        write_records(
            &mut out,
            "alpha",
            &mut pending,
            &mut trailing_cr,
            b"a\rb\n",
            false,
        )
        .unwrap();
        write_records(
            &mut out,
            "alpha",
            &mut pending,
            &mut trailing_cr,
            b"c\r\n",
            false,
        )
        .unwrap();
        write_records(
            &mut out,
            "alpha",
            &mut pending,
            &mut trailing_cr,
            b"d\r",
            false,
        )
        .unwrap();
        write_records(
            &mut out,
            "alpha",
            &mut pending,
            &mut trailing_cr,
            b"\ne\n",
            false,
        )
        .unwrap();

        let mut big = vec![b'x'; 8191];
        big.push(b'\n');
        write_records(
            &mut out,
            "alpha",
            &mut pending,
            &mut trailing_cr,
            &big,
            false,
        )
        .unwrap();

        write_records(
            &mut out,
            "alpha",
            &mut pending,
            &mut trailing_cr,
            b"tail",
            false,
        )
        .unwrap();
        write_records(&mut out, "alpha", &mut pending, &mut trailing_cr, b"", true).unwrap();
        write_records(&mut out, "alpha", &mut pending, &mut trailing_cr, b"", true).unwrap();

        let mut expected =
            b"alpha: line\nalpha: a\ralpha: b\nalpha: c\r\nalpha: d\r\nalpha: e\n".to_vec();
        expected.extend_from_slice(b"alpha: ");
        expected.extend(std::iter::repeat_n(b'x', 8191));
        expected.extend_from_slice(b"\nalpha: tail");
        assert_eq!(out, expected);
    }
}
