//! StatusEngine - Manages TUI footer status display with timing, git info, and external providers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use serde_json;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;
use ratatui::style::Stylize;

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
    GitCounts,
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
}

impl StatusEngine {
    /// Create a new StatusEngine with the given configuration
    pub fn new(config: StatusEngineConfig) -> Self {
        // Default order from the requirement
        let default_items = vec![
            StatusItem::Model,
            StatusItem::Effort,
            StatusItem::WorkspaceName,
            StatusItem::GitBranch,
            StatusItem::GitCounts,
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

    /// Tick the engine and produce status output
    /// Respects the 300ms throttle for external provider calls
    pub async fn tick(&mut self, now: Instant) -> StatusEngineOutput {
        let line2 = self.build_line2();
        let line3 = self.maybe_run_command_provider(now).await;

        StatusEngineOutput { line2, line3 }
    }

    /// Build Line 2 from selected status items
    fn build_line2(&self) -> String {
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
                        let mut git_part = branch.clone();
                        // Add counts if available
                        if let Some(ref counts) = self.state.git_counts {
                            git_part.push_str(&format!(" {}", counts));
                        }
                        parts.push(git_part);
                    }
                }
                StatusItem::GitCounts => {
                    // This is handled in GitBranch to avoid duplication
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
            parts.join(" | ").dim().to_string()
        }
    }

    /// Check if we should run the command provider and execute if so
    async fn maybe_run_command_provider(&mut self, now: Instant) -> Option<String> {
        // Only run if provider is "command" and command is configured
        if self.config.provider != "command" || self.config.command.is_none() {
            return None;
        }

        // Check throttling
        if let Some(last_run) = self.last_command_run {
            if now.duration_since(last_run) < self.command_cooldown {
                return self.last_line3.clone();
            }
        }

        // Run the command
        match self.run_command_provider().await {
            Ok(Some(output)) => {
                self.last_command_run = Some(now);
                self.last_line3 = Some(output.clone());
                Some(output)
            }
            Ok(None) => {
                self.last_command_run = Some(now);
                // Keep last good output on empty result
                self.last_line3.clone()
            }
            Err(_) => {
                self.last_command_run = Some(now);
                // Keep last good output on error
                self.last_line3.clone()
            }
        }
    }

    /// Execute the configured command provider
    async fn run_command_provider(&self) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        let command_path = match &self.config.command {
            Some(cmd) => cmd,
            None => return Ok(None),
        };

        // Build JSON payload
        let payload = self.build_command_payload()?;
        let payload_json = serde_json::to_string(&payload)?;

        // Spawn the command with timeout
        let timeout_duration = Duration::from_millis(self.config.command_timeout_ms);
        
        let result = timeout(timeout_duration, async {
            let mut child = Command::new(command_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;

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
                    Ok(None)
                } else {
                    Ok(Some(first_line))
                }
            } else {
                Ok(None)
            }
        }).await;

        match result {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Ok(None), // Timeout - return None to keep last good output
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
            payload.insert("effort".to_string(), serde_json::Value::String(effort.clone()));
        }

        if let Some(ref workspace_name) = self.state.workspace_name {
            let mut workspace_obj = serde_json::Map::new();
            workspace_obj.insert("name".to_string(), serde_json::Value::String(workspace_name.clone()));
            if let Some(ref cwd) = self.state.cwd {
                workspace_obj.insert("current_dir".to_string(), serde_json::Value::String(cwd.display().to_string()));
                workspace_obj.insert("project_dir".to_string(), serde_json::Value::String(cwd.display().to_string()));
            }
            payload.insert("workspace".to_string(), serde_json::Value::Object(workspace_obj));
        }

        if let Some(ref cwd) = self.state.cwd {
            payload.insert("cwd".to_string(), serde_json::Value::String(cwd.display().to_string()));
        }

        // Git info
        if self.state.git_branch.is_some() || self.state.git_counts.is_some() {
            let mut git_obj = serde_json::Map::new();
            if let Some(ref branch) = self.state.git_branch {
                git_obj.insert("branch".to_string(), serde_json::Value::String(branch.clone()));
            }
            if let Some(ref counts) = self.state.git_counts {
                git_obj.insert("counts".to_string(), serde_json::Value::String(counts.clone()));
            }
            payload.insert("git".to_string(), serde_json::Value::Object(git_obj));
        }

        if let Some(ref sandbox) = self.state.sandbox {
            payload.insert("sandbox".to_string(), serde_json::Value::String(sandbox.clone()));
        }

        if let Some(ref approval) = self.state.approval {
            payload.insert("approval".to_string(), serde_json::Value::String(approval.clone()));
        }

        // Timing info
        if let Some(since_session_ms) = self.state.since_session_ms {
            let mut timing_obj = serde_json::Map::new();
            timing_obj.insert("since_session_ms".to_string(), serde_json::Value::Number(since_session_ms.into()));
            payload.insert("timing".to_string(), serde_json::Value::Object(timing_obj));
        }

        Ok(serde_json::Value::Object(payload))
    }

    /// Apply center-ellipsis truncation to a string for the given width
    fn truncate_with_ellipsis(text: &str, max_width: usize) -> String {
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
        let right_part: String = chars.iter().rev().take(right_len).collect::<String>().chars().rev().collect();

        format!("{}{}{}", left_part, ellipsis, right_part)
    }

    /// Get width-aware Line 2 with truncation applied
    pub fn get_line2_with_width(&self, max_width: usize) -> String {
        let line2 = self.build_line2();
        if line2.len() <= max_width {
            line2
        } else {
            // Apply ellipsis to git branch if it's too long
            // This is a simplified version - in practice you'd want more sophisticated logic
            Self::truncate_with_ellipsis(&line2, max_width)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statusengine_creation() {
        let config = StatusEngineConfig::default();
        let engine = StatusEngine::new(config);
        assert_eq!(engine.line2_items.len(), 7);
        assert_eq!(engine.line2_items[0], StatusItem::Model);
    }

    #[test]
    fn test_line2_building() {
        let config = StatusEngineConfig::default();
        let mut engine = StatusEngine::new(config);
        
        let mut state = StatusEngineState::default();
        state.model = Some("gpt-4o-mini".to_string());
        state.effort = Some("medium".to_string());
        state.workspace_name = Some("codex".to_string());
        
        engine.set_state(state);
        let line2 = engine.build_line2();
        
        assert!(line2.contains("gpt-4o-mini"));
        assert!(line2.contains("medium"));
        assert!(line2.contains("codex"));
    }

    #[test]
    fn test_truncate_with_ellipsis() {
        assert_eq!(StatusEngine::truncate_with_ellipsis("short", 10), "short");
        assert_eq!(StatusEngine::truncate_with_ellipsis("verylongbranchname", 10), "very…name");
        assert_eq!(StatusEngine::truncate_with_ellipsis("abc", 2), "ab");
    }

    #[tokio::test]
    async fn test_command_throttling() {
        let config = StatusEngineConfig {
            enabled: true,
            provider: "command".to_string(),
            command: Some("/bin/echo".to_string()),
            command_timeout_ms: 100,
        };
        
        let mut engine = StatusEngine::new(config);
        let now = Instant::now();
        
        // First call should work
        let output1 = engine.maybe_run_command_provider(now).await;
        
        // Immediate second call should return cached result (throttled)
        let output2 = engine.maybe_run_command_provider(now).await;
        
        // Should get same result due to throttling
        assert_eq!(output1, output2);
    }
}