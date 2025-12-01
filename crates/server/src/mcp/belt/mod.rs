//! Forge MCP Belt Tools
//!
//! The "utility belt" of 15 core tools that provide 100% UI parity for Forge.
//! These are the essential tools for master agent orchestration.
//!
//! ## Hierarchy
//!
//! ```text
//! LEVEL 0: FORGE        - forge (config, executors, mcp_servers)
//! LEVEL 1: PROJECTS     - projects, project
//! LEVEL 2: TASKS        - tasks, task (with create-and-start)
//! LEVEL 3: ATTEMPTS     - attempts, attempt, continue, stop
//! LEVEL 4: GIT & PR     - branch, merge, push, pr
//! ```
//!
//! Process level is abstracted away - attempt is the maximum abstraction level.

pub mod types;

use std::{
    cmp::Ordering,
    str::FromStr,
    sync::{Arc, RwLock},
};

use db::models::{
    project::Project,
    task::{CreateTask, Task, TaskStatus, TaskWithAttemptStatus},
    task_attempt::TaskAttempt,
};
use executors::{executors::BaseCodingAgent, profile::ExecutorProfileId};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, InitializeRequestParam, ProtocolVersion,
        ServerCapabilities, ServerInfo,
    },
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json;
use types::*;
use uuid::Uuid;

use crate::routes::task_attempts::CreateTaskAttemptBody;

// =============================================================================
// REQUEST TYPES
// =============================================================================

/// LEVEL 0: Forge configuration and discovery
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ForgeRequest {
    #[schemars(description = "Action: 'config' (default), 'executors', 'mcp_servers'")]
    pub action: Option<String>,
    #[schemars(description = "For config: key to get/set. For mcp_servers: executor name")]
    pub key: Option<String>,
    #[schemars(description = "New value to set (for config or mcp_servers)")]
    pub value: Option<String>,
}

/// LEVEL 1: Project operations
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProjectRequest {
    #[schemars(description = "Project name or ID")]
    pub name: String,
    #[schemars(
        description = "Action: 'get' (default), 'create', 'update', 'delete', 'branches', 'open'"
    )]
    pub action: Option<String>,
    #[schemars(description = "Path to git repository (for create)")]
    pub path: Option<String>,
}

/// LEVEL 2: List tasks
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TasksRequest {
    #[schemars(description = "Project name or ID (uses default if not specified)")]
    pub project: Option<String>,
    #[schemars(
        description = "Status filter: 'todo', 'in-progress', 'in-review', 'done', 'cancelled'"
    )]
    pub status: Option<String>,
    #[schemars(description = "Maximum number of tasks (default: 50)")]
    pub limit: Option<u32>,
}

/// LEVEL 2: Task operations (including create-and-start)
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TaskRequest {
    #[schemars(description = "Task title/description for create, or task ID/title to find")]
    pub title: String,
    #[schemars(
        description = "Action: 'get' (default), 'create', 'update', 'delete', 'start' (create+start)"
    )]
    pub action: Option<String>,
    #[schemars(description = "Project name or ID")]
    pub project: Option<String>,
    #[schemars(
        description = "Executor: 'CLAUDE_CODE', 'CODEX', 'GEMINI', 'CURSOR_AGENT', 'OPENCODE'"
    )]
    pub executor: Option<String>,
    #[schemars(description = "Base branch for the attempt (defaults to project default)")]
    pub branch: Option<String>,
    #[schemars(description = "Task description (for create)")]
    pub description: Option<String>,
    #[schemars(
        description = "New status (for update): 'todo', 'in-progress', 'in-review', 'done', 'cancelled'"
    )]
    pub status: Option<String>,
}

/// LEVEL 3: List attempts
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AttemptsRequest {
    #[schemars(description = "Task ID or title")]
    pub task: String,
    #[schemars(description = "Include all attempts (default: only active)")]
    pub all: Option<bool>,
}

/// LEVEL 3: Get attempt details with response/history
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AttemptRequest {
    #[schemars(description = "Attempt ID")]
    pub id: String,
    #[schemars(
        description = "Include full conversation history (default: false, only last response)"
    )]
    pub history: Option<bool>,
}

/// LEVEL 3: Continue an attempt
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ContinueRequest {
    #[schemars(description = "Attempt ID")]
    pub attempt: String,
    #[schemars(description = "Follow-up message to send")]
    pub message: String,
    #[schemars(description = "Optional executor variant")]
    pub variant: Option<String>,
}

/// LEVEL 3: Stop an attempt
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StopRequest {
    #[schemars(description = "Attempt ID")]
    pub attempt: String,
}

/// LEVEL 4: Branch operations
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BranchRequest {
    #[schemars(description = "Attempt ID")]
    pub attempt: String,
    #[schemars(description = "Action: 'status' (default), 'change-target'")]
    pub action: Option<String>,
    #[schemars(description = "New target branch (for change-target)")]
    pub target: Option<String>,
}

/// LEVEL 4: Merge attempt branch
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MergeRequest {
    #[schemars(description = "Attempt ID")]
    pub attempt: String,
}

/// LEVEL 4: Push attempt branch
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PushRequest {
    #[schemars(description = "Attempt ID")]
    pub attempt: String,
}

