---
stage: assessment
generated_at: 2025-09-09T14:12:21Z
accomplishment_path: /Users/ouzy/Documents/DevProjects/codex/project/0-StatusDisplay-Dev-plan-Accomplishment-report.md
dev_plan_path: project/0-StatusDisplay-Dev-plan.md
requirement_path: project/0-StatusDisplay-requirement.md
working_dir: /Users/ouzy/Documents/DevProjects/codex
branch: feat/statusengine
commit: 7176080
mode: reassessment
improvement_round: 3
findings_count: {critical: 0, medium: 1, low: 1}
previous_counts: {critical: 1, medium: 2, low: 0}
---

# Status Display & Timing — Assessment Report

## Verdict Summary
- Feature status: Complete
- Security posture: Strong
- Code quality: High
- Test coverage: Adequate (engine/helpers); Missing (TUI snapshots/integration)
- Documentation: Adequate
- Overall rating: 9/10
- Reassessment: Yes (Round 3)
- Delta since last assessment: Critical 1→0, Medium 2→1, Low 0→1

## Detailed Assessment

Context:
- Code improvements are on branch feat/statusengine.
- Development followed plan project/0-StatusDisplay-Dev-plan.md and requirement project/0-StatusDisplay-requirement.md.
- Accomplishment report path is /Users/ouzy/Documents/DevProjects/codex/project/0-StatusDisplay-Dev-plan-Accomplishment-report.md.

### Alignment to Plan & Requirements
- M1 (protocol timing): Optional since_session_ms field remains and is propagated across event emissions (per Round 1); no regressions observed.
- M2 (StatusEngine): Core complete with clamping, backoff/jitter, env sanitization, and tests.
- M3 (Git helpers): Best-effort counts; minor heuristic limitations remain acceptable.
- M4 (TUI integration): Now complete — engine wired into App state, tick cadence, footer rendering (Lines 2/3), desired height updated.
- M5 (Validation/rollout): Engine tests solid; TUI snapshot/integration tests still missing.

### Code Quality Analysis (evidence)

Protocol timing field (unchanged; correct and optional):
```rust path=/Users/ouzy/Documents/DevProjects/codex/codex-rs/protocol/src/protocol.rs start=398
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Event {
    pub id: String,
    pub msg: EventMsg,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_session_ms: Option<u64>,
}
```

App state wiring and tick cadence:
```rust path=/Users/ouzy/Documents/DevProjects/codex/codex-rs/tui/src/app.rs start=125
// Initialize StatusEngine if enabled with proper config mapping
let status_engine = if config.tui.statusengine.unwrap_or(false) {
    let status_config = Self::build_statusengine_config(&config.tui);
    let mut engine = StatusEngine::new(status_config);
    engine.set_line2_selection(&[
        StatusItem::Model,
        StatusItem::Effort,
        StatusItem::WorkspaceName,
        StatusItem::GitBranch,
        StatusItem::Sandbox,
        StatusItem::Approval,
    ]);
    Some(engine)
} else { None };
...
// Tick every 300ms and forward output to widget
if let Some(status_engine) = &mut app.status_engine {
    let now = Instant::now();
    let output = status_engine.tick(now).await;
    app.chat_widget.set_statusengine_output(Some(output));
    tui.frame_requester().schedule_frame();
}
```

Footer layout and desired height with Lines 2/3:
```rust path=/Users/ouzy/Documents/DevProjects/codex/codex-rs/tui/src/bottom_pane/chat_composer.rs start=158
pub fn desired_height(&self, width: u16) -> u16 {
    self.textarea.desired_height(width - 1)
        + match &self.active_popup {
            ActivePopup::None => {
                if self.statusengine_enabled { 3u16 } else { 1u16 }
            }
            ActivePopup::Command(c) => c.calculate_required_height(),
            ActivePopup::File(c) => c.calculate_required_height(),
        }
}
```

Rendering Lines 2/3 from StatusEngine output:
```rust path=/Users/ouzy/Documents/DevProjects/codex/codex-rs/tui/src/bottom_pane/chat_composer.rs start=1389
fn render_statusengine_lines(&self, area: Rect, buf: &mut Buffer) {
    if area.height >= 2 {
        let [line2_rect, line3_rect] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
        if let Some(ref output) = self.statusengine_output {
            Line::from(output.line2.clone()).style(Style::default().dim()).render_ref(line2_rect, buf);
            if let Some(ref line3) = output.line3 {
                Line::from(line3.clone()).style(Style::default().dim()).render_ref(line3_rect, buf);
            }
        }
    }
}
```

### Security Review
- Provider subprocess hardening: env cleared with minimal PATH, kill_on_drop, explicit kill on timeout; throttle with jitter and exponential backoff — strong posture.
- Git operations bounded by timeouts — safe against UI stalls.
- No sensitive data present in payloads beyond paths and state metadata.

### Test Coverage Analysis
- Engine: unit tests present and organized under codex-rs/tui/tests.
- Git helpers: robust temporary-repo tests.
- TUI: still lacks snapshot or end-to-end tests validating footer lines and width behavior.

### Behavioral Outcomes
- TUI now displays Line 2 and optional Line 3; height adjusts accordingly; updates flow within a frame after state changes.
- No regressions observed in protocol or engine behavior since Round 2.

## Findings & Recommendations (ordered)

### Critical
- None.

### Medium
1) Missing TUI snapshot/integration tests
- Impact: Footer rendering and width/ellipsis behavior not guarded; potential regressions.
- Recommendations:
  - Add insta-based snapshots for narrow/medium/wide widths under codex-rs/tui/tests/snapshots/.
  - Add an integration test to simulate StatusEngine outputs in ChatComposer and assert desired_height and rendering lines.

### Low
1) Provider environment allowlisting and docs
- Impact: While env_clear is used with PATH restored, consider explicit allowlisting if future needs arise (e.g., locale variables) and document the policy.
- Recommendations:
  - Document env semantics in README/TUI docs and ensure future additions go through review.

## Acceptance Criteria Check
- Timing: since_session_ms present and non-decreasing (logical path intact) — satisfied.
- StatusEngine: Line 2 composition and Line 3 provider with 300 ms throttle and timeouts — satisfied.
- Rendering: Footer Lines 2/3 integrated with height adjustments — satisfied.
- Config: Mapping present in app initialization with clamping/validation — satisfied.

## Conclusion
Round 3 completes the user-visible integration. The engine is robust and secured; Git helpers are safe; TUI wiring and rendering are in place. Remaining work is limited to test coverage for rendering.

Overall: Feature ready; maintain with added snapshots and minor docs.

## Appendices
- Key diffs/paths reviewed (since last assessment):
  - Commits: 25c561c (Round 3 integration), 7176080 (formatting)
  - Files touched in Round 3: codex-rs/tui/src/app.rs; codex-rs/tui/src/bottom_pane/{chat_composer.rs,mod.rs}; codex-rs/tui/src/chatwidget.rs; codex-rs/core/src/git_info.rs
  - Merge-base vs origin/main: 62bd0e3

