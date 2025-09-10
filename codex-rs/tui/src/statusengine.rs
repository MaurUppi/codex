//! StatusEngine - Manages TUI footer status display with timing, git info, and external providers.

use ratatui::style::Stylize;
use serde_json;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;
use tracing;

// Import git helpers for branch and diff count information
use codex_core::git_info::{collect_git_info, working_diff_counts};

/// Configuration for the StatusEngine
#[derive(Debug, Clone)]
pub struct StatusEngineConfig {
    /// Timeout for external command provider in milliseconds
    pub command_timeout_ms: u64,
    /// Optional path to external command provider
    pub command: Option<String>,
    /// Whether StatusEngine is enabled
    pub enabled: bool,
    /// Provider type: "command" or "builtin"
    pub provider: String,
}

impl Default for StatusEngineConfig {
    fn default() -> Self {
        Self {
            command_timeout_ms: 300,
            command: None,
            enabled: false,
            provider: "builtin".to_string(),
        }
    }
}

/// Available status items for Line 2
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusItem {
    Model,
    Effort,
    WorkspaceName,
    GitBranch,
    Sandbox,
    Approval,
}

/// Current state of the session for status display
#[derive(Debug, Clone, Default)]
pub struct StatusEngineState {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub workspace_name: Option<String>,
    pub git_branch: Option<String>,
    pub git_counts: Option<String>,
    pub sandbox: Option<String>,
    pub approval: Option<String>,
    pub since_session_ms: Option<u64>,
    pub cwd: Option<PathBuf>,
}

/// Output from the StatusEngine
#[derive(Debug, Clone, Default)]
pub struct StatusEngineOutput {
    pub line2: String,
    pub line3: Option<String>,
}

/// Main StatusEngine implementation
pub struct StatusEngine {
    config: StatusEngineConfig,
    state: StatusEngineState,
    line2_items: Vec<StatusItem>,
    last_command_run: Option<Instant>,
    last_line3: Option<String>,
    command_cooldown: Duration,
    consecutive_failures: u32,
    backoff_until: Option<Instant>,
}

impl StatusEngine {
    /// Create a new StatusEngine with the given configuration
    pub fn new(mut config: StatusEngineConfig) -> Self {
        // Validate and clamp configuration values

        // Clamp command timeout to reasonable range (150-500ms as per assessment)
        config.command_timeout_ms = config.command_timeout_ms.clamp(150, 500);

        // Validate provider type with fallback to "builtin"
        if config.provider != "command" && config.provider != "builtin" {
            tracing::debug!(
                "StatusEngine: invalid provider '{}', falling back to 'builtin'",
                config.provider
            );
            config.provider = "builtin".to_string();
        }

        // Default order from the requirement
        let default_items = vec![
            StatusItem::Model,
            StatusItem::Effort,
            StatusItem::WorkspaceName,
            StatusItem::GitBranch,
            StatusItem::Sandbox,
            StatusItem::Approval,
        ];

        Self {
            config,
            state: StatusEngineState::default(),
            line2_items: default_items,
            last_command_run: None,
            last_line3: None,
            command_cooldown: Duration::from_millis(300), // Built-in 300ms throttle
            consecutive_failures: 0,
            backoff_until: None,
        }
    }

    /// Update the engine state with new session information
    pub fn set_state(&mut self, state: StatusEngineState) {
        self.state = state;
    }

    /// Set the items and order for Line 2 display
    pub fn set_line2_selection(&mut self, items: &[StatusItem]) {
        self.line2_items = items.to_vec();
    }

    /// Get the current Line 2 items (for testing)
    pub fn line2_items(&self) -> &[StatusItem] {
        &self.line2_items
    }

    /// Apply consistent styling to status line text
    fn style_status_line(text: String) -> String {
        text.dim().to_string()
    }

    /// Tick the engine and produce status output
    /// Respects the 300ms throttle for external provider calls
    pub async fn tick(&mut self, now: Instant) -> StatusEngineOutput {
        // Update git information before building Line 2
        self.update_git_info().await;
        
        let line2 = self.build_line2();
        let line3 = self.maybe_run_command_provider(now).await;

        StatusEngineOutput { line2, line3 }
    }

