//! Response types for the Belt tools.
//!
//! These are simplified, LLM-friendly response types that provide clear context
//! and suggest next actions.

use chrono::{DateTime, Utc};
use rmcp::schemars;
use serde::Serialize;
use uuid::Uuid;

// =============================================================================
// LEVEL 0: FORGE (Global Configuration)
// =============================================================================

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ForgeResult {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executors: Option<Vec<ExecutorInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<McpServerInfo>>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ExecutorInfo {
    pub name: String,
    pub description: String,
    pub variants: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct McpServerInfo {
    pub executor: String,
    pub servers: Vec<String>,
}

// =============================================================================
// LEVEL 1: PROJECTS
// =============================================================================

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ProjectsResult {
    pub projects: Vec<ProjectSummary>,
    pub count: usize,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ProjectSummary {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub active_tasks: usize,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ProjectResult {
    pub action: String,
    pub project: Option<ProjectDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branches: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ProjectDetails {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub setup_script: Option<String>,
    pub cleanup_script: Option<String>,
    pub dev_script: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// LEVEL 2: TASKS
// =============================================================================

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TasksResult {
    pub tasks: Vec<TaskSummary>,
    pub count: usize,
    pub project_id: Uuid,
    pub filters: TaskFilters,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskFilters {
    pub status: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskSummary {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub has_active_attempt: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskResult {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<AttemptSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskDetails {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub attempts_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// LEVEL 3: ATTEMPTS (Maximum Abstraction Level)
// =============================================================================

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AttemptsResult {
    pub attempts: Vec<AttemptSummary>,
    pub count: usize,
    pub task_id: Uuid,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AttemptSummary {
    pub id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub executor: String,
    pub branch: String,
    pub created_at: DateTime<Utc>,
}

/// The main attempt result with last_response and optional history.
/// This abstracts away the Process level - users don't need to know about processes.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AttemptResult {
    pub attempt_id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub executor: String,
    pub branch: String,
    pub target_branch: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    /// The last response from the executor. This is extracted from the most recent
    /// process's conversation history. ALWAYS included.
    #[schemars(description = "Last assistant response from the executor")]
    pub last_response: Option<String>,

    /// Full conversation history. Only included if history=true was passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Full conversation history (only if history=true)")]
    pub history: Option<Vec<ConversationTurn>>,

    /// Suggested next actions based on attempt status.
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ConversationTurn {
    pub role: String,
    pub content: String,
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ContinueResult {
    pub attempt_id: Uuid,
    pub status: String,
    pub message: String,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct StopResult {
    pub attempt_id: Uuid,
    pub stopped: bool,
    pub message: String,
    pub next_steps: Vec<String>,
}

// =============================================================================
// LEVEL 4: GIT & PR
// =============================================================================

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BranchResult {
    pub attempt_id: Uuid,
    pub branch: String,
    pub target_branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub has_conflicts: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MergeResult {
    pub attempt_id: Uuid,
    pub success: bool,
    pub message: String,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PushResult {
    pub attempt_id: Uuid,
    pub success: bool,
    pub branch: String,
    pub message: String,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PrResult {
    pub attempt_id: Uuid,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    pub message: String,
    pub next_steps: Vec<String>,
}

// =============================================================================
// ERROR TYPE
// =============================================================================

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BeltError {
    pub success: bool,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    pub suggestions: Vec<String>,
}

impl BeltError {
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            success: false,
            error: error.into(),
            details: None,
            suggestions: vec![],
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = suggestions;
        self
    }
}
