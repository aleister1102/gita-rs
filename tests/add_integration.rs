#![cfg(unix)]

use std::fs;
use std::process::Command;

use tempfile::TempDir;

#[test]
fn add_skips_a_repo_registered_through_another_path() {
    let tmp = TempDir::new().unwrap();
    let real = tmp.path().join("real");
    let repo = real.join("repo");
    git2::Repository::init(&repo).unwrap();
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let config = tmp.path().join("gita");
    fs::create_dir_all(&config).unwrap();
    let registered = format!("{},repo,,\n", link.join("repo").display());
    fs::write(config.join("repos.csv"), &registered).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_gita"))
        .arg("add")
        .arg(&repo)
        .env("GITA_PROJECT_HOME", tmp.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "gita add failed: {out:?}");
    assert_eq!(
        fs::read_to_string(config.join("repos.csv")).unwrap(),
        registered
    );
}
