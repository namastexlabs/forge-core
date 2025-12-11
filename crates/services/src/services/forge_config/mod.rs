//! Forge configuration service
//!
//! This module provides project-level configuration management for Forge,
//! including project settings and integration with the upstream config system.

mod service;
mod types;

pub use service::ForgeConfigService;
pub use types::*;

// Re-export OmniConfig from the omni module for convenience
pub use super::omni::OmniConfig;

// Re-export upstream Config for convenience
pub use super::config::Config;
