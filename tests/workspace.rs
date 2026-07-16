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

#[test]
#[serial]
fn invalid_workspace_names_are_rejected() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    for bad in ["", ".", "..", "a/b", "a\\b"] {
        let err = gita::config::create_workspace(bad, false).unwrap_err();
        assert!(
            !err.to_string().contains("already exists"),
            "'{bad}' should be rejected for invalid name, not duplicate"
        );
        assert!(
            gita::config::set_workspace(bad).is_err(),
            "set_workspace('{bad}') should be rejected"
        );
        assert!(
            gita::config::remove_workspace(bad).is_err(),
            "remove_workspace('{bad}') should be rejected"
        );
        assert!(
            gita::config::rename_workspace(bad, "x").is_err(),
            "rename_workspace('{bad}', 'x') should be rejected"
        );
    }

    clear_project_home();
}

#[test]
#[serial]
fn create_workspace_fails_when_already_exists() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    gita::config::create_workspace("work", false).unwrap();
    let err = gita::config::create_workspace("work", false).unwrap_err();
    assert!(err.to_string().contains("already exists"));

    clear_project_home();
}

#[test]
#[serial]
fn stale_active_workspace_pointer_is_cleared() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    gita::config::create_workspace("work", false).unwrap();
    gita::config::set_workspace("work").unwrap();
    fs::remove_dir_all(gita::config::workspace_dir("work")).unwrap();

    // config_dir should fall back to root and current_workspace should be None
    assert_eq!(gita::config::config_dir(), gita::config::workspace_root());
    assert_eq!(gita::config::current_workspace(), None);

    clear_project_home();
}

#[test]
#[serial]
fn corrupted_active_workspace_pointer_is_cleared() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    let root = gita::config::workspace_root();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("workspace"), "../\n").unwrap();

    // config_dir should fall back to root and the corrupted pointer should be removed
    assert_eq!(gita::config::config_dir(), root);
    assert_eq!(gita::config::current_workspace(), None);
    assert!(!root.join("workspace").exists());

    clear_project_home();
}

#[test]
#[serial]
fn from_current_copies_all_config_files() {
    let tmp = TempDir::new().unwrap();
    set_project_home(&tmp);

    let root = gita::config::workspace_root();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("repos.csv"), "/tmp/a,repo-a,,\n").unwrap();
    fs::write(root.join("groups.csv"), "g1:repo-a:\n").unwrap();
    fs::write(root.join("color.csv"), "no_remote,white\n").unwrap();
    fs::write(root.join("info.csv"), "branch,commit_msg\n").unwrap();

    gita::config::create_workspace("work", true).unwrap();

    let ws = root.join("workspaces").join("work");
    assert_eq!(
        fs::read_to_string(ws.join("repos.csv")).unwrap(),
        "/tmp/a,repo-a,,\n"
    );
    assert_eq!(
        fs::read_to_string(ws.join("groups.csv")).unwrap(),
        "g1:repo-a:\n"
    );
    assert_eq!(
        fs::read_to_string(ws.join("color.csv")).unwrap(),
        "no_remote,white\n"
    );
    assert_eq!(
        fs::read_to_string(ws.join("info.csv")).unwrap(),
        "branch,commit_msg\n"
    );
    // active-workspace pointer should not be copied into the workspace
    assert!(!ws.join("workspace").exists());

    clear_project_home();
}
