use anyhow::Error;
use executors::{executors::BaseCodingAgent, profile::ExecutorProfileId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
pub use v7::{
    EditorConfig, EditorType, GitHubConfig, ShowcaseState, SoundFile, ThemeMode, UiLanguage,
};

use crate::services::config::versions::v7;

fn default_git_branch_prefix() -> String {
    "af".to_string()
}

fn default_sound_volume() -> u8 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct NotificationConfig {
    pub sound_enabled: bool,
    pub push_enabled: bool,
    pub sound_file: SoundFile,
    #[serde(default = "default_sound_volume")]
    pub sound_volume: u8, // 0-100 percentage
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            sound_enabled: true,
            push_enabled: true,
            sound_file: SoundFile::GenieNotify1,
            sound_volume: default_sound_volume(),
        }
    }
}

impl From<v7::NotificationConfig> for NotificationConfig {
    fn from(old: v7::NotificationConfig) -> Self {
        Self {
            sound_enabled: old.sound_enabled,
            push_enabled: old.push_enabled,
            sound_file: old.sound_file,
            sound_volume: default_sound_volume(), // Default to 100% for backward compatibility
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct Config {
    pub config_version: String,
    pub theme: ThemeMode,
    pub executor_profile: ExecutorProfileId,
    pub disclaimer_acknowledged: bool,
    pub onboarding_acknowledged: bool,
    pub github_login_acknowledged: bool,
    pub telemetry_acknowledged: bool,
    pub notifications: NotificationConfig,
    pub editor: EditorConfig,
    pub github: GitHubConfig,
    pub analytics_enabled: Option<bool>,
    pub contact_email_opt_in: Option<bool>,
    pub contact_username_opt_in: Option<bool>,
    pub workspace_dir: Option<String>,
    pub last_app_version: Option<String>,
    pub show_release_notes: bool,
    #[serde(default)]
    pub language: UiLanguage,
    #[serde(default = "default_git_branch_prefix")]
    pub git_branch_prefix: String,
    #[serde(default)]
    pub showcases: ShowcaseState,
}

impl Config {
    pub fn from_previous_version(raw_config: &str) -> Result<Self, Error> {
        let old_config = match serde_json::from_str::<v7::Config>(raw_config) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::error!("❌ Failed to parse config: {}", e);
                tracing::error!("   at line {}, column {}", e.line(), e.column());
                return Err(e.into());
            }
        };

        Ok(Self {
            config_version: "v8".to_string(),
            theme: old_config.theme,
            executor_profile: old_config.executor_profile,
            disclaimer_acknowledged: old_config.disclaimer_acknowledged,
            onboarding_acknowledged: old_config.onboarding_acknowledged,
            github_login_acknowledged: old_config.github_login_acknowledged,
            telemetry_acknowledged: old_config.telemetry_acknowledged,
            notifications: NotificationConfig::from(old_config.notifications),
            editor: old_config.editor,
            github: old_config.github,
            analytics_enabled: old_config.analytics_enabled,
            contact_email_opt_in: old_config.contact_email_opt_in,
            contact_username_opt_in: old_config.contact_username_opt_in,
            workspace_dir: old_config.workspace_dir,
            last_app_version: old_config.last_app_version,
            show_release_notes: old_config.show_release_notes,
            language: old_config.language,
            git_branch_prefix: old_config.git_branch_prefix,
            showcases: old_config.showcases,
        })
    }
}

impl From<String> for Config {
    fn from(raw_config: String) -> Self {
        if let Ok(config) = serde_json::from_str::<Config>(&raw_config)
            && config.config_version == "v8"
        {
            return config;
        }

        match Self::from_previous_version(&raw_config) {
            Ok(config) => {
                tracing::info!("Config upgraded to v8");
                config
            }
            Err(e) => {
                tracing::warn!("Config migration failed: {}, using default", e);
                Self::default()
            }
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: "v8".to_string(),
            theme: ThemeMode::System,
            executor_profile: ExecutorProfileId::new(BaseCodingAgent::ClaudeCode),
            disclaimer_acknowledged: false,
            onboarding_acknowledged: false,
            github_login_acknowledged: false,
            telemetry_acknowledged: false,
            notifications: NotificationConfig::default(),
            editor: EditorConfig::default(),
            github: GitHubConfig::default(),
            analytics_enabled: None,
            contact_email_opt_in: None,
            contact_username_opt_in: None,
            workspace_dir: None,
            last_app_version: None,
            show_release_notes: false,
            language: UiLanguage::default(),
            git_branch_prefix: default_git_branch_prefix(),
            showcases: ShowcaseState::default(),
        }
    }
}