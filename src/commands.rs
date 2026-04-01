use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use colored::Colorize;
use skim::prelude::*;

use crate::cache;
use crate::doctl;

/// Fetch cluster lists from all doctl contexts in parallel using scoped threads.
/// Returns the combined list of clusters.
fn fetch_all_clusters(contexts: &[doctl::AuthContext]) -> Vec<doctl::ClusterInfo> {
    let results: Mutex<Vec<doctl::ClusterInfo>> = Mutex::new(Vec::new());

    std::thread::scope(|s| {
        for ctx in contexts {
            let results = &results;
            s.spawn(move || match doctl::list_clusters(&ctx.name) {
                Ok(clusters) => {
                    results.lock().unwrap().extend(clusters);
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        format!("Warning: failed to list clusters for '{}': {e}", ctx.name)
                            .yellow()
                    );
                }
            });
        }
    });

    results.into_inner().unwrap()
}

/// Discover all clusters across all doctl auth contexts and save metadata.
/// Replaces the cached list entirely -- stale clusters are removed.
fn full_sync() -> anyhow::Result<Vec<doctl::ClusterInfo>> {
    eprintln!(
        "{}",
        "Discovering clusters across all doctl contexts...".blue()
    );

    let contexts = doctl::list_auth_contexts()?;
    let all_clusters = fetch_all_clusters(&contexts);

    if all_clusters.is_empty() {
        anyhow::bail!("No kubernetes clusters found across any doctl context.");
    }

    eprintln!(
        "{}",
        format!("Found {} cluster(s).", all_clusters.len()).green()
    );

    cache::save_metadata(&all_clusters)?;
    Ok(all_clusters)
}

/// Download a kubeconfig for a cluster if not cached or older than 24 hours.
fn ensure_hydrated(cluster: &doctl::ClusterInfo) -> anyhow::Result<()> {
    let filename = cache::config_filename(&cluster.doctl_context, &cluster.name);
    let path = cache::configs_dir().join(&filename);

    if path.exists() {
        let fresh = path.metadata().and_then(|m| m.modified()).is_ok_and(|t| {
            t.elapsed()
                .is_ok_and(|age| age < std::time::Duration::from_secs(24 * 60 * 60))
        });
        if fresh {
            return Ok(());
        }
    }

    eprintln!(
        "{}",
        format!("Fetching kubeconfig for {}...", cluster.kube_context_name()).yellow()
    );

    let content = doctl::download_kubeconfig(&cluster.doctl_context, &cluster.id)?;
    cache::write_config(cluster, &content)?;

    Ok(())
}

