use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use walkdir::WalkDir;

use crate::config::{
    self, delete_repo_from_groups, load_groups, load_repos, make_repo_name, write_groups,
    write_repos, GroupProp, RepoProp,
};
use crate::delegate::{self, CmdDef};
use crate::git_util::{is_git, relative_path_depth};
use crate::info;
use crate::ll::{self, LlOptions};

fn default_ll_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(16)
}

#[derive(Parser)]
#[command(name = "gita", version, about = "Manage multiple git repos")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(about = "add repo(s)")]
    Add {
        paths: Vec<String>,
        #[arg(short = 'n', long = "dry-run")]
        dry_run: bool,
        #[arg(short = 'g', long = "group")]
        group: Option<String>,
        #[arg(long = "group-path")]
        gpath: Option<String>,
        #[arg(short = 's', long = "skip-submodule")]
        skip_submodule: bool,
        #[arg(short = 'r', long = "recursive")]
        recursive: bool,
        #[arg(short = 'a', long = "auto-group")]
        auto_group: bool,
        #[arg(short = 'b', long = "bare")]
        bare: bool,
    },
    #[command(about = "remove repo(s)")]
    Rm { repo: Vec<String> },
    #[command(about = "rename a repo")]
    Rename { repo: String, new_name: String },
    #[command(about = "display summary of all repos")]
    Ll {
        group: Option<String>,
        #[arg(short = 'C', long = "no-colors")]
        no_colors: bool,
        #[arg(short = 'g')]
        by_group: bool,
        #[arg(long = "jobs", default_value_t = default_ll_jobs())]
        jobs: usize,
        #[arg(long, help = "Bypass ll snapshot cache")]
        refresh: bool,
        #[arg(
            long = "full-status",
            help = "Scan untracked files (?); default uses git -uno for speed"
        )]
        full_status: bool,
    },
    #[command(about = "show repo(s) or repo path")]
    Ls { repo: Option<String> },
    #[command(about = "group repos")]
    Group {
        #[command(subcommand)]
        cmd: Option<GroupCommands>,
    },
    #[command(about = "set context")]
    Context { choice: Option<String> },
    #[command(about = "information setting")]
    Info {
        #[command(subcommand)]
        cmd: Option<InfoCommands>,
    },
    #[command(about = "color configuration")]
    Color {
        #[command(subcommand)]
        cmd: Option<ColorCommands>,
    },
    #[command(about = "git flags configuration")]
    Flags {
        #[command(subcommand)]
        cmd: Option<FlagsCommands>,
    },
    #[command(about = "print repo info for gita clone")]
    Freeze {
        #[arg(short = 'g', long = "group")]
        group: Option<String>,
    },
    #[command(about = "clone repos")]
    Clone {
        clonee: String,
        #[arg(short = 'C', long = "directory")]
        directory: Option<String>,
        #[arg(short = 'p', long = "preserve-path")]
        preserve_path: bool,
        #[arg(short = 'n', long = "dry-run")]
        dry_run: bool,
        #[arg(short = 'g', long = "group")]
        group: Option<String>,
        #[arg(short = 'f', long = "from-file")]
        from_file: bool,
    },
    #[command(about = "removes all groups and repositories")]
    Clear,
    #[command(about = "generate shell completions", hide = true)]
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    #[command(about = "run any git command")]
    Super {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        man: Vec<String>,
        #[arg(short = 'q', long = "quote-mode")]
        quote_mode: bool,
    },
    #[command(about = "run any shell command")]
    Shell {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        man: Vec<String>,
        #[arg(short = 'q', long = "quote-mode")]
        quote_mode: bool,
    },
    #[command(about = "delegated git command", name = "delegate")]
    Delegate {
        name: String,
        #[arg(trailing_var_arg = true)]
        repo: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum GroupCommands {
    #[command(about = "List all groups with repos")]
    Ll { to_show: Option<String> },
    #[command(about = "List all group names")]
    Ls,
    #[command(about = "Add repo(s) to a group")]
    Add {
        to_group: Vec<String>,
        #[arg(short = 'n', long = "name")]
        gname: String,
        #[arg(short = 'p', long = "path")]
        gpath: Option<String>,
    },
    #[command(about = "remove repo(s) from a group")]
    Rmrepo {
        to_rm: Vec<String>,
        #[arg(short = 'n', long = "name")]
        gname: String,
    },
    #[command(about = "Change group name")]
    Rename { gname: String, new_name: String },
    #[command(about = "Remove group(s)")]
    Rm { to_ungroup: Vec<String> },
}

#[derive(Subcommand)]
pub enum InfoCommands {
    #[command(about = "show used and unused information items")]
    Ll,
    #[command(about = "Enable information item")]
    Add { info_item: String },
    #[command(about = "Disable information item")]
    Rm { info_item: String },
    #[command(about = "Set default column widths")]
    SetLength,
}

#[derive(Subcommand)]
pub enum ColorCommands {
    #[command(about = "display available colors")]
    Ll,
    #[command(about = "reset color scheme")]
    Reset,
    #[command(about = "Set color for situation")]
    Set { situation: String, color: String },
}

#[derive(Subcommand)]
pub enum FlagsCommands {
    #[command(about = "display repos with custom flags")]
    Ll,
    #[command(about = "Set flags for repo")]
    Set {
        repo: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        flags: Vec<String>,
    },
}

pub fn run_with_delegate(argv: &[String], cmds: &HashMap<String, CmdDef>) -> Result<()> {
    if argv.is_empty() {
        print_help(cmds);
        return Ok(());
    }
    let sub = &argv[0];
    if let Some(def) = cmds.get(sub) {
        return run_delegated_cmd(sub, def, &argv[1..], cmds);
    }
    match parse_builtin(argv) {
        Ok(cli) => execute(cli, cmds),
        Err(_) => {
            eprintln!("unknown command: {sub}");
            print_help(cmds);
            std::process::exit(1);
        }
    }
}

fn parse_builtin(argv: &[String]) -> Result<Cli> {
    let mut args = vec!["gita".to_string()];
    args.extend_from_slice(argv);
    Ok(Cli::try_parse_from(args)?)
}

pub fn execute(cli: Cli, cmds: &HashMap<String, CmdDef>) -> Result<()> {
    match cli.command {
        Some(Commands::Add {
            paths,
            dry_run,
            group,
            gpath,
            skip_submodule,
            recursive,
            auto_group,
            bare,
        }) => cmd_add(
            paths,
            dry_run,
            group,
            gpath,
            skip_submodule,
            recursive,
            auto_group,
            bare,
        ),
        Some(Commands::Rm { repo }) => cmd_rm(repo),
        Some(Commands::Rename { repo, new_name }) => cmd_rename(repo, new_name),
        Some(Commands::Ll {
            group,
            no_colors,
            by_group,
            jobs,
            refresh,
            full_status,
        }) => cmd_ll(group, no_colors, by_group, jobs, refresh, full_status),
        Some(Commands::Ls { repo }) => cmd_ls(repo),
        Some(Commands::Group { cmd }) => cmd_group(cmd),
        Some(Commands::Context { choice }) => cmd_context(choice),
        Some(Commands::Info { cmd }) => cmd_info(cmd),
        Some(Commands::Color { cmd }) => cmd_color(cmd),
        Some(Commands::Flags { cmd }) => cmd_flags(cmd),
        Some(Commands::Freeze { group }) => crate::freeze_clone::cmd_freeze(group),
        Some(Commands::Clone {
            clonee,
            directory,
            preserve_path,
            dry_run,
            group,
            from_file,
        }) => crate::freeze_clone::cmd_clone(
            clonee,
            directory,
            preserve_path,
            dry_run,
            group,
            from_file,
        ),
        Some(Commands::Clear) => cmd_clear(),
        Some(Commands::Completions { shell }) => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "gita", &mut std::io::stdout());
            Ok(())
        }
        Some(Commands::Super { man, quote_mode }) => cmd_super(man, quote_mode, cmds),
        Some(Commands::Shell { man, quote_mode }) => cmd_shell(man, quote_mode),
        Some(Commands::Delegate { name, repo }) => {
            if let Some(def) = cmds.get(&name) {
                run_delegated_cmd(&name, def, &repo, cmds)
            } else {
                bail!("unknown delegated command: {name}");
            }
        }
        None => {
            print_help(cmds);
            Ok(())
        }
    }
}

