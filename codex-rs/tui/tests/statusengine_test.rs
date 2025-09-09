use codex_tui::statusengine::{StatusEngine, StatusEngineConfig, StatusEngineState, StatusItem};
use std::time::Instant;

#[test]
fn test_statusengine_creation() {
    let config = StatusEngineConfig::default();
    let engine = StatusEngine::new(config);
    assert_eq!(engine.line2_items().len(), 6);
    assert_eq!(engine.line2_items()[0], StatusItem::Model);
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
    assert_eq!(
        StatusEngine::truncate_with_ellipsis("verylongbranchname", 10),
        "very…name"
    );
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
