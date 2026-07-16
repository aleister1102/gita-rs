use std::fs;

use serial_test::serial;
use tempfile::TempDir;

fn set_project_home(tmp: &TempDir) {
    std::env::set_var("GITA_PROJECT_HOME", tmp.path());
}

fn clear_project_home() {
    std::env::remove_var("GITA_PROJECT_HOME");
}

#[test]
#[serial]
fn default_workspace_is_root_config_dir() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    let root = gita::config::workspace_root();
    let cfg = gita::config::config_dir();
    assert_eq!(cfg, root);

    clear_project_home();
}

#[test]
#[serial]
fn create_and_switch_workspaces() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    gita::config::create_workspace("work", false).unwrap();
    gita::config::set_workspace("work").unwrap();

    let root = gita::config::workspace_root();
    assert_eq!(
        gita::config::config_dir(),
        root.join("workspaces").join("work")
    );
    assert_eq!(gita::config::current_workspace(), Some("work".to_string()));

    gita::config::set_workspace("default").unwrap();
    assert_eq!(gita::config::config_dir(), root);
    assert_eq!(gita::config::current_workspace(), None);

    clear_project_home();
}

#[test]
#[serial]
fn workspace_add_copies_repos_and_groups() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    let root = gita::config::workspace_root();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("repos.csv"), "/tmp/a,repo-a,,\n").unwrap();
    fs::write(root.join("groups.csv"), "g1:repo-a:\n").unwrap();

    gita::config::create_workspace("work", true).unwrap();

    let ws = root.join("workspaces").join("work");
    assert!(ws.join("repos.csv").is_file());
    assert!(ws.join("groups.csv").is_file());
    let repos = gita::config::load_repos(true).unwrap();
    assert_eq!(repos["repo-a"].path, "/tmp/a");

    clear_project_home();
}

#[test]
#[serial]
fn list_workspaces() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    assert!(gita::config::list_workspaces().unwrap().is_empty());
    gita::config::create_workspace("a", false).unwrap();
    gita::config::create_workspace("b", false).unwrap();
    assert_eq!(gita::config::list_workspaces().unwrap(), vec!["a", "b"]);

    clear_project_home();
}

#[test]
#[serial]
fn remove_workspace_refuses_active() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    gita::config::create_workspace("work", false).unwrap();
    gita::config::set_workspace("work").unwrap();

    let err = gita::config::remove_workspace("work").unwrap_err();
    assert!(err.to_string().contains("active workspace"));

    gita::config::set_workspace("default").unwrap();
    gita::config::remove_workspace("work").unwrap();
    assert!(!gita::config::workspace_dir("work").exists());

    clear_project_home();
}

#[test]
#[serial]
fn rename_workspace_updates_active_file() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    gita::config::create_workspace("old", false).unwrap();
    gita::config::set_workspace("old").unwrap();
    gita::config::rename_workspace("old", "new").unwrap();

    assert!(!gita::config::workspace_dir("old").exists());
    assert!(gita::config::workspace_dir("new").exists());
    assert_eq!(gita::config::current_workspace(), Some("new".to_string()));

    clear_project_home();
}

#[test]
#[serial]
fn cli_workspace_show_and_switch() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    let cmds = gita::delegate::load_cmds().unwrap();
    gita::cli::run_with_delegate(&["workspace".into(), "add".into(), "work".into()], &cmds)
        .unwrap();
    gita::cli::run_with_delegate(&["workspace".into(), "use".into(), "work".into()], &cmds)
        .unwrap();

    assert_eq!(gita::config::current_workspace(), Some("work".to_string()));

    gita::cli::run_with_delegate(&["workspace".into(), "use".into(), "default".into()], &cmds)
        .unwrap();
    assert_eq!(gita::config::current_workspace(), None);

    clear_project_home();
}
