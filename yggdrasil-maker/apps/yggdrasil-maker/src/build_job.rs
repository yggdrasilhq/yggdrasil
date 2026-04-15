use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetachedBuildJobRecord {
    pub setup_id: String,
    pub setup_name: String,
    pub setup_path: String,
    pub artifacts_dir: String,
    pub repo_root: Option<String>,
    pub log_path: String,
    pub completion_path: String,
    pub pid: u32,
    pub started_at_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetachedBuildCompletionRecord {
    pub success: bool,
    pub completed_at_ms: u128,
    #[serde(default)]
    pub manifest_path: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}