fn print_help(cmds: &HashMap<String, CmdDef>) {
    println!("gita {}", crate::VERSION);
    println!("sub-commands:");
    println!("  ll, ls, add, rm, rename, clear, group, context, info, color, flags");
    println!("  super, shell");
    for (name, def) in cmds {
        println!("  {name}: {}", def.help);
    }
}

fn run_delegated_cmd(
    name: &str,
    def: &CmdDef,
    repo_args: &[String],
    cmds: &HashMap<String, CmdDef>,
) -> Result<()> {
    let repos = load_repos(false)?;
    let groups = load_groups(&repos)?;
    let ctx = config::get_context(&groups)?;
    let blacklist: HashSet<String> = delegate::async_blacklist(cmds).into_iter().collect();

    let (chosen, _) = if def.allow_all && repo_args.is_empty() {
        (repos.clone(), vec![])
    } else if def.allow_all {
        delegate::parse_repos_and_rest(
            &repo_args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &repos,
            &groups,
            ctx.as_deref(),
        )
    } else if repo_args.is_empty() {
        delegate::parse_repos_and_rest(&[], &repos, &groups, ctx.as_deref())
    } else {
        delegate::parse_repos_and_rest(
            &repo_args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &repos,
            &groups,
            ctx.as_deref(),
        )
    };

    let base_cmd: Vec<String> = if def.shell {
        vec![def.cmd.clone()]
    } else {
        def.cmd.split_whitespace().map(|s| s.to_string()).collect()
    };
    let disable = blacklist.contains(name) || def.disable_async;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(delegate::exec_git_cmd(
        &chosen, &base_cmd, disable, def.shell,
    ))
}

