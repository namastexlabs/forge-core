//! Profile Loader Service
//!
//! Discovers and loads executor profiles from workspace `.genie` folders.
//! Supports hot-reload when profile files change.

mod cache;
mod genie_profiles;

pub use cache::{ProfileCache, ProfileCacheManager};
pub use genie_profiles::{
    AgentFile, AgentFrontmatter, AgentType, Collective, ForgeConfig, ForgeConfigMap, GenieConfig,
    GenieProfileLoader,
};