/// Interactive context picker using skim. Cached names appear instantly.
/// When `no_sync` is false, a background thread syncs from doctl and pushes
/// new names into skim live. Stale clusters are removed from metadata.
#[allow(clippy::too_many_lines)]
fn pick_context_with_live_sync(
    no_sync: bool,
) -> anyhow::Result<Option<(String, Vec<doctl::ClusterInfo>)>> {
    // If no metadata exists, do a full sync first so the user sees all clusters.
    // This happens regardless of --no-sync since there's nothing to show otherwise.
    let cached_clusters = match cache::load_metadata()? {
        Some(clusters) if !clusters.is_empty() => clusters,
        _ => full_sync()?,
    };

    let cached_names: HashSet<String> = cached_clusters
        .iter()
        .map(doctl::ClusterInfo::kube_context_name)
        .collect();

    // Skim reads batches of items from this channel and renders them as they arrive.
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = bounded(50);

    // Send cached names immediately.
    let items: Vec<Arc<dyn SkimItem>> = cached_names
        .iter()
        .map(|name| Arc::new(name.clone()) as Arc<dyn SkimItem>)
        .collect();
    let _ = tx.send(items);

    // Shared cluster list for the preview callback and background sync.
    let clusters_shared: Arc<Mutex<Vec<doctl::ClusterInfo>>> =
        Arc::new(Mutex::new(cached_clusters.clone()));

    if !no_sync {
        // Background thread: discover all clusters in parallel, replace the cached
        // list with the fresh one (removing stale entries), send new names to skim.
        let clusters_bg = Arc::clone(&clusters_shared);
        let tx_bg = tx.clone();
        std::thread::spawn(move || {
            let Ok(contexts) = doctl::list_auth_contexts() else {
                return;
            };

            let fresh_clusters = fetch_all_clusters(&contexts);
            if fresh_clusters.is_empty() {
                return;
            }

            let fresh_names: HashSet<String> = fresh_clusters
                .iter()
                .map(doctl::ClusterInfo::kube_context_name)
                .collect();

            // Find names that are new (not in the cached set).
            let new_names: Vec<String> = fresh_names
                .iter()
                .filter(|name| !cached_names.contains(*name))
                .cloned()
                .collect();

            // Replace the shared cluster list with the fresh one.
            // This removes stale clusters that no longer exist in DO.
            {
                let mut all = clusters_bg.lock().unwrap();
                *all = fresh_clusters;
            }

            // Always save -- even if no new clusters, stale ones may have been removed,
            // or existing cluster metadata (version, node pools) may have changed.
            if let Ok(all) = clusters_bg.lock() {
                let _ = cache::save_metadata(&all);
            }

            // Send new names to skim so they appear in the picker.
            if !new_names.is_empty() {
                let new_items: Vec<Arc<dyn SkimItem>> = new_names
                    .into_iter()
                    .map(|name| Arc::new(name) as Arc<dyn SkimItem>)
                    .collect();
                let _ = tx_bg.send(new_items);
            }
        });
    }

    // Drop our sender so skim knows we're done once the background thread finishes
    // (or immediately if --no-sync).
    drop(tx);

    // Preview callback: look up cluster info by context name and format it.
    let clusters_preview = Arc::clone(&clusters_shared);
    let preview_fn = move |items: Vec<Arc<dyn SkimItem>>| -> Vec<String> {
        let Some(item) = items.first() else {
            return vec![];
        };
        let ctx_name = item.output().to_string();
        let clusters = clusters_preview.lock().unwrap();
        let Some(cluster) = clusters.iter().find(|c| c.kube_context_name() == ctx_name) else {
            return vec![format!("No metadata for {ctx_name}")];
        };

        let mut lines = vec![
            format!("  Cluster:  {}", cluster.name),
            format!("  Region:   {}", cluster.region),
            format!("  Account:  {}", cluster.doctl_context),
        ];

        if !cluster.version.is_empty() {
            lines.push(format!("  Version:  {}", cluster.version));
        }
        if !cluster.status.is_empty() {
            lines.push(format!("  Status:   {}", cluster.status));
        }
        if cluster.ha {
            lines.push("  HA:       yes".to_string());
        }
        if !cluster.node_pools.is_empty() {
            lines.push(String::new());
            lines.push("  Node Pools:".to_string());
            for pool in &cluster.node_pools {
                let scaling = match (pool.min_nodes, pool.max_nodes) {
                    (Some(min), Some(max)) => format!("{min}-{max} nodes (autoscale)"),
                    _ if pool.count > 0 => format!("{} nodes", pool.count),
                    _ => "unknown".to_string(),
                };
                lines.push(format!("    {} ({}, {})", pool.name, pool.size, scaling));
            }
        }

        lines
    };

    let options = SkimOptionsBuilder::default()
        .reverse(true)
        .cycle(true)
        .prompt("context> ")
        .info(skim::tui::statusline::InfoDisplay::Hidden)
        .color("16")
        .preview_fn(preview_fn)
        .build()
        .expect("Failed to build skim options");

    let output = Skim::run_with(options, Some(rx)).map_err(|e| anyhow::anyhow!("{e}"))?;

    if output.is_abort {
        return Ok(None);
    }

    let Some(selected) = output.selected_items.first() else {
        return Ok(None);
    };

    let selection = selected.output().to_string();

    // Use the shared cluster list which includes any updates from the background sync.
    let clusters = clusters_shared.lock().unwrap().clone();

    Ok(Some((selection, clusters)))
}

/// Main entry point.
pub fn ctx(context: Option<String>, no_sync: bool) -> anyhow::Result<()> {
    let (ctx_name, clusters) = match context {
        Some(name) => {
            let mut clusters = cache::load_metadata()?.unwrap_or_default();
            if cache::find_config_for_context(&name, &clusters).is_none() {
                clusters = full_sync()?;
            }
            (name, clusters)
        }
        None => match pick_context_with_live_sync(no_sync)? {
            Some(result) => result,
            None => return Ok(()),
        },
    };

    let (_, cluster) = cache::find_config_for_context(&ctx_name, &clusters)
        .context(format!("Unknown context: {ctx_name}"))?;

    ensure_hydrated(&cluster)?;

    let config_file = cache::configs_dir().join(cache::config_filename(
        &cluster.doctl_context,
        &cluster.name,
    ));

    let status = Command::new("kubie")
        .args(["ctx", "-f"])
        .arg(&config_file)
        .arg(&ctx_name)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to launch kubie")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}