#[allow(clippy::too_many_arguments)]
fn cmd_add(
    paths: Vec<String>,
    dry_run: bool,
    group: Option<String>,
    gpath: Option<String>,
    skip_submodule: bool,
    recursive: bool,
    auto_group: bool,
    bare: bool,
) -> Result<()> {
    let mut repos = load_repos(false)?;
    let path_refs = paths.clone();
    let mut search_paths = Vec::new();
    for p in paths {
        let abs = Path::new(&p)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(&p));
        if recursive || auto_group {
            for entry in WalkDir::new(&abs).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_dir() {
                    search_paths.push(entry.path().to_string_lossy().to_string());
                }
            }
        } else {
            search_paths.push(abs.to_string_lossy().to_string());
        }
    }
    let existing: HashSet<_> = repos.values().map(|r| r.path.as_str()).collect();
    let mut new_paths: HashSet<String> = HashSet::new();
    for p in search_paths {
        if is_git(&p, bare, skip_submodule) && !existing.contains(p.as_str()) {
            new_paths.insert(p);
        }
    }
    if new_paths.is_empty() {
        println!("No new repos found!");
        return Ok(());
    }
    println!("Found {} new repo(s).", new_paths.len());
    if dry_run {
        for p in &new_paths {
            println!("{p}");
        }
        return Ok(());
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for p in &new_paths {
        let base = Path::new(p)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("repo")
            .to_string();
        *counts.entry(base).or_insert(0) += 1;
    }
    let mut new_repos = HashMap::new();
    for path in new_paths {
        let name = make_repo_name(&path, &repos, &counts);
        new_repos.insert(
            name,
            RepoProp {
                path,
                repo_type: String::new(),
                flags: vec![],
            },
        );
    }
    for (name, prop) in &new_repos {
        repos.insert(name.clone(), prop.clone());
    }
    write_repos(&repos)?;
    if auto_group {
        let refs: Vec<&str> = path_refs.iter().map(|s| s.as_str()).collect();
        let groups = auto_group_paths(&new_repos, &refs);
        if !groups.is_empty() {
            println!("Created {} new group(s).", groups.len());
            let mut existing = load_groups(&repos)?;
            for (k, v) in groups {
                existing.insert(k, v);
            }
            write_groups(&existing)?;
        }
    }
    if let Some(gname) = group {
        let mut groups = load_groups(&repos)?;
        let names: Vec<String> = new_repos.keys().cloned().collect();
        if groups.contains_key(&gname) {
            let entry = groups.get_mut(&gname).unwrap();
            for n in &names {
                if !entry.repos.contains(n) {
                    entry.repos.push(n.clone());
                }
            }
            entry.repos.sort();
            if let Some(gp) = gpath {
                entry.path = gp;
            }
            write_groups(&groups)?;
        } else {
            groups.insert(
                gname.clone(),
                GroupProp {
                    repos: {
                        let mut v = names;
                        v.sort();
                        v
                    },
                    path: gpath.unwrap_or_default(),
                },
            );
            write_groups(&groups)?;
        }
        println!("Added {} repos to the {gname} group", new_repos.len());
    }
    Ok(())
}

