//! StatusEngine TUI snapshot and integration tests
//!
//! This module tests footer rendering with StatusEngine output at different widths,
//! ensuring the layout behaves correctly and text truncation works as expected.

use codex_tui::app_event_sender::AppEventSender;
use codex_tui::bottom_pane::chat_composer::ChatComposer;
use codex_tui::statusengine::StatusEngine;
use codex_tui::statusengine::StatusEngineConfig;
use codex_tui::statusengine::StatusEngineOutput;
use codex_tui::statusengine::StatusEngineState;
use codex_tui::statusengine::StatusItem;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;
use tokio::sync::mpsc;

/// Test helper to create a ChatComposer with StatusEngine enabled
fn create_test_chat_composer(statusengine_output: Option<StatusEngineOutput>) -> ChatComposer {
    let (tx, _rx) = mpsc::unbounded_channel();
    let app_event_tx = AppEventSender::new(tx);

    let mut composer = ChatComposer::new(
        false, // has_input_focus
        app_event_tx,
        true,          // enhanced_keys_supported
        String::new(), // placeholder_text
        false,         // disable_paste_burst
        true,          // statusengine_enabled
    );

    // Set StatusEngine output
    composer.set_statusengine_output(statusengine_output);

    composer
}

/// Create a test StatusEngineOutput with sample data
fn create_sample_output(line2: &str, line3: Option<&str>) -> StatusEngineOutput {
    StatusEngineOutput {
        line2: line2.to_string(),
        line3: line3.map(|s| s.to_string()),
    }
}

/// Test helper to render StatusEngine footer at a specific width
fn render_statusengine_footer(
    output: Option<StatusEngineOutput>,
    width: u16,
    height: u16,
) -> String {
    let composer = create_test_chat_composer(output);
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_else(|e| {
        panic!("Failed to create test terminal for StatusEngine rendering: {e}")
    });

    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, width, height);
            composer.render_ref(area, f.buffer_mut());
        })
        .unwrap_or_else(|e| panic!("Failed to draw StatusEngine footer to test terminal: {e}"));

    terminal
        .backend()
        .buffer()
        .clone()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

#[test]
fn test_desired_height_without_statusengine() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let app_event_tx = AppEventSender::new(tx);

    let composer = ChatComposer::new(
        false, // has_input_focus
        app_event_tx,
        true,          // enhanced_keys_supported
        String::new(), // placeholder_text
        false,         // disable_paste_burst
        false,         // statusengine_enabled (disabled)
    );

    // Expected: 1 line for textarea + 1 line for hints = 2
    assert_eq!(composer.desired_height(80), 2);
}

#[test]
fn test_desired_height_with_statusengine() {
    let output = create_sample_output("model: gpt-4 | effort: auto", Some("git: main (+2 -1)"));
    let composer = create_test_chat_composer(Some(output));

    // Expected: 1 line for textarea + (hints + line2 + line3) = 1 + 3 = 4
    assert_eq!(composer.desired_height(80), 4);
}

#[test]
fn test_statusengine_lines_rendering_narrow_width() {
    let output = create_sample_output(
        "model: claude-sonnet-3.5 | effort: auto | workspace: codex | git: feat/statusengine (+15 -2) | sandbox: read-only",
        Some("provider: git status --porcelain"),
    );

    let result = render_statusengine_footer(Some(output), 40, 3);
    insta::assert_snapshot!("statusengine_narrow_40", result);
}

#[test]
fn test_statusengine_lines_rendering_medium_width() {
    let output = create_sample_output(
        "model: claude-sonnet-3.5 | effort: auto | workspace: codex | git: feat/statusengine (+15 -2) | sandbox: read-only | approval: on-request",
        Some("provider: git status --porcelain && git log --oneline -1"),
    );

    let result = render_statusengine_footer(Some(output), 80, 3);
    insta::assert_snapshot!("statusengine_medium_80", result);
}

#[test]
fn test_statusengine_lines_rendering_wide_width() {
    let output = create_sample_output(
        "model: claude-sonnet-3.5 | effort: auto | workspace: codex-development-environment | git: feat/statusengine-integration (+25 -8) | sandbox: workspace-write | approval: on-request",
        Some(
            "provider: git status --porcelain --branch && git log --oneline -3 && git diff --stat HEAD~1",
        ),
    );

    let result = render_statusengine_footer(Some(output), 120, 3);
    insta::assert_snapshot!("statusengine_wide_120", result);
}

