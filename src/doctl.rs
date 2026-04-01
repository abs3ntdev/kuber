use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

/// A doctl auth context from `doctl auth list`.
#[derive(Debug, Deserialize)]
pub struct AuthContext {
    pub name: String,
}

/// A node pool as returned by doctl.
#[derive(Debug, Deserialize)]
struct DoctlNodePool {
    name: String,
    size: String,
    #[serde(default)]
    count: u32,
    #[serde(default)]
    min_nodes: Option<u32>,
    #[serde(default)]
    max_nodes: Option<u32>,
}

/// Raw cluster data as returned by `doctl kubernetes cluster list`.
#[derive(Debug, Deserialize)]
struct DoctlCluster {
    id: String,
    name: String,
    region: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    status: DoctlClusterStatus,
    #[serde(default)]
    ha: bool,
    #[serde(default)]
    node_pools: Vec<DoctlNodePool>,
    #[serde(default)]
    created_at: String,
}

#[derive(Debug, Default, Deserialize)]
struct DoctlClusterStatus {
    #[serde(default)]
    state: String,
}

/// Node pool summary stored in metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePoolInfo {
    pub name: String,
    pub size: String,
    pub count: u32,
    #[serde(default)]
    pub min_nodes: Option<u32>,
    #[serde(default)]
    pub max_nodes: Option<u32>,
}

/// Cluster info enriched with the doctl auth context it belongs to.
///
/// Note: `endpoint` is intentionally excluded from metadata to avoid
/// persisting API server URLs on disk. The cluster `id` is retained
/// because it's needed to fetch kubeconfigs via doctl.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterInfo {
    pub id: String,
    pub name: String,
    pub region: String,
    pub doctl_context: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub ha: bool,
    #[serde(default)]
    pub node_pools: Vec<NodePoolInfo>,
    #[serde(default)]
    pub created_at: String,
}

impl ClusterInfo {
    /// The kubernetes context name as it appears in the kubeconfig from doctl.
    /// doctl generates names like `do-<region>-<cluster-name>`.
    pub fn kube_context_name(&self) -> String {
        format!("do-{}-{}", self.region, self.name)
    }
}

/// Get all doctl auth contexts, excluding the "default" placeholder.
pub fn list_auth_contexts() -> anyhow::Result<Vec<AuthContext>> {
    let output = Command::new("doctl")
        .args(["auth", "list", "-o", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
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
pub fn list_clusters(doctl_context: &str) -> anyhow::Result<Vec<ClusterInfo>> {
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
            version: c.version,
            status: c.status.state,
            ha: c.ha,
            node_pools: c
                .node_pools
                .into_iter()
                .map(|p| NodePoolInfo {
                    name: p.name,
                    size: p.size,
                    count: p.count,
                    min_nodes: p.min_nodes,
                    max_nodes: p.max_nodes,
                })
                .collect(),
            created_at: c.created_at,
        })
        .collect())
}

/// Download the kubeconfig for a specific cluster.
/// Uses the `--context` flag so no global auth state is mutated.
pub fn download_kubeconfig(doctl_context: &str, cluster_id: &str) -> anyhow::Result<String> {
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
        .context("Failed to download kubeconfig")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to download kubeconfig for cluster {cluster_id}: {stderr}");
    }

    String::from_utf8(output.stdout).context("Kubeconfig output is not valid UTF-8")
}
