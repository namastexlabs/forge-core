use std::path::Path;

use super::types::{ForgeProjectSettings, ProjectConfig};
use super::super::config::Config;

/// Service for managing Forge project configuration
#[derive(Debug, Clone)]
pub struct ForgeConfigService {
    /// Global configuration from upstream
    global_config: Config,
    /// Project-specific configuration
    project_config: Option<ProjectConfig>,
}

impl ForgeConfigService {
    /// Create a new configuration service
    pub fn new(global_config: Config) -> Self {
        Self {
            global_config,
            project_config: None,
        }
    }

    /// Load project configuration from a directory
    pub fn load_project_config(&mut self, project_dir: &Path) -> std::io::Result<()> {
        let config_path = project_dir.join(".forge").join("config.json");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            self.project_config = Some(serde_json::from_str(&content).unwrap_or_default());
        }
        Ok(())
    }

    /// Get the global configuration
    pub fn global_config(&self) -> &Config {
        &self.global_config
    }

    /// Get the project configuration if loaded
    pub fn project_config(&self) -> Option<&ProjectConfig> {
        self.project_config.as_ref()
    }

    /// Get project settings, falling back to defaults
    pub fn project_settings(&self) -> ForgeProjectSettings {
        self.project_config
            .as_ref()
            .map(|c| c.settings.clone())
            .unwrap_or_default()
    }

    /// Get the branch prefix, preferring project setting over global
    pub fn branch_prefix(&self) -> String {
        self.project_settings()
            .branch_prefix
            .unwrap_or_else(|| "forge".to_string())
    }

    /// Save project configuration to disk
    pub fn save_project_config(&self, project_dir: &Path) -> std::io::Result<()> {
        if let Some(config) = &self.project_config {
            let forge_dir = project_dir.join(".forge");
            std::fs::create_dir_all(&forge_dir)?;
            let config_path = forge_dir.join("config.json");
            let content = serde_json::to_string_pretty(config)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            std::fs::write(config_path, content)?;
        }
        Ok(())
    }
}