#[test]
fn test_statusengine_line2_only() {
    let output = create_sample_output(
        "model: gpt-4 | effort: auto | workspace: test",
        None, // No line3
    );

    let result = render_statusengine_footer(Some(output), 80, 3);
    insta::assert_snapshot!("statusengine_line2_only", result);
}

#[test]
fn test_statusengine_disabled() {
    let result = render_statusengine_footer(None, 80, 1);
    insta::assert_snapshot!("statusengine_disabled", result);
}

#[test]
fn test_statusengine_truncation_behavior() {
    // Test various scenarios where text needs truncation
    let long_output = create_sample_output(
        "model: claude-3-5-sonnet-20241022 | effort: automatically-determined | workspace: very-long-workspace-name-that-should-truncate | git: feature/very-long-branch-name-with-lots-of-changes (+999 -888) | sandbox: danger-full-access | approval: on-request-with-confirmation",
        Some(
            "provider: git status --porcelain --branch && git log --oneline -10 && git diff --stat HEAD~5 && git branch -av",
        ),
    );

    // Test at different widths to verify truncation
    let narrow = render_statusengine_footer(Some(long_output.clone()), 30, 3);
    insta::assert_snapshot!("statusengine_truncation_narrow_30", narrow);

    let medium = render_statusengine_footer(Some(long_output.clone()), 60, 3);
    insta::assert_snapshot!("statusengine_truncation_medium_60", medium);

    let wide = render_statusengine_footer(Some(long_output), 100, 3);
    insta::assert_snapshot!("statusengine_truncation_wide_100", wide);
}

/// Integration test: simulate StatusEngine tick and verify ChatComposer integration
#[tokio::test]
async fn test_statusengine_chatcomposer_integration() {
    // Create a StatusEngine with realistic configuration
    let config = StatusEngineConfig {
        provider: "builtin".to_string(),
        command: None,
        command_timeout_ms: 300,
        enabled: false,
    };

    let mut engine = StatusEngine::new(config);
    engine.set_line2_selection(&[
        StatusItem::Model,
        StatusItem::Effort,
        StatusItem::WorkspaceName,
        StatusItem::GitBranch,
        StatusItem::Sandbox,
        StatusItem::Approval,
    ]);

    // Set realistic state
    let state = StatusEngineState {
        model: Some("gpt5".to_string()),
        effort: Some("auto".to_string()),
        workspace_name: Some("codex".to_string()),
        git_branch: Some("feat/statusengine".to_string()),
        git_counts: Some("+5 -2 ?1".to_string()), // staged, unstaged, untracked
        sandbox: Some("read-only".to_string()),
        approval: Some("on-request".to_string()),
        ..Default::default()
    };

    engine.set_state(state);

    // Tick the engine to get output
    let now = std::time::Instant::now();
    let output = engine.tick(now).await;

    // Verify the output structure
    assert!(
        !output.line2.is_empty(),
        "Line2 should contain status information"
    );
    assert!(output.line2.contains("gpt5"), "Should contain model name");
    assert!(
        output.line2.contains("feat/statusengine"),
        "Should contain branch name"
    );
    assert!(output.line2.contains("+5"), "Should contain git counts");

    // Test ChatComposer integration
    let composer = create_test_chat_composer(Some(output.clone()));

    // Test desired height calculation
    assert_eq!(
        composer.desired_height(80),
        4,
        "Height should be 4 with StatusEngine enabled (textarea + hints + 2 status lines)"
    );

    // Test rendering at different widths
    let narrow_render = render_statusengine_footer(Some(output.clone()), 40, 3);
    let wide_render = render_statusengine_footer(Some(output.clone()), 120, 3);

    // The narrow version should be different from wide (due to truncation)
    assert_ne!(
        narrow_render, wide_render,
        "Rendering should differ based on width"
    );

    // Both should render some content
    assert!(
        !narrow_render.trim().is_empty(),
        "Narrow render should not be empty"
    );
    assert!(
        !wide_render.trim().is_empty(),
        "Wide render should not be empty"
    );
}

/// Test ellipsis truncation behavior specifically
#[test]
fn test_statusengine_ellipsis_truncation() {
    let output = create_sample_output(
        "model: claude-3-5-sonnet-20241022-very-long-model-name | effort: automatically-determined-with-high-confidence | workspace: extremely-long-workspace-name-that-definitely-needs-truncation",
        None,
    );

    // Test at very narrow width where ellipsis should appear
    let result = render_statusengine_footer(Some(output), 25, 3);
    insta::assert_snapshot!("statusengine_ellipsis_25", result);
}
