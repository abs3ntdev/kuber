use std::process::Stdio;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// A doctl auth context from `doctl auth list`.
#[derive(Debug, Deserialize)]
pub struct AuthContext {
    pub name: String,
}

/// Raw cluster data as returned by `doctl kubernetes cluster list`.
#[derive(Debug, Deserialize)]
struct DoctlCluster {
    id: String,
    name: String,
    region: String,
}

/// Cluster info enriched with the doctl auth context it belongs to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterInfo {
    pub id: String,
    pub name: String,
    pub region: String,
    pub doctl_context: String,
}

impl ClusterInfo {
    /// The kubernetes context name as it appears in the kubeconfig from doctl.
    /// doctl generates names like `do-<region>-<cluster-name>`.
    pub fn kube_context_name(&self) -> String {
        format!("do-{}-{}", self.region, self.name)
    }
}

/// Get all doctl auth contexts, excluding the "default" placeholder.
pub async fn list_auth_contexts() -> anyhow::Result<Vec<AuthContext>> {
    let output = Command::new("doctl")
        .args(["auth", "list", "-o", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("Failed to run doctl auth list")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("doctl auth list failed: {stderr}");
    }

    let contexts: Vec<AuthContext> =
        serde_json::from_slice(&output.stdout).context("Failed to parse doctl auth list output")?;

    Ok(contexts
        .into_iter()
        .filter(|c| c.name != "default")
        .collect())
}

/// List kubernetes clusters for a doctl auth context.
/// Uses the `--context` flag so no global auth state is mutated.
pub async fn list_clusters(doctl_context: &str) -> anyhow::Result<Vec<ClusterInfo>> {
    let output = Command::new("doctl")
        .args([
            "kubernetes",
            "cluster",
            "list",
            "--context",
            doctl_context,
            "-o",
            "json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("Failed to list kubernetes clusters")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("doctl kubernetes cluster list failed: {stderr}");
    }

    let clusters: Vec<DoctlCluster> = serde_json::from_slice(&output.stdout)
        .context("Failed to parse doctl kubernetes cluster list output")?;

    Ok(clusters
        .into_iter()
        .map(|c| ClusterInfo {
            id: c.id,
            name: c.name,
            region: c.region,
            doctl_context: doctl_context.to_string(),
        })
        .collect())
}

/// Download the kubeconfig for a specific cluster.
/// Uses the `--context` flag so no global auth state is mutated.
pub async fn download_kubeconfig(doctl_context: &str, cluster_id: &str) -> anyhow::Result<String> {
    let output = Command::new("doctl")
        .args([
            "kubernetes",
            "cluster",
            "kubeconfig",
            "show",
            cluster_id,
            "--context",
            doctl_context,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("Failed to download kubeconfig")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to download kubeconfig for cluster {cluster_id}: {stderr}");
    }

    String::from_utf8(output.stdout).context("Kubeconfig output is not valid UTF-8")
}