    /// Update git branch and diff counts information
    /// This is called on each tick to refresh git status
    async fn update_git_info(&mut self) {
        if let Some(ref cwd) = self.state.cwd {
            // Get current git branch from git info
            if let Some(git_info) = collect_git_info(cwd).await {
                self.state.git_branch = git_info.branch;
            }
            
            // Get diff counts (+added, -removed) against HEAD
            if let Some((added, removed)) = working_diff_counts(cwd).await {
                self.state.git_counts = Some(format!("+{} -{}", added, removed));
            }
        }
    }

    /// Build Line 2 from selected status items
    /// Made public for testing purposes
    pub fn build_line2(&self) -> String {
        let mut parts = Vec::new();

        for item in &self.line2_items {
            match item {
                StatusItem::Model => {
                    if let Some(ref model) = self.state.model {
                        parts.push(model.clone());
                    }
                }
                StatusItem::Effort => {
                    if let Some(ref effort) = self.state.effort {
                        parts.push(effort.clone());
                    }
                }
                StatusItem::WorkspaceName => {
                    if let Some(ref name) = self.state.workspace_name {
                        parts.push(name.clone());
                    }
                }
                StatusItem::GitBranch => {
                    if let Some(ref branch) = self.state.git_branch {
                        let git_part = if let Some(ref counts) = self.state.git_counts {
                            format!("{branch} {counts}")
                        } else {
                            branch.clone()
                        };
                        parts.push(git_part);
                    }
                }
                StatusItem::Sandbox => {
                    if let Some(ref sandbox) = self.state.sandbox {
                        parts.push(sandbox.clone());
                    }
                }
                StatusItem::Approval => {
                    if let Some(ref approval) = self.state.approval {
                        parts.push(approval.clone());
                    }
                }
            }
        }

        // Join with " | " separator and apply consistent styling
        if parts.is_empty() {
            String::new()
        } else {
            Self::style_status_line(parts.join(" | "))
        }
    }

    /// Check if we should run the command provider and execute if so
    /// Made public for testing purposes
    pub async fn maybe_run_command_provider(&mut self, now: Instant) -> Option<String> {
        // Only run if provider is "command" and command is configured
        if self.config.provider != "command" || self.config.command.is_none() {
            return None;
        }

        // Check if we're in backoff period
        if let Some(backoff_until) = self.backoff_until {
            if now < backoff_until {
                tracing::debug!(
                    "StatusEngine command provider in backoff period, using cached result"
                );
                return self.last_line3.clone();
            } else {
                // Backoff period expired, clear it
                tracing::debug!(
                    "StatusEngine backoff period expired, resuming command provider calls"
                );
                self.backoff_until = None;
            }
        }

        // Check throttling with jitter
        let jitter_ms = (now.elapsed().as_nanos() % 100) as u64; // Simple jitter 0-99ms
        let effective_cooldown = self.command_cooldown + Duration::from_millis(jitter_ms);

        if let Some(last_run) = self.last_command_run
            && now.duration_since(last_run) < effective_cooldown
        {
            tracing::debug!("StatusEngine command provider throttled, using cached result");
            return self.last_line3.clone();
        }

        // Run the command
        match self.run_command_provider().await {
            Ok(Some(output)) => {
                tracing::debug!("StatusEngine command provider succeeded, got output");
                self.last_command_run = Some(now);
                self.last_line3 = Some(output.clone());
                self.consecutive_failures = 0; // Reset failure count on success
                Some(output)
            }
            Ok(None) => {
                tracing::debug!("StatusEngine command provider returned empty result");
                self.last_command_run = Some(now);
                self.consecutive_failures = 0; // Empty result is not a failure
                // Keep last good output on empty result
                self.last_line3.clone()
            }
            Err(e) => {
                tracing::debug!("StatusEngine command provider error: {}", e);
                self.last_command_run = Some(now);
                self.consecutive_failures += 1;

                // Apply exponential backoff after failures
                if self.consecutive_failures >= 3 {
                    let backoff_ms =
                        std::cmp::min(5000, 1000 * (1 << (self.consecutive_failures - 3)));
                    self.backoff_until = Some(now + Duration::from_millis(backoff_ms));
                    tracing::debug!(
                        "StatusEngine entering backoff mode for {}ms after {} consecutive failures",
                        backoff_ms,
                        self.consecutive_failures
                    );
                }

                // Keep last good output on error
                self.last_line3.clone()
            }
        }
    }

