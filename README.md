# gita-rs

Rust reimplementation of [nosarthur/gita](https://github.com/nosarthur/gita): manage many git repos with a single `gita` binary. Reads your existing `~/.config/gita/` layout (no migration).

## Features

- **`gita ll`** - parallel repo status (one combined `git` shell script per repo on cache miss: porcelain status + `show-branch` subject + relative time, matching upstream gita), fingerprint cache in `ll-cache.json`, default **64** threads (`--jobs N`)
- Default cold path uses **`git status -uno`** (no untracked scan); use **`--full-status`** for `?` markers
- **`gita freeze` / `gita clone`** - export and restore repo sets (compatible with upstream freeze CSV)
- Bookkeeping: `add` (`-r`, `-a`), `rm`, `rename`, `ls`, `clear`, `group`, `context`, `info`, `color`, `flags`
- Delegated commands from `cmds.json` (fetch, pull, `st`, …) with **tokio** async when multiple repos
- `gita super` / `gita shell` passthrough
- Config: `$GITA_PROJECT_HOME/gita`, `$XDG_CONFIG_HOME/gita`, or `~/.config/gita`

## Install

```bash
cargo install --path . --locked
```

Ensure `~/.cargo/bin` is before `~/.local/bin` on `PATH` if you previously used the Python `gita` from `uv tool install`.

### Rollback to Python gita

```bash
cargo uninstall gita
uv tool install gita
```

Your `~/.config/gita/` directory is unchanged either way.

## Usage

```bash
gita ll                  # warm: uses ~/.config/gita/ll-cache.json when repos unchanged
gita ll --refresh        # cold: rescan every repo
gita ll --full-status    # include untracked (?) - slower on large trees
gita ll -C               # no ANSI colors
gita ll --jobs 128       # parallelism (I/O-bound; try 64-128 on many repos)
gita ll mygroup          # filter by group
gita ll -g               # group headers
gita freeze > repos.txt
gita clone -f repos.txt
gita clone -p -f repos.txt   # preserve paths from freeze file
gita -v
```

## Shell completions

```bash
# bash
gita completions bash > ~/.local/share/bash-completion/completions/gita

# zsh
gita completions zsh > ~/.local/share/zsh/site-functions/_gita
# then add to .zshrc if needed: fpath=(~/.local/share/zsh/site-functions $fpath)
```

Regenerate after upgrading `gita` if CLI flags change.

## Benchmark

On **259 repos** (`~/.config/gita/repos.csv`, macOS, release build, `--jobs 64`):

| Scenario | Wall time (`/usr/bin/time -p gita ll >/dev/null`) |
|----------|---------------------------------------------------|
| Cold (`--refresh` or no cache) | **~3.6-3.8s** (259× `git status`; git subprocess floor on this machine) |
| Warm (unchanged repos, cache hit) | **~0.03-0.35s** (fingerprint scan of 259 repos adds ~0.3s) |

Cold time is dominated by **one git subprocess per repo** (status + show-branch + log). Large worktrees and `--full-status` increase cost. The cache keys on `HEAD`, index mtime, stash log mtime, and `FETCH_HEAD` mtime (so `gita fetch` invalidates ahead/behind) so normal `gita ll` stays instant when nothing changed.

Measure locally:

```bash
rm -f ~/.config/gita/ll-cache.json
cargo build --release
/usr/bin/time -p ./target/release/gita ll --refresh >/dev/null
/usr/bin/time -p ./target/release/gita ll >/dev/null
```

## Windows

ANSI colors require a terminal that supports escape sequences (Windows Terminal, modern conhost). Use `gita ll -C` if colors garble output.

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## License

MIT
