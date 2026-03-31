use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::doctl::ClusterInfo;

static CACHE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Initialize the cache root directory. Must be called once at startup.
/// Uses `KUBER_CACHE_DIR` env var if set, otherwise defaults to `/dev/shm/kuber-<uid>/`.
pub fn init() {
    CACHE_ROOT.get_or_init(|| {
        if let Ok(dir) = std::env::var("KUBER_CACHE_DIR") {
            PathBuf::from(dir)
        } else {
            // SAFETY: getuid is always safe to call.
            let uid = unsafe { libc::getuid() };
            PathBuf::from(format!("/dev/shm/kuber-{uid}"))
        }
    });
}

fn cache_root() -> &'static Path {
    CACHE_ROOT
        .get()
        .expect("cache::init() must be called before using cache")
}

/// Returns the configs directory under the cache root.
pub fn configs_dir() -> PathBuf {
    cache_root().join("configs")
}

/// Returns the metadata file path under the cache root.
fn metadata_path() -> PathBuf {
    cache_root().join("metadata.json")
}

/// Ensure the cache directories exist with restrictive permissions.
fn ensure_cache_dirs() -> anyhow::Result<()> {
    let dir = configs_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
        fs::set_permissions(cache_root(), fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Save metadata (cluster list) to the cache.
pub fn save_metadata(clusters: &[ClusterInfo]) -> anyhow::Result<()> {
    ensure_cache_dirs()?;
    let json = serde_json::to_string_pretty(clusters)?;
    fs::write(metadata_path(), json)?;
    Ok(())
}

/// Load cached metadata. Returns `None` if no cache exists.
pub fn load_metadata() -> anyhow::Result<Option<Vec<ClusterInfo>>> {
    let path = metadata_path();
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&path)?;
    let clusters: Vec<ClusterInfo> = serde_json::from_str(&data)?;
    Ok(Some(clusters))
}

/// Generate a kubeconfig file name from doctl context and cluster name.
pub fn config_filename(doctl_context: &str, cluster_name: &str) -> String {
    let safe_context: String = doctl_context
        .replace(['@', '.'], "_")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    let safe_cluster: String = cluster_name
        .replace(' ', "_")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    format!("{safe_context}_{safe_cluster}.yaml")
}

/// Write a kubeconfig fetched from doctl to the cache.
pub fn write_config(cluster: &ClusterInfo, content: &str) -> anyhow::Result<PathBuf> {
    ensure_cache_dirs()?;
    let filename = config_filename(&cluster.doctl_context, &cluster.name);
    let path = configs_dir().join(&filename);
    fs::write(&path, content)?;
    Ok(path)
}

/// Find the cluster info and config path for a given kube context name.
pub fn find_config_for_context(
    context_name: &str,
    clusters: &[ClusterInfo],
) -> Option<(PathBuf, ClusterInfo)> {
    clusters
        .iter()
        .find(|c| c.kube_context_name() == context_name)
        .map(|cluster| {
            let filename = config_filename(&cluster.doctl_context, &cluster.name);
            let path = configs_dir().join(&filename);
            (path, cluster.clone())
        })
}
