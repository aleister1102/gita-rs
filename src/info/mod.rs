use crate::config::config_path;
use std::collections::HashMap;
use std::fs;

pub const ALL_INFO_ITEMS: &[&str] = &["branch", "branch_name", "commit_msg", "commit_time", "path"];

pub fn ansi_color(name: &str) -> &'static str {
    match name {
        "black" => "\x1b[30m",
        "red" => "\x1b[31m",
        "green" => "\x1b[32m",
        "yellow" => "\x1b[33m",
        "blue" => "\x1b[34m",
        "purple" => "\x1b[35m",
        "cyan" => "\x1b[36m",
        "white" => "\x1b[37m",
        "b_black" => "\x1b[30;1m",
        "b_red" => "\x1b[31;1m",
        "b_green" => "\x1b[32;1m",
        "b_yellow" => "\x1b[33;1m",
        "b_blue" => "\x1b[34;1m",
        "b_purple" => "\x1b[35;1m",
        "b_cyan" => "\x1b[36;1m",
        "b_white" => "\x1b[37;1m",
        "underline" => "\x1B[4m",
        _ => "",
    }
}

pub const ANSI_END: &str = "\x1b[0m";

pub fn default_colors() -> HashMap<String, String> {
    HashMap::from([
        ("no_remote".into(), "white".into()),
        ("in_sync".into(), "green".into()),
        ("diverged".into(), "red".into()),
        ("local_ahead".into(), "purple".into()),
        ("remote_ahead".into(), "yellow".into()),
    ])
}

pub fn load_color_encoding() -> HashMap<String, String> {
    let path = config_path("color.csv");
    if path.is_file() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut lines = content.lines();
        if let (Some(header), Some(row)) = (lines.next(), lines.next()) {
            let keys: Vec<&str> = header.split(',').collect();
            let vals: Vec<&str> = row.split(',').collect();
            let mut map = default_colors();
            for (k, v) in keys.iter().zip(vals.iter()) {
                map.insert(k.to_string(), v.to_string());
            }
            return map;
        }
    }
    default_colors()
}

pub fn default_symbols() -> HashMap<String, String> {
    HashMap::from([
        ("dirty".into(), "*".into()),
        ("staged".into(), "+".into()),
        ("untracked".into(), "?".into()),
        ("stashed".into(), "$".into()),
        ("local_ahead".into(), "↑".into()),
        ("remote_ahead".into(), "↓".into()),
        ("diverged".into(), "⇕".into()),
        ("in_sync".into(), String::new()),
        ("no_remote".into(), "∅".into()),
        ("".into(), String::new()),
    ])
}

pub fn load_symbols() -> HashMap<String, String> {
    let mut symbols = default_symbols();
    let path = config_path("symbols.csv");
    if path.is_file() {
        if let Ok(content) = fs::read_to_string(&path) {
            let mut lines = content.lines();
            if let (Some(header), Some(row)) = (lines.next(), lines.next()) {
                let keys: Vec<&str> = header.split(',').collect();
                let vals: Vec<&str> = row.split(',').collect();
                for (k, v) in keys.iter().zip(vals.iter()) {
                    symbols.insert(k.to_string(), v.to_string());
                }
            }
        }
    }
    symbols
}

pub fn load_info_items() -> Vec<String> {
    let path = config_path("info.csv");
    if path.is_file() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Some(line) = content.lines().next() {
                let items: Vec<String> = line
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| ALL_INFO_ITEMS.contains(&s.as_str()))
                    .collect();
                if !items.is_empty() {
                    return items;
                }
            }
        }
    }
    vec!["branch".into(), "commit_msg".into(), "commit_time".into()]
}

#[derive(Debug, Default)]
pub struct Truncate {
    pub widths: HashMap<String, usize>,
}

impl Truncate {
    pub fn load() -> Self {
        let path = config_path("layout.csv");
        let mut t = Truncate::default();
        if path.is_file() {
            if let Ok(content) = fs::read_to_string(&path) {
                let mut lines = content.lines();
                if let (Some(header), Some(row)) = (lines.next(), lines.next()) {
                    let keys: Vec<&str> = header.split(',').collect();
                    let vals: Vec<&str> = row.split(',').collect();
                    for (k, v) in keys.iter().zip(vals.iter()) {
                        if let Ok(w) = v.trim().parse::<usize>() {
                            t.widths.insert(k.to_string(), w);
                        }
                    }
                }
            }
        }
        t
    }

    pub fn truncate(&self, field: &str, message: &str) -> String {
        let width = match self.widths.get(field) {
            Some(0) | None => return message.to_string(),
            Some(w) => *w,
        };
        if message.len() <= width {
            if width > message.len() && width < 1000 {
                return format!("{message:<width$}");
            }
            return message.to_string();
        }
        let length = if width < 3 { 3 } else { width };
        if message.len() > length {
            format!("{}...", &message[..length.saturating_sub(3)])
        } else {
            message.to_string()
        }
    }
}

pub fn write_color_csv(colors: &HashMap<String, String>) -> std::io::Result<()> {
    let path = config_path("color.csv");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let keys = [
        "no_remote",
        "in_sync",
        "diverged",
        "local_ahead",
        "remote_ahead",
    ];
    let header = keys.join(",");
    let row: Vec<_> = keys
        .iter()
        .map(|k| colors.get(*k).cloned().unwrap_or_default())
        .collect();
    fs::write(path, format!("{header}\n{}\n", row.join(",")))
}

pub fn write_info_csv(items: &[String]) -> std::io::Result<()> {
    let path = config_path("info.csv");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", items.join(",")))
}

pub fn write_layout_defaults() -> std::io::Result<()> {
    let path = config_path("layout.csv");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        "branch,symbols,branch_name,commit_msg,commit_time,path\n19,5,27,0,0,30\n",
    )
}

pub fn color_names() -> Vec<&'static str> {
    vec![
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "purple",
        "cyan",
        "white",
        "b_black",
        "b_red",
        "b_green",
        "b_yellow",
        "b_blue",
        "b_purple",
        "b_cyan",
        "b_white",
        "underline",
    ]
}

pub fn show_colors() {
    let names = color_names();
    for (i, name) in names.iter().enumerate() {
        if *name == "underline" {
            continue;
        }
        print!("{}{:<8} ", ansi_color(name), name);
        if (i + 1) % 9 == 0 {
            println!();
        }
    }
    println!("{ANSI_END}");
    let colors = load_color_encoding();
    let mut situations: Vec<_> = colors.keys().collect();
    situations.sort();
    for situation in situations {
        let cname = colors[situation].as_str();
        println!(
            "{situation:<12}: {}{}{:<8}{ANSI_END} ",
            ansi_color(cname),
            cname,
            ANSI_END
        );
    }
}