fn auto_group_paths(
    new_repos: &HashMap<String, RepoProp>,
    paths: &[&str],
) -> HashMap<String, GroupProp> {
    let mut new_groups: HashMap<String, GroupProp> = HashMap::new();
    for (repo_name, prop) in new_repos {
        for p in paths {
            if let Some(rel) = relative_path_depth(Path::new(&prop.path), p) {
                if rel.is_empty() {
                    continue;
                }
                for i in 1..=rel.len() {
                    let group_name = rel[..i].join("-");
                    let gpath = Path::new(p).join(rel[..i].iter().collect::<PathBuf>());
                    let entry = new_groups.entry(group_name).or_insert_with(|| GroupProp {
                        repos: vec![],
                        path: gpath.to_string_lossy().to_string(),
                    });
                    if !entry.repos.contains(repo_name) {
                        entry.repos.push(repo_name.clone());
                    }
                }
                break;
            }
        }
    }
    new_groups
}

fn cmd_rm(repo_names: Vec<String>) -> Result<()> {
    let mut repos = load_repos(false)?;
    let mut groups = load_groups(&repos)?;
    let mut group_updated = false;
    for repo in repo_names {
        repos.remove(&repo);
        if delete_repo_from_groups(&repo, &mut groups) {
            group_updated = true;
        }
    }
    if group_updated {
        write_groups(&groups)?;
    }
    write_repos(&repos)?;
    Ok(())
}

fn cmd_rename(repo: String, new_name: String) -> Result<()> {
    let mut repos = load_repos(false)?;
    if repos.contains_key(&new_name) {
        println!("{new_name} is already in use!");
        return Ok(());
    }
    let Some(prop) = repos.remove(&repo) else {
        bail!("repo not found: {repo}");
    };
    repos.insert(new_name.clone(), prop);
    write_repos(&repos)?;
    let mut groups = load_groups(&repos)?;
    for g in groups.values_mut() {
        if let Some(pos) = g.repos.iter().position(|r| r == &repo) {
            g.repos.remove(pos);
            g.repos.push(new_name.clone());
            g.repos.sort();
        }
    }
    write_groups(&groups)?;
    Ok(())
}

