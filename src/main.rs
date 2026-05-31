use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmds = gita::delegate::load_cmds()?;
    if args.is_empty() {
        gita::cli::run_with_delegate(&[], &cmds)?;
        return Ok(());
    }
    // Handle -v/--version at top level
    if args == ["-v"] || args == ["--version"] {
        println!("gita {}", gita::VERSION);
        return Ok(());
    }
    gita::cli::run_with_delegate(&args, &cmds)
}