/// LEVEL 4: PR operations
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PrRequest {
    #[schemars(description = "Attempt ID")]
    pub attempt: String,
    #[schemars(description = "Action: 'create' (default), 'attach'")]
    pub action: Option<String>,
    #[schemars(description = "PR title (for create)")]
    pub title: Option<String>,
    #[schemars(description = "PR body/description (for create)")]
    pub body: Option<String>,
    #[schemars(description = "PR number (for attach)")]
    pub pr_number: Option<i64>,
}

// =============================================================================
// BELT TOOLS IMPLEMENTATION
// =============================================================================

const SUPPORTED_PROTOCOL_VERSIONS: [ProtocolVersion; 2] =
    [ProtocolVersion::V_2025_03_26, ProtocolVersion::V_2024_11_05];

/// Belt tools server - the core 15 tools for Forge MCP
#[derive(Debug, Clone)]
pub struct BeltServer {
    client: reqwest::Client,
    base_url: String,
    tool_router: ToolRouter<Self>,
    negotiated_protocol_version: Arc<RwLock<ProtocolVersion>>,
}

impl BeltServer {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.to_string(),
            tool_router: Self::tool_router(),
            negotiated_protocol_version: Arc::new(RwLock::new(Self::latest_supported_protocol())),
        }
    }

    pub fn tool_router_belt() -> ToolRouter<Self> {
        Self::tool_router()
    }

    fn supported_protocol_versions() -> &'static [ProtocolVersion] {
        &SUPPORTED_PROTOCOL_VERSIONS
    }

    fn latest_supported_protocol() -> ProtocolVersion {
        Self::supported_protocol_versions()
            .first()
            .expect("supported protocols list cannot be empty")
            .clone()
    }

    fn minimum_supported_protocol() -> ProtocolVersion {
        Self::supported_protocol_versions()
            .last()
            .expect("supported protocols list cannot be empty")
            .clone()
    }

    fn current_protocol_version(&self) -> ProtocolVersion {
        self.negotiated_protocol_version
            .read()
            .expect("protocol negotiation lock poisoned")
            .clone()
    }

    fn set_negotiated_protocol_version(&self, version: ProtocolVersion) {
        let mut guard = self
            .negotiated_protocol_version
            .write()
            .expect("protocol negotiation lock poisoned");
        *guard = version;
    }

    fn negotiate_protocol_version(
        requested: &ProtocolVersion,
    ) -> Result<ProtocolVersion, ErrorData> {
        for supported in Self::supported_protocol_versions() {
            match requested.partial_cmp(supported) {
                Some(Ordering::Greater) | Some(Ordering::Equal) => {
                    return Ok(supported.clone());
                }
                Some(Ordering::Less) => continue,
                None => {
                    return Err(ErrorData::invalid_params(
                        format!(
                            "Unable to compare requested MCP protocol version ({requested}) with supported versions"
                        ),
                        Some(serde_json::json!({
                            "requested_protocol": requested.to_string(),
                            "supported_protocols": Self::supported_protocol_versions()
                                .iter()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>(),
                        })),
                    ));
                }
            }
        }

        let minimum = Self::minimum_supported_protocol();
        Err(ErrorData::invalid_params(
            format!(
                "Requested MCP protocol version ({requested}) is older than the supported minimum ({minimum})"
            ),
            Some(serde_json::json!({
                "requested_protocol": requested.to_string(),
                "minimum_supported_protocol": minimum.to_string(),
            })),
        ))
    }

    fn server_info_for_version(&self, protocol_version: ProtocolVersion) -> ServerInfo {
        ServerInfo {
            protocol_version,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "automagik-forge-belt".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: Some("Forge Belt Tools".to_string()),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Forge Belt: 15 core tools for task orchestration. \
                HIERARCHY: forge → projects → tasks → attempts → git/pr. \
                START HERE: task(title='...', project='...', action='start') creates AND starts a task. \
                CHECK PROGRESS: attempt(id='...') shows last_response. \
                CONTINUE: continue(attempt='...', message='...'). \
                Process level is abstracted - attempt is the maximum abstraction level.".to_string()
            ),
        }
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn success<T: serde::Serialize>(data: &T) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(data)
                .unwrap_or_else(|_| "Failed to serialize response".to_string()),
        )]))
    }

    fn error(err: BeltError) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::error(vec![Content::text(
            serde_json::to_string_pretty(&err)
                .unwrap_or_else(|_| "Failed to serialize error".to_string()),
        )]))
    }

    async fn send_json<T: serde::de::DeserializeOwned>(
        &self,
        rb: reqwest::RequestBuilder,
    ) -> Result<T, BeltError> {
        #[derive(serde::Deserialize)]
        struct ApiResponse<T> {
            success: bool,
            data: Option<T>,
            message: Option<String>,
        }

        let resp = rb.send().await.map_err(|e| {
            BeltError::new("Failed to connect to Forge API").with_details(e.to_string())
        })?;

        if !resp.status().is_success() {
            return Err(BeltError::new(format!(
                "Forge API error: {}",
                resp.status()
            )));
        }

        let api_response: ApiResponse<T> = resp.json().await.map_err(|e| {
            BeltError::new("Failed to parse Forge API response").with_details(e.to_string())
        })?;

        if !api_response.success {
            return Err(BeltError::new(
                api_response
                    .message
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        api_response
            .data
            .ok_or_else(|| BeltError::new("Forge API response missing data"))
    }

    /// Resolve a project name or ID to a UUID
    async fn resolve_project(&self, name_or_id: &str) -> Result<Uuid, BeltError> {
        // Try parsing as UUID first
        if let Ok(uuid) = Uuid::parse_str(name_or_id) {
            return Ok(uuid);
        }

        // Otherwise, search by name
        let url = self.url("/api/projects");
        let projects: Vec<Project> = self.send_json(self.client.get(&url)).await?;

        projects
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name_or_id))
            .map(|p| p.id)
            .ok_or_else(|| {
                BeltError::new(format!("Project not found: {}", name_or_id)).with_suggestions(vec![
                    "Call projects() to list available projects".to_string(),
                ])
            })
    }

    /// Resolve a task title or ID to a UUID
    async fn resolve_task(
        &self,
        title_or_id: &str,
        project_id: Option<Uuid>,
    ) -> Result<Uuid, BeltError> {
        // Try parsing as UUID first
        if let Ok(uuid) = Uuid::parse_str(title_or_id) {
            return Ok(uuid);
        }

        // Otherwise, search by title
        let url = if let Some(pid) = project_id {
            self.url(&format!("/api/tasks?project_id={}", pid))
        } else {
            self.url("/api/tasks")
        };

        let tasks: Vec<TaskWithAttemptStatus> = self.send_json(self.client.get(&url)).await?;

        tasks
            .iter()
            .find(|t| t.title.eq_ignore_ascii_case(title_or_id))
            .map(|t| t.id)
            .ok_or_else(|| {
                BeltError::new(format!("Task not found: {}", title_or_id))
                    .with_suggestions(vec!["Call tasks() to list available tasks".to_string()])
            })
    }

    /// Get default branch for a project
    async fn get_default_branch(&self, _project_id: Uuid) -> Result<String, BeltError> {
        // Try to get the default branch from git
        // For now, default to "main"
        Ok("main".to_string())
    }
}