fn cmd_ll(
    group: Option<String>,
    no_colors: bool,
    by_group: bool,
    jobs: usize,
    refresh: bool,
    full_status: bool,
) -> Result<()> {
    // Skip per-repo validation; `ll` isolates open/status errors per row.
    let repos = load_repos(true)?;
    let groups = load_groups(&repos)?;
    let mut group = group;
    if group.is_none() {
        group = config::get_context(&groups)?;
    }
    let lines = ll::run_ll(
        &repos,
        &groups,
        &LlOptions {
            group,
            by_group,
            no_colors,
            jobs,
            refresh,
            no_untracked: !full_status,
        },
    );
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

fn cmd_ls(repo: Option<String>) -> Result<()> {
    let repos = load_repos(false)?;
    if let Some(name) = repo {
        if let Some(prop) = repos.get(&name) {
            println!("{}", prop.path);
        } else {
            bail!("repo not found: {name}");
        }
    } else {
        let mut names: Vec<String> = repos.keys().cloned().collect();
        names.sort();
        println!("{}", names.join(" "));
    }
    Ok(())
}

fn cmd_group(cmd: Option<GroupCommands>) -> Result<()> {
    let repos = load_repos(false)?;
    let mut groups = load_groups(&repos)?;
    match cmd {
        None | Some(GroupCommands::Ll { to_show: None }) => {
            for (group, prop) in &groups {
                print!(
                    "{}{}{}: ",
                    info::ansi_color("underline"),
                    group,
                    info::ANSI_END
                );
                println!("{}", prop.path);
                for r in &prop.repos {
                    println!(" - {r}");
                }
            }
        }
        Some(GroupCommands::Ll { to_show: Some(g) }) => {
            if let Some(prop) = groups.get(&g) {
                println!("{}", prop.repos.join(" "));
            }
        }
        Some(GroupCommands::Ls) => {
            let mut names: Vec<String> = groups.keys().cloned().collect();
            names.sort();
            println!("{}", names.join(" "));
        }
        Some(GroupCommands::Rename { gname, new_name }) => {
            validate_group_name(&new_name, &repos, &groups, true)?;
            if groups.contains_key(&new_name) {
                eprintln!("Cannot use group name {new_name} since it's already in use.");
                std::process::exit(1);
            }
            let prop = groups.remove(&gname).context("group not found")?;
            groups.insert(new_name.clone(), prop);
            write_groups(&groups)?;
            if let Some(ctx_path) = config::context_file_path() {
                if ctx_path.file_stem().and_then(|s| s.to_str()) == Some(&gname) {
                    config::replace_context(Some(&ctx_path), &new_name)?;
                }
            }
        }
        Some(GroupCommands::Rm { to_ungroup }) => {
            let ctx_path = config::context_file_path();
            for name in to_ungroup {
                groups.remove(&name);
                if let Some(ref ctx) = ctx_path {
                    if ctx.file_stem().and_then(|s| s.to_str()) == Some(&name) {
                        config::replace_context(Some(ctx), "none")?;
                    }
                }
            }
            write_groups(&groups)?;
        }
        Some(GroupCommands::Add {
            to_group,
            gname,
            gpath,
        }) => {
            validate_group_name(&gname, &repos, &groups, false)?;
            match groups.entry(gname) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    for r in to_group {
                        if !e.get().repos.contains(&r) {
                            e.get_mut().repos.push(r);
                        }
                    }
                    e.get_mut().repos.sort();
                    if let Some(p) = gpath {
                        e.get_mut().path = p;
                    }
                    write_groups(&groups)?;
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    let mut v = to_group;
                    v.sort();
                    e.insert(GroupProp {
                        repos: v,
                        path: gpath.unwrap_or_default(),
                    });
                    write_groups(&groups)?;
                }
            }
        }
        Some(GroupCommands::Rmrepo { to_rm, gname }) => {
            if let Some(entry) = groups.get_mut(&gname) {
                entry.repos.retain(|r| !to_rm.contains(r));
                write_groups(&groups)?;
            }
        }
    }
    Ok(())
}

