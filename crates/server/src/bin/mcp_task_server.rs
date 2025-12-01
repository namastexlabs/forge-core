use rmcp::{ServiceExt, transport::stdio};
use server::mcp::belt::BeltServer;
use server::mcp::task_server::TaskServer;
use tracing_subscriber::{EnvFilter, prelude::*};
use utils::{
    port_file::read_port_file,
    sentry::{self as sentry_utils, SentrySource, sentry_layer},
};

/// Helper to serve an MCP server and wait for completion
macro_rules! serve_and_wait {
    ($server:expr) => {{
        let service = $server.serve(stdio()).await.map_err(|e| {
            tracing::error!("serving error: {:?}", e);
            e
        })?;
        service.waiting().await?;
    }};
}

fn main() -> anyhow::Result<()> {
    sentry_utils::init_once(SentrySource::Mcp);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(std::io::stderr)
                        .with_filter(EnvFilter::new("debug")),
                )
                .with(sentry_layer())
                .init();

            let version = env!("CARGO_PKG_VERSION");
            let use_advanced = std::env::var("FORGE_MCP_ADVANCED").is_ok();

            if use_advanced {
                tracing::info!("[MCP] Starting MCP task server (ADVANCED mode) version {version}...");
            } else {
                tracing::info!("[MCP] Starting MCP task server (Belt mode) version {version}...");
            }

            // Read backend port from port file or environment variable
            let base_url = if let Ok(url) = std::env::var("FORGE_BACKEND_URL") {
                tracing::info!("[MCP] Using backend URL from FORGE_BACKEND_URL: {}", url);
                url
            } else {
                let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

                // Get port from environment variables or fall back to port file
                let port = match std::env::var("BACKEND_PORT").or_else(|_| std::env::var("PORT")) {
                    Ok(port_str) => {
                        tracing::info!("[MCP] Using port from environment: {}", port_str);
                        port_str.parse::<u16>().map_err(|e| {
                            anyhow::anyhow!("Invalid port value '{}': {}", port_str, e)
                        })?
                    }
                    Err(_) => {
                        let port = read_port_file("automagik-forge").await?;
                        tracing::info!("[MCP] Using port from port file: {}", port);
                        port
                    }
                };

                let url = format!("http://{}:{}", host, port);
                tracing::info!("[MCP] Using backend URL: {}", url);
                url
            };

            // Use Belt tools by default, TaskServer (advanced) when FORGE_MCP_ADVANCED is set
            if use_advanced {
                tracing::info!("[MCP] Using advanced tools (7 tools)");
                serve_and_wait!(TaskServer::new(&base_url));
            } else {
                tracing::info!("[MCP] Using Belt tools (15 core tools)");
                serve_and_wait!(BeltServer::new(&base_url));
            }

            Ok(())
        })
}
