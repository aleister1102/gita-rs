use std::collections::HashMap;

use crate::config::RepoProp;
use crate::info::{self, Truncate, ANSI_END};
use crate::ll::repo_status::RepoSnapshot;

pub struct FormatCtx {
    pub symbols: HashMap<String, String>,
    pub colors: HashMap<String, String>,
    pub items: Vec<String>,
    pub truncator: Truncate,
    pub no_colors: bool,
}

impl FormatCtx {
    pub fn load(no_colors: bool) -> Self {
        FormatCtx {
            symbols: info::load_symbols(),
            colors: info::load_color_encoding(),
            items: info::load_info_items(),
            truncator: Truncate::load(),
            no_colors,
        }
    }
}

pub fn format_row(_name: &str, prop: &RepoProp, snap: &RepoSnapshot, ctx: &FormatCtx) -> String {
    if let Some(err) = &snap.error {
        return format!("[error: {err}]");
    }
    let mut parts = Vec::new();
    for item in &ctx.items {
        let s = match item.as_str() {
            "branch" => format_branch(prop, snap, ctx),
            "branch_name" => ctx.truncator.truncate("branch_name", &snap.branch),
            "commit_msg" => ctx.truncator.truncate("commit_msg", &snap.commit_msg),
            "commit_time" => ctx.truncator.truncate("commit_time", &snap.commit_time),
            "path" => format!(
                "{}{}{}",
                info::ansi_color("cyan"),
                ctx.truncator.truncate("path", &prop.path),
                ANSI_END
            ),
            _ => String::new(),
        };
        parts.push(s);
    }
    parts.join(" ")
}

fn format_branch(_prop: &RepoProp, snap: &RepoSnapshot, ctx: &FormatCtx) -> String {
    let branch = ctx.truncator.truncate("branch", &snap.branch);
    let sym = format!(
        "[{}{}{}{}{}]",
        ctx.symbols
            .get(snap.dirty.as_str())
            .unwrap_or(&String::new()),
        ctx.symbols
            .get(snap.staged.as_str())
            .unwrap_or(&String::new()),
        ctx.symbols
            .get(snap.stashed.as_str())
            .unwrap_or(&String::new()),
        ctx.symbols
            .get(snap.untracked.as_str())
            .unwrap_or(&String::new()),
        ctx.symbols
            .get(snap.situ.as_str())
            .unwrap_or(&String::new()),
    );
    let sym = ctx.truncator.truncate("symbols", &sym);
    let info = format!("{branch:<10} {sym}");
    if ctx.no_colors {
        return format!("{info:<18}");
    }
    let color_name = ctx
        .colors
        .get(&snap.situ)
        .map(|s| s.as_str())
        .unwrap_or("white");
    format!(
        "{}{info:<18}{ANSI_END}",
        info::ansi_color(color_name),
        info = info,
    )
}
