use std::path::Path;

use chrono::{DateTime, Utc};
use git2::BranchType;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

use super::git::{GitService, GitServiceError};
use super::git_cli::GitCli;

#[derive(Debug, Error)]
pub enum GitRemoteError {
    #[error(transparent)]
    GitService(#[from] GitServiceError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct GitRemoteService {
    git_service: GitService,
}

impl GitRemoteService {
    pub fn new() -> Self {
        Self {
            git_service: GitService::new(),
        }
    }

    /// Fetch all tracked branches from origin
    pub fn fetch_project(
        &self,
        repo_path: &Path,
        github_token: &str,
    ) -> Result<FetchResult, GitRemoteError> {
        let start = std::time::Instant::now();

        // Get tracked branches
        let tracked_branches = self.git_service.get_tracked_branches(repo_path)?;

        tracing::debug!(
            "Fetching {} tracked branches for {:?}",
            tracked_branches.len(),
            repo_path
        );

        // Fetch using smart incremental approach (only tracked branches)
        let git_cli = GitCli::new();
        let remote_url = self.get_remote_url(repo_path)?;

        for branch in &tracked_branches {
            let refspec = format!("+refs/heads/{branch}:refs/remotes/origin/{branch}");

            tracing::debug!("Fetching branch: {}", branch);

            git_cli.fetch_with_token_and_refspec(repo_path, &remote_url, &refspec, github_token)?;
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        tracing::info!(
            "Fetched {} branches in {}ms",
            tracked_branches.len(),
            duration_ms
        );

        Ok(FetchResult {
            branches_fetched: tracked_branches.len(),
            duration_ms,
        })
    }

    /// Get sync status for all branches (always fresh, no cache)
    pub fn get_sync_status(&self, repo_path: &Path) -> Result<ProjectSyncStatus, GitRemoteError> {
        let start = std::time::Instant::now();

        let repo = self.git_service.open_repo(repo_path)?;

        // Get current branch
        let current_branch = self.git_service.get_current_branch_name(repo_path)?;

        tracing::debug!("Getting sync status for repo at {:?}", repo_path);

        // Get all branches with sync status
        let mut branches = Vec::new();

        for branch_result in repo.branches(Some(BranchType::Local))? {
            let (branch, _) = branch_result?;

            // Only process branches with upstream
            if let Ok(upstream) = branch.upstream() {
                let branch_name = branch
                    .name()?
                    .ok_or_else(|| {
                        GitServiceError::InvalidRepository("Branch has invalid name".into())
                    })?
                    .to_string();

                let local_oid = branch.get().target().ok_or_else(|| {
                    GitServiceError::InvalidRepository("Branch has no target".into())
                })?;

                let remote_oid = upstream.get().target().ok_or_else(|| {
                    GitServiceError::InvalidRepository("Upstream has no target".into())
                })?;

                // Calculate ahead/behind
                let graph_start = std::time::Instant::now();
                let (ahead, behind) = repo.graph_ahead_behind(local_oid, remote_oid)?;
                let graph_duration = graph_start.elapsed().as_micros();

                tracing::debug!(
                    "Branch {}: ahead={}, behind={} (graph query: {}µs)",
                    branch_name,
                    ahead,
                    behind,
                    graph_duration
                );

                branches.push(BranchSyncStatus {
                    branch_name,
                    local_sha: local_oid.to_string(),
                    remote_sha: Some(remote_oid.to_string()),
                    ahead_count: ahead,
                    behind_count: behind,
                    is_diverged: ahead > 0 && behind > 0,
                    is_up_to_date: ahead == 0 && behind == 0,
                    needs_pull: behind > 0,
                    needs_push: ahead > 0 && behind == 0,
                });
            }
        }

        let total_duration = start.elapsed().as_millis();

        tracing::info!(
            "Sync status for {} branches completed in {}ms",
            branches.len(),
            total_duration
        );

        Ok(ProjectSyncStatus {
            current_branch,
            branches,
            last_fetch_at: None, // No cache, so no last fetch time
        })
    }

    /// Pull branch with conflict detection
    pub fn pull_branch(
        &self,
        repo_path: &Path,
        branch_name: &str,
        github_token: &str,
        strategy: PullStrategy,
    ) -> Result<PullResult, GitRemoteError> {
        tracing::info!("Pulling branch {} with strategy {:?}", branch_name, strategy);

        let repo = self.git_service.open_repo(repo_path)?;

        // Safety check: working tree must be clean
        self.git_service.check_worktree_clean(&repo)?;

        // Get current branch (must match requested branch)
        let current = self.git_service.get_current_branch_name(repo_path)?;
        if current != branch_name {
            return Err(GitServiceError::InvalidRepository(format!(
                "Cannot pull {branch_name}: currently on {current}"
            ))
            .into());
        }

        // Fetch first
        self.fetch_branch(repo_path, branch_name, github_token)?;

        // Get ahead/behind after fetch
        let branch = GitService::find_branch(&repo, branch_name)?;
        let upstream = branch.upstream().map_err(|_| {
            GitServiceError::BranchNotFound(format!("{branch_name} has no upstream"))
        })?;

        let local_oid = branch.get().target().ok_or_else(|| {
            GitServiceError::InvalidRepository("Branch has no target".into())
        })?;

        let remote_oid = upstream.get().target().ok_or_else(|| {
            GitServiceError::InvalidRepository("Upstream has no target".into())
        })?;

        let (ahead, behind) = repo.graph_ahead_behind(local_oid, remote_oid)?;

        tracing::debug!("Pull status: ahead={}, behind={}", ahead, behind);

        // Check if pull is needed
        if behind == 0 {
            tracing::info!("Branch {} is already up-to-date", branch_name);
            return Ok(PullResult {
                success: true,
                strategy_used: strategy,
                commits_pulled: 0,
                message: "Already up-to-date".to_string(),
            });
        }

        // Check for divergence
        if ahead > 0 {
            return Err(GitServiceError::BranchesDiverged(format!(
                "Branch '{branch_name}' has diverged: {ahead} ahead, {behind} behind. Manual merge required."
            ))
            .into());
        }

        // Perform pull (fast-forward possible since ahead = 0)
        let git_cli = GitCli::new();

        let pull_start = std::time::Instant::now();

        match strategy {
            PullStrategy::Merge | PullStrategy::FastForward => {
                git_cli.run_command(repo_path, &["merge", "--ff-only", "HEAD@{u}"])?;
            }
            PullStrategy::Rebase => {
                git_cli.run_command(repo_path, &["rebase", "HEAD@{u}"])?;
            }
        }

        let pull_duration = pull_start.elapsed().as_millis();

        tracing::info!(
            "Successfully pulled {} commits in {}ms",
            behind,
            pull_duration
        );

        Ok(PullResult {
            success: true,
            strategy_used: strategy,
            commits_pulled: behind,
            message: format!("Successfully pulled {behind} commits"),
        })
    }

    // Helper methods

    fn get_remote_url(&self, repo_path: &Path) -> Result<String, GitRemoteError> {
        let repo = self.git_service.open_repo(repo_path)?;
        let remote = repo.find_remote("origin")?;
        let url = remote
            .url()
            .ok_or_else(|| GitServiceError::InvalidRepository("Remote has no URL".into()))?;
        Ok(self.git_service.convert_to_https_url(url))
    }

    fn fetch_branch(
        &self,
        repo_path: &Path,
        branch_name: &str,
        github_token: &str,
    ) -> Result<(), GitRemoteError> {
        let git_cli = GitCli::new();
        let refspec = format!("+refs/heads/{branch_name}:refs/remotes/origin/{branch_name}");

        git_cli.fetch_with_token_and_refspec(
            repo_path,
            &self.get_remote_url(repo_path)?,
            &refspec,
            github_token,
        )?;

        Ok(())
    }
}

impl Default for GitRemoteService {
    fn default() -> Self {
        Self::new()
    }
}

// Types

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct FetchResult {
    pub branches_fetched: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ProjectSyncStatus {
    pub current_branch: String,
    pub branches: Vec<BranchSyncStatus>,
    pub last_fetch_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct BranchSyncStatus {
    pub branch_name: String,
    pub local_sha: String,
    pub remote_sha: Option<String>,
    pub ahead_count: usize,
    pub behind_count: usize,
    pub is_diverged: bool,
    pub is_up_to_date: bool,
    pub needs_pull: bool,
    pub needs_push: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PullStrategy {
    Merge,
    Rebase,
    FastForward,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PullResult {
    pub success: bool,
    pub strategy_used: PullStrategy,
    pub commits_pulled: usize,
    pub message: String,
}
