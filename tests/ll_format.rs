use gita::config::RepoProp;
use gita::ll::format::{format_row, FormatCtx};
use gita::ll::repo_status::RepoSnapshot;

#[test]
fn format_branch_no_colors() {
    let ctx = FormatCtx {
        symbols: gita::info::default_symbols(),
        colors: gita::info::default_colors(),
        items: vec!["branch".into()],
        truncator: gita::info::Truncate::default(),
        no_colors: true,
    };
    let prop = RepoProp {
        path: "/tmp/r".into(),
        repo_type: String::new(),
        flags: vec![],
    };
    let snap = RepoSnapshot {
        branch: "main".into(),
        dirty: "dirty".into(),
        staged: "staged".into(),
        untracked: "untracked".into(),
        stashed: String::new(),
        situ: "diverged".into(),
        commit_msg: String::new(),
        commit_time: String::new(),
        error: None,
    };
    let row = format_row("myrepo", &prop, &snap, &ctx);
    assert!(row.contains("main"));
    assert!(row.contains("[*+?⇕]") || row.contains('['));
    assert!(!row.contains("\x1b[31m"));
}

#[test]
fn format_error_row() {
    let ctx = FormatCtx::load(true);
    let prop = RepoProp {
        path: "/missing".into(),
        repo_type: String::new(),
        flags: vec![],
    };
    let snap = RepoSnapshot {
        branch: String::new(),
        dirty: String::new(),
        staged: String::new(),
        untracked: String::new(),
        stashed: String::new(),
        situ: String::new(),
        commit_msg: String::new(),
        commit_time: String::new(),
        error: Some("cannot open".into()),
    };
    let row = format_row("bad", &prop, &snap, &ctx);
    assert!(row.contains("[error: cannot open]"));
}
