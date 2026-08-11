use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serial_test::serial;
use tempfile::TempDir;

fn write_repo_config(tmp: &TempDir) {
    let config = tmp.path().join("gita");
    fs::create_dir_all(&config).unwrap();
    let alpha = tmp.path().join("alpha");
    let beta = tmp.path().join("beta");
    for path in [&alpha, &beta] {
        git2::Repository::init(path).unwrap();
    }
    fs::write(
        config.join("repos.csv"),
        format!(
            "{},{},,\n{},{},,\n",
            alpha.display(),
            "alpha",
            beta.display(),
            "beta"
        ),
    )
    .unwrap();
}

fn write_fake_git(dir: &Path, body: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let script = dir.join("git");
    fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();
    script
}

fn fake_path_env(dir: &Path) -> String {
    format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn setup_then_clean() -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::env::set_var("GITA_PROJECT_HOME", tmp.path());
    tmp
}

struct ChildProc {
    captured: Vec<u8>,
    notified: bool,
}

fn run_super_progress_probe(tmp: &TempDir, fake_dir: &Path, ready_dir: &Path) -> ChildProc {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gita"))
        .args(["super", "alpha", "beta", "progress-probe"])
        .env("GITA_PROJECT_HOME", tmp.path())
        .env("GITA_READY_DIR", ready_dir)
        .env("PATH", fake_path_env(fake_dir))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut notified = false;
        loop {
            let n = stdout.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if !notified && buf.windows(12).any(|w| w == b"stderr-start") {
                notified = true;
                let _ = tx.send(());
            }
        }
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ready_count = fs::read_dir(ready_dir).map(|e| e.count()).unwrap_or(0);
        if ready_count >= 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fake git processes never signaled readiness"
        );
        thread::sleep(Duration::from_millis(20));
    }

    let notified = rx.recv_timeout(Duration::from_millis(500)).is_ok();

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("gita child did not exit within 5 seconds");
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "gita exited with {status:?}");

    let captured = reader.join().unwrap();
    ChildProc { captured, notified }
}

#[test]
#[serial]
fn delegated_command_streams_progress_before_exit() {
    let tmp = setup_then_clean();
    write_repo_config(&tmp);
    let fake_dir = tmp.path().join("fakebin");
    let ready_dir = tmp.path().join("ready");
    fs::create_dir_all(&ready_dir).unwrap();
    write_fake_git(
        &fake_dir,
        r#"if [ "$1" != "progress-probe" ]; then
    echo "unexpected argv: $*" >&2
    exit 1
fi
printf 'stderr-start\r' >&2
printf 'stdout-start\n'
: > "$GITA_READY_DIR/$$"
sleep 2
printf 'stderr-done\n' >&2
printf 'stdout-done\n'
"#,
    );

    let result = run_super_progress_probe(&tmp, &fake_dir, &ready_dir);
    let captured = String::from_utf8(result.captured).unwrap();
    assert!(
        result.notified,
        "no record arrived within 500 ms after both repos signaled readiness; captured:\n{captured}"
    );
    for repo in ["alpha", "beta"] {
        for record in ["stdout-start", "stdout-done", "stderr-start", "stderr-done"] {
            assert!(
                captured.contains(&format!("{repo}: {record}")),
                "missing {repo}: {record} in:\n{captured}"
            );
        }
    }
    std::env::remove_var("GITA_PROJECT_HOME");
}

#[test]
#[serial]
fn network_commands_force_progress() {
    let tmp = setup_then_clean();
    write_repo_config(&tmp);
    let fake_dir = tmp.path().join("fakebin");
    write_fake_git(
        &fake_dir,
        r#"printf '%s\n' "$*" > "$GITA_ARG_LOG_DIR/$1-$$"
"#,
    );

    for cmd in ["fetch", "pull"] {
        let log_dir = tmp.path().join(format!("args-{cmd}"));
        fs::create_dir_all(&log_dir).unwrap();
        let status = Command::new(env!("CARGO_BIN_EXE_gita"))
            .arg(cmd)
            .env("GITA_PROJECT_HOME", tmp.path())
            .env("GITA_ARG_LOG_DIR", &log_dir)
            .env("PATH", fake_path_env(&fake_dir))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .status()
            .unwrap();
        assert!(status.success(), "gita {cmd} exited with {status:?}");

        let mut logged: Vec<String> = fs::read_dir(&log_dir)
            .unwrap()
            .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        logged.sort();
        assert_eq!(
            logged,
            vec![format!("{cmd} --progress\n"), format!("{cmd} --progress\n")],
            "gita {cmd} did not run the network command with --progress"
        );
    }
    std::env::remove_var("GITA_PROJECT_HOME");
}
