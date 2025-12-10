pub mod error;
pub mod mcp;
pub mod middleware;
pub mod routes;

// #[cfg(feature = "cloud")]
// type DeploymentImpl = automagik_forge_cloud::forge_core_deployment::CloudDeployment;
// #[cfg(not(feature = "cloud"))]
pub type DeploymentImpl = forge_core_local_deployment::LocalDeployment;