    /// Execute the configured command provider
    async fn run_command_provider(
        &self,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        let command_path = match &self.config.command {
            Some(cmd) => cmd,
            None => return Ok(None),
        };

        // Build JSON payload
        let payload = self.build_command_payload()?;
        let payload_json = serde_json::to_string(&payload)?;

        // Spawn the command with timeout and proper cleanup
        let timeout_duration = Duration::from_millis(self.config.command_timeout_ms);
        tracing::debug!(
            "StatusEngine spawning command provider with timeout {}ms",
            self.config.command_timeout_ms
        );

        let mut child = Command::new(command_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true) // Ensure child is killed if dropped
            .env_clear() // Clear environment for security
            .env("PATH", std::env::var("PATH").unwrap_or_default()) // Keep minimal PATH
            .spawn()?;

        let result = timeout(timeout_duration, async move {
            // Write payload to stdin
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(payload_json.as_bytes()).await?;
                stdin.shutdown().await?;
            }

            // Wait for completion and get output
            let output = child.wait_with_output().await?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Get first line only
                let first_line = stdout.lines().next().unwrap_or("").trim().to_string();
                if first_line.is_empty() {
                    Ok::<Option<String>, Box<dyn std::error::Error + Send + Sync>>(None)
                } else {
                    Ok(Some(first_line))
                }
            } else {
                Ok(None)
            }
        })
        .await;

        match result {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                // Timeout occurred - child process should be killed by kill_on_drop(true)
                tracing::debug!(
                    "StatusEngine command provider timed out, child will be killed on drop"
                );
                Ok(None) // Return None to keep last good output
            }
        }
    }

    /// Build JSON payload for command provider
    fn build_command_payload(&self) -> Result<serde_json::Value, serde_json::Error> {
        let mut payload = serde_json::Map::new();

        // Add available fields
        if let Some(ref model) = self.state.model {
            let mut model_obj = serde_json::Map::new();
            model_obj.insert("id".to_string(), serde_json::Value::String(model.clone()));
            payload.insert("model".to_string(), serde_json::Value::Object(model_obj));
        }

        if let Some(ref effort) = self.state.effort {
            payload.insert(
                "effort".to_string(),
                serde_json::Value::String(effort.clone()),
            );
        }

        if let Some(ref workspace_name) = self.state.workspace_name {
            let mut workspace_obj = serde_json::Map::new();
            workspace_obj.insert(
                "name".to_string(),
                serde_json::Value::String(workspace_name.clone()),
            );
            if let Some(ref cwd) = self.state.cwd {
                workspace_obj.insert(
                    "current_dir".to_string(),
                    serde_json::Value::String(cwd.display().to_string()),
                );
                // Only add project_dir if it differs from current_dir
                // In the current implementation they're the same, so we omit project_dir
                // to avoid duplication as recommended in the assessment
            }
            payload.insert(
                "workspace".to_string(),
                serde_json::Value::Object(workspace_obj),
            );
        }

        if let Some(ref cwd) = self.state.cwd {
            payload.insert(
                "cwd".to_string(),
                serde_json::Value::String(cwd.display().to_string()),
            );
        }

        // Git info
        if self.state.git_branch.is_some() || self.state.git_counts.is_some() {
            let mut git_obj = serde_json::Map::new();
            if let Some(ref branch) = self.state.git_branch {
                git_obj.insert(
                    "branch".to_string(),
                    serde_json::Value::String(branch.clone()),
                );
            }
            if let Some(ref counts) = self.state.git_counts {
                git_obj.insert(
                    "counts".to_string(),
                    serde_json::Value::String(counts.clone()),
                );
            }
            payload.insert("git".to_string(), serde_json::Value::Object(git_obj));
        }

        if let Some(ref sandbox) = self.state.sandbox {
            payload.insert(
                "sandbox".to_string(),
                serde_json::Value::String(sandbox.clone()),
            );
        }

        if let Some(ref approval) = self.state.approval {
            payload.insert(
                "approval".to_string(),
                serde_json::Value::String(approval.clone()),
            );
        }

        // Timing info
        if let Some(since_session_ms) = self.state.since_session_ms {
            let mut timing_obj = serde_json::Map::new();
            timing_obj.insert(
                "since_session_ms".to_string(),
                serde_json::Value::Number(since_session_ms.into()),
            );
            payload.insert("timing".to_string(), serde_json::Value::Object(timing_obj));
        }

        Ok(serde_json::Value::Object(payload))
    }

    /// Apply center-ellipsis truncation to a string for the given width
    /// Made public for testing purposes
    pub fn truncate_with_ellipsis(text: &str, max_width: usize) -> String {
        if text.len() <= max_width {
            return text.to_string();
        }

        if max_width < 3 {
            return "...".chars().take(max_width).collect();
        }

        let ellipsis = "…";
        let available = max_width - ellipsis.len();
        let left_len = available / 2;
        let right_len = available - left_len;

        let chars: Vec<char> = text.chars().collect();
        let left_part: String = chars.iter().take(left_len).collect();
        let right_part: String = chars
            .iter()
            .rev()
            .take(right_len)
            .collect::<String>()
            .chars()
            .rev()
            .collect();

        format!("{left_part}{ellipsis}{right_part}")
    }

    /// Get width-aware Line 2 with truncation applied specifically to branch token
    pub fn get_line2_with_width(&self, max_width: usize) -> String {
        let mut parts = Vec::new();

        for item in &self.line2_items {
            match item {
                StatusItem::Model => {
                    if let Some(ref model) = self.state.model {
                        parts.push(model.clone());
                    }
                }
                StatusItem::Effort => {
                    if let Some(ref effort) = self.state.effort {
                        parts.push(effort.clone());
                    }
                }
                StatusItem::WorkspaceName => {
                    if let Some(ref name) = self.state.workspace_name {
                        parts.push(name.clone());
                    }
                }
                StatusItem::GitBranch => {
                    if let Some(ref branch) = self.state.git_branch {
                        let mut git_part = if let Some(ref counts) = self.state.git_counts {
                            format!("{branch} {counts}")
                        } else {
                            branch.clone()
                        };

                        // Calculate available width for branch token
                        let separator_len = if parts.is_empty() { 0 } else { " | ".len() };
                        let other_parts_len: usize = parts.iter().map(|p| p.len()).sum::<usize>()
                            + (parts.len().saturating_sub(1)) * " | ".len(); // separators between existing parts
                        let remaining_parts_estimate =
                            (self.line2_items.len() - parts.len() - 1) * 15; // estimate for remaining
                        let available_for_branch = max_width.saturating_sub(
                            other_parts_len + separator_len + remaining_parts_estimate,
                        );

                        if git_part.len() > available_for_branch && available_for_branch > 3 {
                            git_part =
                                Self::truncate_with_ellipsis(&git_part, available_for_branch);
                        }
                        parts.push(git_part);
                    }
                }
                StatusItem::Sandbox => {
                    if let Some(ref sandbox) = self.state.sandbox {
                        parts.push(sandbox.clone());
                    }
                }
                StatusItem::Approval => {
                    if let Some(ref approval) = self.state.approval {
                        parts.push(approval.clone());
                    }
                }
            }
        }

        // Join with " | " separator and apply dim styling
        if parts.is_empty() {
            String::new()
        } else {
            let result = parts.join(" | ");
            // If still too long after branch truncation, truncate the whole line as fallback
            if result.len() > max_width {
                Self::style_status_line(Self::truncate_with_ellipsis(&result, max_width))
            } else {
                Self::style_status_line(result)
            }
        }
    }
}
