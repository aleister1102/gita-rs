pub mod format;
pub mod repo_status;

use std::collections::HashMap;

use rayon::prelude::*;

use crate::config::{GroupProp, RepoProp};
use crate::ll::format::{format_row, FormatCtx};
use crate::ll::repo_status::{collect_snapshot, LlCache, RepoFingerprint, SnapshotOpts};

pub struct LlOptions {
    pub group: Option<String>,
    pub by_group: bool,
    pub no_colors: bool,
    pub jobs: usize,
    pub refresh: bool,
    pub no_untracked: bool,
}

pub fn run_ll(
    repos: &HashMap<String, RepoProp>,
    groups: &HashMap<String, GroupProp>,
    opts: &LlOptions,
) -> Vec<String> {
    let mut filtered: Vec<(String, RepoProp)> =
        repos.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    if let Some(g) = &opts.group {
        if let Some(prop) = groups.get(g) {
            filtered.retain(|(k, _)| prop.repos.contains(k));
        } else {
            filtered.clear();
        }
    }

    let mut cache = if opts.refresh {
        LlCache::default()
    } else {
        LlCache::load()
    };
    let snap_opts = SnapshotOpts {
        no_untracked: opts.no_untracked,
    };

    let mut cache_dirty = false;
    let lines = if opts.by_group {
        run_by_group(
            &filtered,
            groups,
            opts,
            &mut cache,
            snap_opts,
            &mut cache_dirty,
        )
    } else {
        describe_pairs(&filtered, opts, &mut cache, snap_opts, &mut cache_dirty)
    };

    if cache_dirty {
        cache.save();
    }
    lines
}

fn run_by_group(
    filtered: &[(String, RepoProp)],
    groups: &HashMap<String, GroupProp>,
    opts: &LlOptions,
    cache: &mut LlCache,
    snap_opts: SnapshotOpts,
    cache_dirty: &mut bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(g) = &opts.group {
        lines.push(format!("{g}:"));
        for line in describe_pairs(filtered, opts, cache, snap_opts, cache_dirty) {
            lines.push(format!("  {line}"));
        }
    } else {
        for (g, prop) in groups {
            lines.push(format!("{g}:"));
            let g_repos: Vec<_> = filtered
                .iter()
                .filter(|(k, _)| prop.repos.contains(k))
                .cloned()
                .collect();
            for line in describe_pairs(&g_repos, opts, cache, snap_opts, cache_dirty) {
                lines.push(format!("  {line}"));
            }
        }
    }
    lines
}

struct RepoWork {
    name: String,
    prop: RepoProp,
    fp: RepoFingerprint,
    cached: Option<repo_status::RepoSnapshot>,
}

type LlRow = (
    String,
    String,
    Option<(String, RepoFingerprint, repo_status::RepoSnapshot)>,
);

fn describe_pairs(
    repos: &[(String, RepoProp)],
    opts: &LlOptions,
    cache: &mut LlCache,
    snap_opts: SnapshotOpts,
    cache_dirty: &mut bool,
) -> Vec<String> {
    if repos.is_empty() {
        return vec![];
    }
    let ctx = std::sync::Arc::new(FormatCtx::load(opts.no_colors));
    let name_width = repos.iter().map(|(k, _)| k.len()).max().unwrap_or(0) + 1;
    let jobs = opts.jobs.max(1);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

    let work: Vec<RepoWork> = if opts.refresh {
        pool.install(|| {
            repos
                .par_iter()
                .map(|(name, prop)| RepoWork {
                    name: name.clone(),
                    prop: prop.clone(),
                    fp: RepoFingerprint::empty(),
                    cached: None,
                })
                .collect()
        })
    } else {
        let scanned = pool.install(|| {
            repos
                .par_iter()
                .map(|(name, prop)| {
                    let fp = RepoFingerprint::read(prop).unwrap_or_else(RepoFingerprint::empty);
                    (name.clone(), prop.clone(), fp)
                })
                .collect::<Vec<_>>()
        });
        scanned
            .into_iter()
            .map(|(name, prop, fp)| {
                let cached = cache.get_snap(&prop.path, &fp);
                RepoWork {
                    name,
                    prop,
                    fp,
                    cached,
                }
            })
            .collect()
    };

    let refresh = opts.refresh;
    let mut rows: Vec<LlRow> = pool.install(|| {
        work.into_par_iter()
            .map(|item| {
                let fp = if refresh {
                    RepoFingerprint::read(&item.prop).unwrap_or_else(RepoFingerprint::empty)
                } else {
                    item.fp
                };
                let from_cache = item
                    .cached
                    .as_ref()
                    .map(|s| s.error.is_none())
                    .unwrap_or(false);
                let snap = if from_cache {
                    item.cached.unwrap()
                } else {
                    collect_snapshot(&item.prop, snap_opts)
                };
                let cache_update = if !from_cache && snap.error.is_none() {
                    Some((item.prop.path.clone(), fp, snap.clone()))
                } else {
                    None
                };
                let body = format_row(&item.name, &item.prop, &snap, &ctx);
                (
                    item.name.clone(),
                    format!("{:<name_width$} {body}", item.name),
                    cache_update,
                )
            })
            .collect()
    });

    for (_, _, update) in &rows {
        if let Some((path, fp, snap)) = update {
            cache.insert_snap(path.clone(), fp.clone(), snap.clone());
            *cache_dirty = true;
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.into_iter().map(|(_, line, _)| line).collect()
}

#[cfg(test)]
pub fn snapshot_for_test(prop: &RepoProp) -> repo_status::RepoSnapshot {
    collect_snapshot(
        prop,
        SnapshotOpts {
            no_untracked: false,
        },
    )
}