#[tool_router]
impl BeltServer {
    // =========================================================================
    // LEVEL 0: FORGE (Global Configuration)
    // =========================================================================

    #[tool(
        description = "Forge global configuration and discovery. Get config, list executors, manage MCP servers. Actions: 'config' (default), 'executors', 'mcp_servers'"
    )]
    async fn forge(
        &self,
        Parameters(ForgeRequest {
            action,
            key: _key,
            value: _value,
        }): Parameters<ForgeRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let action = action.as_deref().unwrap_or("config");

        match action {
            "executors" => {
                // Return list of available executors
                let executors = vec![
                    ExecutorInfo {
                        name: "CLAUDE_CODE".to_string(),
                        description: "Claude Code (Anthropic's coding agent)".to_string(),
                        variants: vec!["DEFAULT".to_string()],
                    },
                    ExecutorInfo {
                        name: "CODEX".to_string(),
                        description: "OpenAI Codex".to_string(),
                        variants: vec!["DEFAULT".to_string()],
                    },
                    ExecutorInfo {
                        name: "GEMINI".to_string(),
                        description: "Google Gemini".to_string(),
                        variants: vec!["DEFAULT".to_string()],
                    },
                    ExecutorInfo {
                        name: "CURSOR_AGENT".to_string(),
                        description: "Cursor IDE Agent".to_string(),
                        variants: vec!["DEFAULT".to_string()],
                    },
                    ExecutorInfo {
                        name: "OPENCODE".to_string(),
                        description: "OpenCode Agent".to_string(),
                        variants: vec!["DEFAULT".to_string()],
                    },
                ];

                Self::success(&ForgeResult {
                    action: "executors".to_string(),
                    config: None,
                    executors: Some(executors),
                    mcp_servers: None,
                })
            }
            "mcp_servers" => {
                // Get MCP servers for an executor
                let url = self.url("/api/config");
                let _config: serde_json::Value = match self.send_json(self.client.get(&url)).await {
                    Ok(c) => c,
                    Err(e) => return Self::error(e),
                };

                Self::success(&ForgeResult {
                    action: "mcp_servers".to_string(),
                    config: None,
                    executors: None,
                    mcp_servers: Some(vec![]), // TODO: Parse from config
                })
            }
            _ => {
                // Default: return config
                let url = self.url("/api/config");
                let config: serde_json::Value = match self.send_json(self.client.get(&url)).await {
                    Ok(c) => c,
                    Err(e) => return Self::error(e),
                };

                Self::success(&ForgeResult {
                    action: "config".to_string(),
                    config: Some(config),
                    executors: None,
                    mcp_servers: None,
                })
            }
        }
    }

    // =========================================================================
    // LEVEL 1: PROJECTS
    // =========================================================================

    #[tool(description = "List all Forge projects with their active task counts.")]
    async fn projects(&self) -> Result<CallToolResult, ErrorData> {
        let url = self.url("/api/projects");
        let projects: Vec<Project> = match self.send_json(self.client.get(&url)).await {
            Ok(p) => p,
            Err(e) => return Self::error(e),
        };

        let summaries: Vec<ProjectSummary> = projects
            .into_iter()
            .map(|p| ProjectSummary {
                id: p.id,
                name: p.name,
                path: p.git_repo_path.to_string_lossy().to_string(),
                active_tasks: 0, // TODO: Count from tasks
                created_at: p.created_at,
            })
            .collect();

        let count = summaries.len();
        Self::success(&ProjectsResult {
            projects: summaries,
            count,
            next_steps: vec![
                "project(name='<name>') - Get project details".to_string(),
                "tasks(project='<name>') - List tasks in a project".to_string(),
            ],
        })
    }

    #[tool(
        description = "Get, create, update, or delete a project. Actions: 'get' (default), 'create', 'update', 'delete', 'branches', 'open'"
    )]
    async fn project(
        &self,
        Parameters(ProjectRequest { name, action, path }): Parameters<ProjectRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let action = action.as_deref().unwrap_or("get");

        match action {
            "create" => {
                let path = match path {
                    Some(p) => p,
                    None => {
                        return Self::error(BeltError::new("Path is required for create action"));
                    }
                };

                #[derive(serde::Serialize)]
                struct CreateProject {
                    name: String,
                    git_repo_path: String,
                }

                let url = self.url("/api/projects");
                let project: Project = match self
                    .send_json(self.client.post(&url).json(&CreateProject {
                        name: name.clone(),
                        git_repo_path: path,
                    }))
                    .await
                {
                    Ok(p) => p,
                    Err(e) => return Self::error(e),
                };

                Self::success(&ProjectResult {
                    action: "create".to_string(),
                    project: Some(ProjectDetails {
                        id: project.id,
                        name: project.name,
                        path: project.git_repo_path.to_string_lossy().to_string(),
                        setup_script: project.setup_script,
                        cleanup_script: project.cleanup_script,
                        dev_script: project.dev_script,
                        created_at: project.created_at,
                        updated_at: project.updated_at,
                    }),
                    branches: None,
                    message: Some("Project created successfully".to_string()),
                    next_steps: vec![format!(
                        "task(title='...', project='{}', action='start') - Create and start a task",
                        project.id
                    )],
                })
            }
            "delete" => {
                let project_id = match self.resolve_project(&name).await {
                    Ok(id) => id,
                    Err(e) => return Self::error(e),
                };

                let url = self.url(&format!("/api/projects/{}", project_id));
                match self
                    .send_json::<serde_json::Value>(self.client.delete(&url))
                    .await
                {
                    Ok(_) => {}
                    Err(e) => return Self::error(e),
                };

                Self::success(&ProjectResult {
                    action: "delete".to_string(),
                    project: None,
                    branches: None,
                    message: Some(format!("Project {} deleted", name)),
                    next_steps: vec!["projects() - List remaining projects".to_string()],
                })
            }
            "branches" => {
                let project_id = match self.resolve_project(&name).await {
                    Ok(id) => id,
                    Err(e) => return Self::error(e),
                };

                let url = self.url(&format!("/api/projects/{}/branches", project_id));
                let branches: Vec<String> = match self.send_json(self.client.get(&url)).await {
                    Ok(b) => b,
                    Err(e) => return Self::error(e),
                };

                Self::success(&ProjectResult {
                    action: "branches".to_string(),
                    project: None,
                    branches: Some(branches),
                    message: None,
                    next_steps: vec![format!(
                        "task(title='...', project='{}', branch='<branch>', action='start')",
                        project_id
                    )],
                })
            }
            _ => {
                // Default: get project details
                let project_id = match self.resolve_project(&name).await {
                    Ok(id) => id,
                    Err(e) => return Self::error(e),
                };

                let url = self.url(&format!("/api/projects/{}", project_id));
                let project: Project = match self.send_json(self.client.get(&url)).await {
                    Ok(p) => p,
                    Err(e) => return Self::error(e),
                };

                Self::success(&ProjectResult {
                    action: "get".to_string(),
                    project: Some(ProjectDetails {
                        id: project.id,
                        name: project.name.clone(),
                        path: project.git_repo_path.to_string_lossy().to_string(),
                        setup_script: project.setup_script,
                        cleanup_script: project.cleanup_script,
                        dev_script: project.dev_script,
                        created_at: project.created_at,
                        updated_at: project.updated_at,
                    }),
                    branches: None,
                    message: None,
                    next_steps: vec![
                        format!("tasks(project='{}') - List tasks", project.name),
                        format!(
                            "project(name='{}', action='branches') - List branches",
                            project.name
                        ),
                    ],
                })
            }
        }
    }

    // =========================================================================
    // LEVEL 2: TASKS
    // =========================================================================

    #[tool(description = "List tasks in a project with optional status filter.")]
    async fn tasks(
        &self,
        Parameters(TasksRequest {
            project,
            status,
            limit,
        }): Parameters<TasksRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        // Resolve project if provided
        let project_id = match project {
            Some(p) => match self.resolve_project(&p).await {
                Ok(id) => Some(id),
                Err(e) => return Self::error(e),
            },
            None => None,
        };

        let url = match project_id {
            Some(pid) => self.url(&format!("/api/tasks?project_id={}", pid)),
            None => self.url("/api/tasks"),
        };

        let all_tasks: Vec<TaskWithAttemptStatus> =
            match self.send_json(self.client.get(&url)).await {
                Ok(t) => t,
                Err(e) => return Self::error(e),
            };

        // Filter by status
        let status_filter = status.as_ref().and_then(|s| TaskStatus::from_str(s).ok());

        let limit = limit.unwrap_or(50) as usize;
        let filtered: Vec<_> = all_tasks
            .into_iter()
            .filter(|t| status_filter.as_ref().is_none_or(|s| &t.status == s))
            .take(limit)
            .map(|t| TaskSummary {
                id: t.id,
                title: t.title.clone(),
                status: t.status.to_string(),
                has_active_attempt: t.has_in_progress_attempt,
                created_at: t.created_at,
            })
            .collect();

        let count = filtered.len();
        Self::success(&TasksResult {
            tasks: filtered,
            count,
            project_id: project_id.unwrap_or(Uuid::nil()),
            filters: TaskFilters {
                status,
                limit: limit as u32,
            },
            next_steps: vec![
                "task(title='<id>', action='get') - Get task details".to_string(),
                "task(title='New task', action='start', project='...') - Create and start"
                    .to_string(),
            ],
        })
    }

    #[tool(
        description = "Create, get, update, delete, or START a task. The 'start' action creates AND starts an attempt in one call - the PRIMARY entry point for work."
    )]
    async fn task(
        &self,
        Parameters(TaskRequest {
            title,
            action,
            project,
            executor,
            branch,
            description,
            status,
        }): Parameters<TaskRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let action = action.as_deref().unwrap_or("get");

        match action {
            "create" | "start" => {
                // For create/start, project is required
                let project_id = match project {
                    Some(p) => match self.resolve_project(&p).await {
                        Ok(id) => id,
                        Err(e) => return Self::error(e),
                    },
                    None => {
                        return Self::error(BeltError::new(
                            "Project is required for create/start action",
                        ));
                    }
                };

                // Create the task
                let url = self.url("/api/tasks");
                let task: Task =
                    match self
                        .send_json(self.client.post(&url).json(
                            &CreateTask::from_title_description(
                                project_id,
                                title.clone(),
                                description,
                            ),
                        ))
                        .await
                    {
                        Ok(t) => t,
                        Err(e) => return Self::error(e),
                    };

                if action == "start" {
                    // Also start an attempt
                    let executor_str = executor.as_deref().unwrap_or("CLAUDE_CODE");
                    let base_executor = match BaseCodingAgent::from_str(
                        &executor_str.replace('-', "_").to_ascii_uppercase(),
                    ) {
                        Ok(e) => e,
                        Err(_) => {
                            return Self::error(BeltError::new(format!(
                                "Unknown executor: {}",
                                executor_str
                            )));
                        }
                    };

                    let base_branch = match branch {
                        Some(b) => b,
                        None => match self.get_default_branch(project_id).await {
                            Ok(b) => b,
                            Err(e) => return Self::error(e),
                        },
                    };

                    let payload = CreateTaskAttemptBody {
                        task_id: task.id,
                        executor_profile_id: ExecutorProfileId {
                            executor: base_executor,
                            variant: None,
                        },
                        base_branch,
                        use_worktree: None,
                    };

                    let attempt_url = self.url("/api/task-attempts");
                    let attempt: TaskAttempt = match self
                        .send_json(self.client.post(&attempt_url).json(&payload))
                        .await
                    {
                        Ok(a) => a,
                        Err(e) => return Self::error(e),
                    };

                    return Self::success(&TaskResult {
                        action: "start".to_string(),
                        task: Some(TaskDetails {
                            id: task.id,
                            title: task.title,
                            description: task.description,
                            status: task.status.to_string(),
                            attempts_count: 1,
                            created_at: task.created_at,
                            updated_at: task.updated_at,
                        }),
                        attempt: Some(AttemptSummary {
                            id: attempt.id,
                            task_id: attempt.task_id,
                            status: "running".to_string(),
                            executor: attempt.executor,
                            branch: attempt.branch,
                            created_at: attempt.created_at,
                        }),
                        message: Some("Task created and attempt started".to_string()),
                        next_steps: vec![
                            format!(
                                "attempt(id='{}') - Check progress and get last response",
                                attempt.id
                            ),
                            format!(
                                "continue(attempt='{}', message='...') - Send follow-up",
                                attempt.id
                            ),
                            format!("stop(attempt='{}') - Stop the attempt", attempt.id),
                        ],
                    });
                }

                Self::success(&TaskResult {
                    action: "create".to_string(),
                    task: Some(TaskDetails {
                        id: task.id,
                        title: task.title,
                        description: task.description,
                        status: task.status.to_string(),
                        attempts_count: 0,
                        created_at: task.created_at,
                        updated_at: task.updated_at,
                    }),
                    attempt: None,
                    message: Some("Task created".to_string()),
                    next_steps: vec![format!(
                        "task(title='{}', action='start') - Start working on task",
                        task.id
                    )],
                })
            }
            "update" => {
                let project_id = match &project {
                    Some(p) => Some(self.resolve_project(p).await.ok()).flatten(),
                    None => None,
                };

                let task_id = match self.resolve_task(&title, project_id).await {
                    Ok(id) => id,
                    Err(e) => return Self::error(e),
                };

                #[derive(serde::Serialize)]
                struct UpdateTask {
                    title: Option<String>,
                    description: Option<String>,
                    status: Option<String>,
                }

                let url = self.url(&format!("/api/tasks/{}", task_id));
                let task: Task = match self
                    .send_json(self.client.put(&url).json(&UpdateTask {
                        title: None, // Don't update title when used for lookup
                        description,
                        status,
                    }))
                    .await
                {
                    Ok(t) => t,
                    Err(e) => return Self::error(e),
                };

                Self::success(&TaskResult {
                    action: "update".to_string(),
                    task: Some(TaskDetails {
                        id: task.id,
                        title: task.title,
                        description: task.description,
                        status: task.status.to_string(),
                        attempts_count: 0,
                        created_at: task.created_at,
                        updated_at: task.updated_at,
                    }),
                    attempt: None,
                    message: Some("Task updated".to_string()),
                    next_steps: vec![],
                })
            }
            "delete" => {
                let project_id = match &project {
                    Some(p) => Some(self.resolve_project(p).await.ok()).flatten(),
                    None => None,
                };

                let task_id = match self.resolve_task(&title, project_id).await {
                    Ok(id) => id,
                    Err(e) => return Self::error(e),
                };

                let url = self.url(&format!("/api/tasks/{}", task_id));
                match self
                    .send_json::<serde_json::Value>(self.client.delete(&url))
                    .await
                {
                    Ok(_) => {}
                    Err(e) => return Self::error(e),
                };

                Self::success(&TaskResult {
                    action: "delete".to_string(),
                    task: None,
                    attempt: None,
                    message: Some(format!("Task {} deleted", task_id)),
                    next_steps: vec!["tasks() - List remaining tasks".to_string()],
                })
            }
            _ => {
                // Default: get task details
                let project_id = match &project {
                    Some(p) => Some(self.resolve_project(p).await.ok()).flatten(),
                    None => None,
                };

                let task_id = match self.resolve_task(&title, project_id).await {
                    Ok(id) => id,
                    Err(e) => return Self::error(e),
                };

                let url = self.url(&format!("/api/tasks/{}", task_id));
                let task: Task = match self.send_json(self.client.get(&url)).await {
                    Ok(t) => t,
                    Err(e) => return Self::error(e),
                };

                Self::success(&TaskResult {
                    action: "get".to_string(),
                    task: Some(TaskDetails {
                        id: task.id,
                        title: task.title.clone(),
                        description: task.description,
                        status: task.status.to_string(),
                        attempts_count: 0, // TODO: Count attempts
                        created_at: task.created_at,
                        updated_at: task.updated_at,
                    }),
                    attempt: None,
                    message: None,
                    next_steps: vec![
                        format!("attempts(task='{}') - List attempts", task.id),
                        format!(
                            "task(title='{}', action='start') - Start new attempt",
                            task.title
                        ),
                    ],
                })
            }
        }
    }

    // =========================================================================
    // LEVEL 3: ATTEMPTS (Maximum Abstraction Level)
    // =========================================================================

    #[tool(description = "List attempts for a task. Shows running and completed attempts.")]
    async fn attempts(
        &self,
        Parameters(AttemptsRequest { task, all: _all }): Parameters<AttemptsRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let task_id = match self.resolve_task(&task, None).await {
            Ok(id) => id,
            Err(e) => return Self::error(e),
        };

        let url = self.url(&format!("/api/task-attempts?task_id={}", task_id));
        let attempts: Vec<TaskAttempt> = match self.send_json(self.client.get(&url)).await {
            Ok(a) => a,
            Err(e) => return Self::error(e),
        };

        let summaries: Vec<AttemptSummary> = attempts
            .into_iter()
            .map(|a| AttemptSummary {
                id: a.id,
                task_id: a.task_id,
                status: "running".to_string(), // TODO: Get actual status
                executor: a.executor,
                branch: a.branch,
                created_at: a.created_at,
            })
            .collect();

        let count = summaries.len();
        Self::success(&AttemptsResult {
            attempts: summaries,
            count,
            task_id,
            next_steps: vec![
                "attempt(id='<id>') - Get attempt details with last response".to_string(),
                "attempt(id='<id>', history=true) - Get full conversation history".to_string(),
            ],
        })
    }

    #[tool(
        description = "Get attempt details including last response or full history. This is how you see what the executor produced."
    )]
    async fn attempt(
        &self,
        Parameters(AttemptRequest { id, history }): Parameters<AttemptRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let attempt_id = match Uuid::parse_str(&id) {
            Ok(uuid) => uuid,
            Err(_) => return Self::error(BeltError::new("Invalid attempt ID")),
        };

        // Get attempt details
        let url = self.url(&format!("/api/task-attempts/{}", attempt_id));
        let attempt: TaskAttempt = match self.send_json(self.client.get(&url)).await {
            Ok(a) => a,
            Err(e) => return Self::error(e),
        };

        // Get processes for this attempt to extract conversation
        let processes_url = self.url(&format!(
            "/api/execution-processes?attempt_id={}",
            attempt_id
        ));

        #[derive(serde::Deserialize)]
        struct ExecutionProcess {
            #[allow(dead_code)]
            id: Uuid,
            logs: Option<serde_json::Value>,
        }

        let processes: Vec<ExecutionProcess> =
            match self.send_json(self.client.get(&processes_url)).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("Could not fetch execution processes: {}", e.error);
                    vec![] // No processes yet
                }
            };

        // Extract last response from processes
        let last_response = processes.last().and_then(|p| {
            p.logs.as_ref().and_then(|logs| {
                // Try to extract the last assistant message from logs
                if let Some(messages) = logs.get("messages").and_then(|m| m.as_array()) {
                    messages
                        .iter()
                        .rev()
                        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"))
                        .and_then(|m| m.get("content").and_then(|c| c.as_str()).map(String::from))
                } else {
                    None
                }
            })
        });

        // Build history if requested
        let history_data = if history.unwrap_or(false) {
            let mut turns = vec![];
            for process in &processes {
                if let Some(messages) = process
                    .logs
                    .as_ref()
                    .and_then(|logs| logs.get("messages"))
                    .and_then(|m| m.as_array())
                {
                    for msg in messages {
                        let role = msg
                            .get("role")
                            .and_then(|r| r.as_str())
                            .unwrap_or("unknown");
                        let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        turns.push(ConversationTurn {
                            role: role.to_string(),
                            content: content.to_string(),
                            timestamp: None,
                        });
                    }
                }
            }
            Some(turns)
        } else {
            None
        };

        // Determine status based on processes
        let status = if processes.is_empty() {
            "pending"
        } else {
            "running" // TODO: Determine actual status
        };

        Self::success(&AttemptResult {
            attempt_id: attempt.id,
            task_id: attempt.task_id,
            status: status.to_string(),
            executor: attempt.executor,
            branch: attempt.branch,
            target_branch: attempt.target_branch,
            created_at: attempt.created_at,
            updated_at: attempt.updated_at,
            last_response,
            history: history_data,
            next_steps: vec![
                format!(
                    "continue(attempt='{}', message='...') - Send follow-up",
                    attempt.id
                ),
                format!("stop(attempt='{}') - Stop the attempt", attempt.id),
                format!("branch(attempt='{}') - Check branch status", attempt.id),
            ],
        })
    }

    #[tool(
        description = "Send a follow-up message to a running attempt. Continue the conversation."
    )]
    async fn continue_attempt(
        &self,
        Parameters(ContinueRequest {
            attempt,
            message,
            variant: _variant,
        }): Parameters<ContinueRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let attempt_id = match Uuid::parse_str(&attempt) {
            Ok(uuid) => uuid,
            Err(_) => return Self::error(BeltError::new("Invalid attempt ID")),
        };

        #[derive(serde::Serialize)]
        struct FollowUp {
            attempt_id: Uuid,
            message: String,
        }

        let url = self.url(&format!("/api/task-attempts/{}/follow-up", attempt_id));
        match self
            .send_json::<serde_json::Value>(self.client.post(&url).json(&FollowUp {
                attempt_id,
                message: message.clone(),
            }))
            .await
        {
            Ok(_) => {}
            Err(e) => return Self::error(e),
        };

        Self::success(&ContinueResult {
            attempt_id,
            status: "running".to_string(),
            message: format!("Follow-up sent: {}", message),
            next_steps: vec![format!("attempt(id='{}') - Check response", attempt_id)],
        })
    }

    #[tool(description = "Stop a running attempt. Work is preserved in the branch.")]
    async fn stop(
        &self,
        Parameters(StopRequest { attempt }): Parameters<StopRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let attempt_id = match Uuid::parse_str(&attempt) {
            Ok(uuid) => uuid,
            Err(_) => return Self::error(BeltError::new("Invalid attempt ID")),
        };

        let url = self.url(&format!("/api/task-attempts/{}/stop", attempt_id));
        match self
            .send_json::<serde_json::Value>(self.client.post(&url))
            .await
        {
            Ok(_) => {}
            Err(e) => return Self::error(e),
        };

        Self::success(&StopResult {
            attempt_id,
            stopped: true,
            message: "Attempt stopped. Work is preserved in the branch.".to_string(),
            next_steps: vec![
                format!("branch(attempt='{}') - Check branch status", attempt_id),
                format!("merge(attempt='{}') - Merge changes", attempt_id),
            ],
        })
    }

    // =========================================================================
    // LEVEL 4: GIT & PR
    // =========================================================================

    #[tool(
        description = "Get branch status for an attempt. Shows commits ahead/behind, conflicts."
    )]
    async fn branch(
        &self,
        Parameters(BranchRequest {
            attempt,
            action,
            target,
        }): Parameters<BranchRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let attempt_id = match Uuid::parse_str(&attempt) {
            Ok(uuid) => uuid,
            Err(_) => return Self::error(BeltError::new("Invalid attempt ID")),
        };

        let action = action.as_deref().unwrap_or("status");

        match action {
            "change-target" => {
                let new_target = match target {
                    Some(t) => t,
                    None => {
                        return Self::error(BeltError::new(
                            "Target branch required for change-target",
                        ));
                    }
                };

                #[derive(serde::Serialize)]
                struct ChangeTarget {
                    target_branch: String,
                }

                let url = self.url(&format!(
                    "/api/task-attempts/{}/change-target-branch",
                    attempt_id
                ));
                match self
                    .send_json::<serde_json::Value>(self.client.post(&url).json(&ChangeTarget {
                        target_branch: new_target.clone(),
                    }))
                    .await
                {
                    Ok(_) => {}
                    Err(e) => return Self::error(e),
                };

                Self::success(&BranchResult {
                    attempt_id,
                    branch: "".to_string(), // Will be filled
                    target_branch: new_target,
                    ahead: 0,
                    behind: 0,
                    has_conflicts: false,
                    message: Some("Target branch changed".to_string()),
                    next_steps: vec![format!(
                        "branch(attempt='{}') - Check branch status",
                        attempt_id
                    )],
                })
            }
            _ => {
                // Get branch status
                let url = self.url(&format!("/api/task-attempts/{}/branch-status", attempt_id));

                #[derive(serde::Deserialize)]
                struct BranchStatus {
                    branch: String,
                    target_branch: String,
                    ahead: Option<usize>,
                    behind: Option<usize>,
                    has_conflicts: Option<bool>,
                }

                let status: BranchStatus = match self.send_json(self.client.get(&url)).await {
                    Ok(s) => s,
                    Err(e) => return Self::error(e),
                };

                Self::success(&BranchResult {
                    attempt_id,
                    branch: status.branch,
                    target_branch: status.target_branch,
                    ahead: status.ahead.unwrap_or(0),
                    behind: status.behind.unwrap_or(0),
                    has_conflicts: status.has_conflicts.unwrap_or(false),
                    message: None,
                    next_steps: vec![
                        format!("merge(attempt='{}') - Merge to target", attempt_id),
                        format!("push(attempt='{}') - Push to GitHub", attempt_id),
                    ],
                })
            }
        }
    }

    #[tool(description = "Merge attempt branch to target branch.")]
    async fn merge(
        &self,
        Parameters(MergeRequest { attempt }): Parameters<MergeRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let attempt_id = match Uuid::parse_str(&attempt) {
            Ok(uuid) => uuid,
            Err(_) => return Self::error(BeltError::new("Invalid attempt ID")),
        };

        let url = self.url(&format!("/api/task-attempts/{}/merge", attempt_id));
        match self
            .send_json::<serde_json::Value>(self.client.post(&url))
            .await
        {
            Ok(_) => {}
            Err(e) => return Self::error(e),
        };

        Self::success(&MergeResult {
            attempt_id,
            success: true,
            message: "Branch merged successfully".to_string(),
            next_steps: vec!["tasks() - View updated tasks".to_string()],
        })
    }

    #[tool(description = "Push attempt branch to GitHub.")]
    async fn push(
        &self,
        Parameters(PushRequest { attempt }): Parameters<PushRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let attempt_id = match Uuid::parse_str(&attempt) {
            Ok(uuid) => uuid,
            Err(_) => return Self::error(BeltError::new("Invalid attempt ID")),
        };

        let url = self.url(&format!("/api/task-attempts/{}/push", attempt_id));
        match self
            .send_json::<serde_json::Value>(self.client.post(&url))
            .await
        {
            Ok(_) => {}
            Err(e) => return Self::error(e),
        };

        // Get attempt to know the branch name
        let attempt_url = self.url(&format!("/api/task-attempts/{}", attempt_id));
        let attempt: TaskAttempt = match self.send_json(self.client.get(&attempt_url)).await {
            Ok(a) => a,
            Err(e) => return Self::error(e),
        };

        Self::success(&PushResult {
            attempt_id,
            success: true,
            branch: attempt.branch,
            message: "Branch pushed to GitHub".to_string(),
            next_steps: vec![format!(
                "pr(attempt='{}', action='create') - Create pull request",
                attempt_id
            )],
        })
    }

    #[tool(description = "Create or attach to a GitHub pull request.")]
    async fn pr(
        &self,
        Parameters(PrRequest {
            attempt,
            action,
            title,
            body,
            pr_number,
        }): Parameters<PrRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let attempt_id = match Uuid::parse_str(&attempt) {
            Ok(uuid) => uuid,
            Err(_) => return Self::error(BeltError::new("Invalid attempt ID")),
        };

        let action = action.as_deref().unwrap_or("create");

        match action {
            "attach" => {
                let pr_num = match pr_number {
                    Some(n) => n,
                    None => return Self::error(BeltError::new("PR number required for attach")),
                };

                #[derive(serde::Serialize)]
                struct AttachPr {
                    pr_number: i64,
                }

                let url = self.url(&format!("/api/task-attempts/{}/attach-pr", attempt_id));
                match self
                    .send_json::<serde_json::Value>(
                        self.client.post(&url).json(&AttachPr { pr_number: pr_num }),
                    )
                    .await
                {
                    Ok(_) => {}
                    Err(e) => return Self::error(e),
                };

                Self::success(&PrResult {
                    attempt_id,
                    action: "attach".to_string(),
                    pr_number: Some(pr_num),
                    pr_url: None,
                    message: format!("PR #{} attached to attempt", pr_num),
                    next_steps: vec![],
                })
            }
            _ => {
                // Create PR
                #[derive(serde::Serialize)]
                struct CreatePr {
                    title: Option<String>,
                    body: Option<String>,
                }

                #[derive(serde::Deserialize)]
                struct PrResponse {
                    pr_number: Option<i64>,
                    pr_url: Option<String>,
                }

                let url = self.url(&format!("/api/task-attempts/{}/create-pr", attempt_id));
                let response: PrResponse = match self
                    .send_json(self.client.post(&url).json(&CreatePr { title, body }))
                    .await
                {
                    Ok(r) => r,
                    Err(e) => return Self::error(e),
                };

                Self::success(&PrResult {
                    attempt_id,
                    action: "create".to_string(),
                    pr_number: response.pr_number,
                    pr_url: response.pr_url.clone(),
                    message: format!("PR created: {}", response.pr_url.unwrap_or_default()),
                    next_steps: vec![],
                })
            }
        }
    }
}

// =============================================================================
// SERVER HANDLER IMPLEMENTATION
// =============================================================================

#[tool_handler]
impl ServerHandler for BeltServer {
    async fn initialize(
        &self,
        request: InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<ServerInfo, ErrorData> {
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request.clone());
        }

        let requested_version = request.protocol_version.clone();
        let negotiated_version = match Self::negotiate_protocol_version(&requested_version) {
            Ok(version) => version,
            Err(error) => return Err(error),
        };

        self.set_negotiated_protocol_version(negotiated_version.clone());

        Ok(self.server_info_for_version(negotiated_version))
    }

    fn get_info(&self) -> ServerInfo {
        let protocol_version = self.current_protocol_version();
        self.server_info_for_version(protocol_version)
    }
}