fn validate_group_name(
    name: &str,
    repos: &HashMap<String, RepoProp>,
    groups: &HashMap<String, GroupProp>,
    exclude_old: bool,
) -> Result<()> {
    if repos.contains_key(name) {
        eprintln!("Cannot use group name {name} since it's a repo name.");
        std::process::exit(1);
    }
    if exclude_old && groups.contains_key(name) {
        eprintln!("Cannot use group name {name} since it's already in use.");
        std::process::exit(1);
    }
    if name == "none" || name == "auto" {
        eprintln!("Cannot use group name {name} since it's a reserved keyword.");
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_context(choice: Option<String>) -> Result<()> {
    let repos = load_repos(false)?;
    let groups = load_groups(&repos)?;
    let ctx_path = config::context_file_path();
    match choice {
        None => {
            if let Some(ctx) = config::get_context(&groups)? {
                if let Some(prop) = groups.get(&ctx) {
                    println!("{}: {}", ctx, prop.repos.join(" "));
                }
            } else if config::config_path("auto.context").exists() {
                println!("auto: none detected!");
            } else {
                println!("Context is not set");
            }
        }
        Some(c) => {
            config::replace_context(ctx_path.as_deref(), &c)?;
        }
    }
    Ok(())
}

fn cmd_info(cmd: Option<InfoCommands>) -> Result<()> {
    let items = info::load_info_items();
    match cmd {
        None | Some(InfoCommands::Ll) => {
            println!("In use: {}", items.join(","));
            let unused: Vec<_> = info::ALL_INFO_ITEMS
                .iter()
                .filter(|i| !items.iter().any(|x| x == *i))
                .collect();
            if !unused.is_empty() {
                println!(
                    "Unused: {}",
                    unused.into_iter().copied().collect::<Vec<_>>().join(",")
                );
            }
        }
        Some(InfoCommands::Add { info_item }) => {
            let mut v = items;
            if !v.contains(&info_item) {
                v.push(info_item);
            }
            info::write_info_csv(&v)?;
        }
        Some(InfoCommands::Rm { info_item }) => {
            let mut v = items;
            v.retain(|x| x != &info_item);
            info::write_info_csv(&v)?;
        }
        Some(InfoCommands::SetLength) => {
            let p = config::config_path("layout.csv");
            info::write_layout_defaults()?;
            println!("Settings are in {}", p.display());
        }
    }
    Ok(())
}

fn cmd_color(cmd: Option<ColorCommands>) -> Result<()> {
    match cmd {
        None | Some(ColorCommands::Ll) => info::show_colors(),
        Some(ColorCommands::Reset) => {
            let p = config::config_path("color.csv");
            let _ = std::fs::remove_file(p);
        }
        Some(ColorCommands::Set { situation, color }) => {
            let mut colors = info::load_color_encoding();
            colors.insert(situation, color);
            info::write_color_csv(&colors)?;
        }
    }
    Ok(())
}

fn cmd_flags(cmd: Option<FlagsCommands>) -> Result<()> {
    let repos = load_repos(false)?;
    match cmd {
        None | Some(FlagsCommands::Ll) => {
            for (r, prop) in &repos {
                if !prop.flags.is_empty() {
                    println!("{r}: {:?}", prop.flags);
                }
            }
        }
        Some(FlagsCommands::Set { repo, flags }) => {
            let mut repos = repos;
            if let Some(prop) = repos.get_mut(&repo) {
                prop.flags = flags;
                write_repos(&repos)?;
            }
        }
    }
    Ok(())
}

fn cmd_clear() -> Result<()> {
    write_repos(&HashMap::new())?;
    write_groups(&HashMap::new())?;
    Ok(())
}

fn cmd_super(man: Vec<String>, quote_mode: bool, cmds: &HashMap<String, CmdDef>) -> Result<()> {
    if man.is_empty() {
        eprintln!("Missing commands");
        std::process::exit(2);
    }
    let repos = load_repos(false)?;
    let groups = load_groups(&repos)?;
    let ctx = config::get_context(&groups)?;
    let input: Vec<String> = man;
    let (chosen, rest) = delegate::parse_repos_and_rest(&input, &repos, &groups, ctx.as_deref());
    if rest.is_empty() {
        eprintln!("Missing commands");
        std::process::exit(2);
    }
    if quote_mode && rest.len() > 1 {
        eprintln!("{} is not a repo or group", rest[0]);
        std::process::exit(2);
    }
    let mut base = vec!["git".to_string()];
    base.extend(rest);
    let blacklist: HashSet<String> = delegate::async_blacklist(cmds).into_iter().collect();
    let disable = blacklist.contains("super");
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(delegate::exec_git_cmd(&chosen, &base, disable, false))
}

fn cmd_shell(man: Vec<String>, quote_mode: bool) -> Result<()> {
    if man.is_empty() {
        eprintln!("Missing commands");
        std::process::exit(2);
    }
    let repos = load_repos(false)?;
    let groups = load_groups(&repos)?;
    let ctx = config::get_context(&groups)?;
    let (chosen, rest) = delegate::parse_repos_and_rest(&man, &repos, &groups, ctx.as_deref());
    if rest.is_empty() {
        eprintln!("Missing commands");
        std::process::exit(2);
    }
    if quote_mode && !rest.is_empty() {
        // quote mode validation
    }
    let cmdline = rest.join(" ");
    for (name, prop) in &chosen {
        let output = Command::new("sh")
            .arg("-c")
            .arg(&cmdline)
            .current_dir(&prop.path)
            .output()?;
        let combined = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        let mut text = combined.to_string();
        text.push_str(&err);
        print!("{}", delegate::format_output(&text, name));
    }
    Ok(())
}
