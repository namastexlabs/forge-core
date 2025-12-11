use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::super::omni::OmniConfig;

/// Project-level configuration stored in .forge/config.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    /// Omni integration settings
    #[serde(default)]
    pub omni: Option<OmniConfig>,
    /// Project-specific settings
    #[serde(default)]
    pub settings: ForgeProjectSettings,
}

/// Project-specific settings for Forge
#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export)]
pub struct ForgeProjectSettings {
    /// Custom branch prefix for this project
    #[serde(default)]
    pub branch_prefix: Option<String>,
    /// Whether to auto-create PRs on task completion
    #[serde(default)]
    pub auto_create_pr: bool,
    /// Default assignees for created PRs
    #[serde(default)]
    pub default_assignees: Vec<String>,
    /// Default labels for created PRs
    #[serde(default)]
    pub default_labels: Vec<String>,
}
