use std::collections::HashSet;
use std::io::Write;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use colored::Colorize;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::cache;
use crate::doctl;

/// Discover all clusters across all doctl auth contexts and save metadata.
async fn full_sync() -> anyhow::Result<Vec<doctl::ClusterInfo>> {
    eprintln!(
        "{}",
        "Discovering clusters across all doctl contexts...".blue()
    );

    let contexts = doctl::list_auth_contexts().await?;
    let mut all_clusters = Vec::new();

    for ctx in &contexts {
        match doctl::list_clusters(&ctx.name).await {
            Ok(clusters) => all_clusters.extend(clusters),
            Err(e) => {
                eprintln!(
                    "{}",
                    format!("Warning: failed to list clusters for '{}': {e}", ctx.name).yellow()
                );
            }
        }
    }

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

/// Download a kubeconfig for a cluster if not already cached.
async fn ensure_hydrated(cluster: &doctl::ClusterInfo) -> anyhow::Result<()> {
    let filename = cache::config_filename(&cluster.doctl_context, &cluster.name);
    let path = cache::configs_dir().join(&filename);

    if path.exists() {
        return Ok(());
    }

    eprintln!(
        "{}",
        format!("Fetching kubeconfig for {}...", cluster.kube_context_name()).yellow()
    );

    let content = doctl::download_kubeconfig(&cluster.doctl_context, &cluster.id).await?;
    cache::write_config(cluster, &content)?;

    Ok(())
}

/// Launch fzf fed by cached metadata instantly, with a background doctl sync that
/// streams newly discovered context names into fzf live. When the user makes a
/// selection (or cancels), the background sync is aborted immediately.
///
/// Returns `(selected context name, best-available cluster list)` or `None` if cancelled.
async fn pick_context_with_live_sync() -> anyhow::Result<Option<(String, Vec<doctl::ClusterInfo>)>>
{
    let cached_clusters = cache::load_metadata()?.unwrap_or_default();
    let cached_names: HashSet<String> = cached_clusters
        .iter()
        .map(doctl::ClusterInfo::kube_context_name)
        .collect();

    let (tx, rx) = mpsc::unbounded_channel::<String>();

    // Send cached names immediately so fzf has content before the sync starts.
    for name in &cached_names {
        let _ = tx.send(name.clone());
    }

    // Shared accumulator: background sync appends here as it discovers clusters.
    let discovered: Arc<Mutex<Vec<doctl::ClusterInfo>>> =
        Arc::new(Mutex::new(cached_clusters.clone()));

    // Background sync: iterates doctl contexts, streams new names into fzf,
    // saves metadata incrementally.
    let discovered_bg = Arc::clone(&discovered);
    let sync_handle = tokio::spawn(async move {
        let Ok(contexts) = doctl::list_auth_contexts().await else {
            return;
        };

        for ctx in &contexts {
            let Ok(clusters) = doctl::list_clusters(&ctx.name).await else {
                continue;
            };

            let mut new_names = Vec::new();
            {
                let mut acc = discovered_bg.lock().unwrap();
                for cluster in &clusters {
                    let name = cluster.kube_context_name();
                    if !acc.iter().any(|c| c.kube_context_name() == name) {
                        acc.push(cluster.clone());
                        if !cached_names.contains(&name) {
                            new_names.push(name);
                        }
                    }
                }
                let _ = cache::save_metadata(&acc);
            }

            for name in new_names {
                if tx.send(name).is_err() {
                    return; // fzf closed, user already picked
                }
            }
        }
    });

    let mut fzf = std::process::Command::new("fzf")
        .args(["--height=~40%", "--reverse", "--prompt=context> "])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to launch fzf. Is it installed?")?;

    let fzf_stdin = fzf.stdin.take().context("Failed to open fzf stdin")?;

    // Feed names into fzf as they arrive from the channel.
    let writer_handle = tokio::task::spawn_blocking(move || {
        let mut stdin = fzf_stdin;
        let mut rx = rx;
        while let Some(name) = rx.blocking_recv() {
            if writeln!(stdin, "{name}").is_err() {
                break;
            }
        }
    });

    let output = tokio::task::spawn_blocking(move || fzf.wait_with_output())
        .await?
        .context("Failed to wait for fzf")?;

    // Kill the background sync immediately so we don't waste time or network.
    sync_handle.abort();
    let _ = writer_handle.await;

    if !output.status.success() {
        return Ok(None);
    }

    let selection = String::from_utf8(output.stdout)?.trim().to_string();
    if selection.is_empty() {
        return Ok(None);
    }

    let clusters = discovered.lock().unwrap().clone();

    Ok(Some((selection, clusters)))
}

/// Show cached contexts instantly via fzf with live background refresh,
/// hydrate only the selected context, launch kubie.
pub async fn ctx(context: Option<String>) -> anyhow::Result<()> {
    let (ctx_name, clusters) = match context {
        Some(name) => {
            // Direct context name -- try cache first, full sync if not found.
            let mut clusters = cache::load_metadata()?.unwrap_or_default();
            if cache::find_config_for_context(&name, &clusters).is_none() {
                clusters = full_sync().await?;
            }
            (name, clusters)
        }
        None => match pick_context_with_live_sync().await? {
            Some(result) => result,
            None => return Ok(()),
        },
    };

    let (_, cluster) = cache::find_config_for_context(&ctx_name, &clusters)
        .context(format!("Unknown context: {ctx_name}"))?;

    ensure_hydrated(&cluster).await?;

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
        .await
        .context("Failed to launch kubie")?;

    // Clean up the ephemeral kubeconfig now that the kubie shell has exited.
    let _ = std::fs::remove_file(&config_file);

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}
