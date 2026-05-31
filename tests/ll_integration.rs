use std::fs;
use std::process::Command;

use serial_test::serial;
use tempfile::TempDir;

fn init_repo(path: &std::path::Path, msg: &str) {
    fs::create_dir_all(path).unwrap();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .unwrap();
    fs::write(path.join("README"), "x").unwrap();
    Command::new("git")
        .args(["add", "README"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", msg])
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t.com")
        .current_dir(path)
        .output()
        .unwrap();
}

#[test]
#[serial]
fn ll_output_shape_has_name_branch_and_commit() {
    let tmp = TempDir::new().unwrap();
    let repo_path = tmp.path().join("sample");
    init_repo(&repo_path, "initial commit");

    let config = tmp.path().join("gita");
    fs::create_dir_all(&config).unwrap();
    let csv = format!("{},{},,\n", repo_path.display(), "sample");
    fs::write(config.join("repos.csv"), csv).unwrap();

    std::env::set_var("GITA_PROJECT_HOME", tmp.path());
    let repos = gita::config::load_repos(true).unwrap();
    assert_eq!(repos.len(), 1);

    let lines = gita::ll::run_ll(
        &repos,
        &std::collections::HashMap::new(),
        &gita::ll::LlOptions {
            group: None,
            by_group: false,
            no_colors: true,
            jobs: 4,
            refresh: true,
            no_untracked: true,
        },
    );
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert!(line.starts_with("sample "));
    assert!(line.contains("main"));
    assert!(line.contains("initial commit"));
    assert!(line.contains('[') && line.contains(']'));
    std::env::remove_var("GITA_PROJECT_HOME");
}

#[test]
#[serial]
fn ll_continues_on_bad_repo() {
    let tmp = TempDir::new().unwrap();
    let config = tmp.path().join("gita");
    fs::create_dir_all(&config).unwrap();
    let good = tmp.path().join("good");
    init_repo(&good, "ok");
    fs::write(
        config.join("repos.csv"),
        format!(
            "{}/missing,bad,,\n{},{},,\n",
            tmp.path().display(),
            good.display(),
            "good"
        ),
    )
    .unwrap();
    std::env::set_var("GITA_PROJECT_HOME", tmp.path());
    let repos = gita::config::load_repos(true).unwrap();
    let lines = gita::ll::run_ll(
        &repos,
        &std::collections::HashMap::new(),
        &gita::ll::LlOptions {
            group: None,
            by_group: false,
            no_colors: true,
            jobs: 2,
            refresh: true,
            no_untracked: true,
        },
    );
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|l| l.contains("good")));
    assert!(lines.iter().any(|l| l.contains("[error:")));
    std::env::remove_var("GITA_PROJECT_HOME");
}
