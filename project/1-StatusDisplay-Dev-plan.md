# Status Display & Timing — Development Plan (codex-rs)

Owner: TUI/Core
Date: 2025-09-09
Status: final

## Conceptual Checklist
- Add stdin-only timing: since_session_ms on every proto event
- Introduce StatusEngine with stable API + 300 ms throttle
- Provide Line 2 (predefined items) and optional Line 3 (provider)
- Keep rendering thin; reuse TUI style helpers and wrapping
- Best-effort Git branch/+x -x with caching & timeouts
- Strict non-blocking behavior; UI never stalls

## Goals, Constraints, Success Criteria
- Goals
  - Timing: derive total_duration_ms from stdin via a monotonic since_session_ms field present on every proto event.
  - Status: StatusEngine composes Line 2 items and can surface Line 3 via a configured command provider only (no file reads). If no command is configured or it yields no data, omit Line 3.
  - Cross-platform, provider-agnostic, low overhead, backward-compatible.
- Non-goals
  - API-only timing (total_api_duration_ms), protocol redesign, provider/model-specific logic.
- Success Criteria (abbrev; see requirement for full list)
  - since_session_ms non-decreasing within a session; max aligns with stopwatch within <200 ms for sessions >5 s.
  - Line 2 shows model | effort | workspace | git_branch(+counts) | sandbox | approval; updates within one render frame after state change.
  - Line 3 populates within ~1 cadence; disappears gracefully when absent; no UI blocking.

## Architecture Overview (as-is → to‑be)
- Event flow
  - codex-core produces typed Events (protocol.rs) → in-memory to TUI (ChatWidget) and, in JSONL mode, codex-exec prints Event as JSON lines.
- Timing: add since_session_ms to Event in protocol; computed in core/session and reset on SessionConfigured.
- StatusEngine (new, TUI crate)
  - Pure engine builds strings; rendering remains in a thin footer widget.
  - Inputs via set_state(StatusEngineState) sourced from Config + live signals (git, approval/sandbox changes, timing).
  - tick(now) enforces a 300 ms throttle for provider invocations; command_timeout_ms configurable.
- Rendering
  - Line 1: existing ChatComposer footer (key hints + tokens used + context left) unchanged.
  - Line 2: engine line, width-aware truncation (center-ellipsis for git branch), simple separators.
  - Line 3: optional provider line, dim defaults per TUI style.


## Key Integration Points
- protocol: codex-rs/protocol/src/protocol.rs (struct Event) — add since_session_ms
- core/session: codex-rs/core/src/codex.rs (Session) — compute since_session_ms once per emitted event
- exec JSONL: codex-rs/exec/src/event_processor_with_json_output.rs — no code change; serialization picks up field
- TUI
  - status engine module: codex-rs/tui/src/statusengine.rs (new)
  - footer widget integration: extend bottom_pane/chat_composer rendering to include Line 2/3 (without disturbing Line 1)
  - git signals: REUSE codex-rs/core/src/git_info.rs for branch; ADD a small counts helper in core (see Step 3). No new TUI git module unless needed.

## Public API (engine)
- statusengine.rs (TUI crate)
  - new(config: StatusEngineConfig) -> StatusEngine
    - fields: command_timeout_ms: u64; command: Option<String>
  - set_state(StatusEngineState) — model, effort, workspace_name, sandbox, approval, git {branch, counts}, timing {since_session_ms}
  - set_line2_selection(items: &[StatusItem]) — enumerated items; default order per requirement
  - tick(now) -> StatusEngineOutput { line2: String, line3: Option<String> }
  - Built-in 300 ms throttle between provider polls; cache last good Line 3
- place `statusengine.rs` in codex/codex-rs/tui/src folder

## Provider Contracts
- Command provider (only)
  - Invocation: spawn the configured command; write a compact JSON payload on stdin; consume the first stdout line as Line 3.
  - Payload: includes model/effort/workspace/git/sandbox/approval/timing.since_session_ms (omit fields not available). Engine is the sole producer of this payload.
  - Timeout: command_timeout_ms (typ. 150–500 ms); on timeout/exit≠0/invalid output → reuse last good line (if any) or omit Line 3; never block UI.
  - Throttle: engine must not invoke the command more frequently than every 300 ms.
  - No file provider: Codex does not read any status files (e.g., ccstatus-codex-monitoring.json). External systems integrate only via the command’s stdin/stdout.

### Command Provider # Input Payload (from engine)
- Stable, additive; omit fields not available
- Outline (example):
```
{
  "model": { "id": "gpt-4o-mini", "provider": "openai" },
  "effort": "medium",
  "workspace": { "name": "codex" },
  "git": { "branch": "feature/x", "counts": "+12 -3" },
  "sandbox": "workspace-write",
  "approval": "on-request",
  "timing": { "since_session_ms": 5230 }
}
```
- Output rule: First stdout line is consumed as Line 3. ANSI allowed; truncated by renderer to fit width.

## Module/File Placement (New/Modified)

### New (TUI)
- `codex-rs/tui/src/statusengine.rs`
  - Contains: `StatusEngineConfig`, `StatusItem`, `StatusEngineState`, `StatusEngineOutput`, `StatusEngine` (line 2/3 assembly, provider logic, 300 ms throttle).
  - KISS: keep provider (file/command) logic in this single file for v1.

### Modified (TUI)
- `codex-rs/tui/src/bottom_pane/chat_composer.rs`
  - Render footer stack: Line 2 (engine), optional Line 3 (engine), then existing Line 1 (key hints/tokens).
  - Update layout to compute `footer_height = 1 + (line2_present as u16) + (line3_present as u16)` and stack via `Layout::vertical([Constraint::Min(1), Constraint::Max(footer_height)])`.
- `codex-rs/tui/src/bottom_pane/mod.rs`
  - Update `desired_height(width)` to include additional footer lines produced by `StatusEngine`.

### Modified (Core)
- `codex-rs/core/src/codex.rs`
  - Add `session_origin: Instant` to `Session` and helper `emit(id, msg)` to attach `since_session_ms` to every Event.
  - Replace direct `tx_event.send(...)` calls with `emit(...)` in this file.
- `codex-rs/core/src/git_info.rs`
  - Add `pub async fn working_diff_counts(cwd: &Path) -> Option<(u64, u64)>` (best‑effort `+added -removed`) using existing timeout machinery.

### Modified (Protocol)
- `codex-rs/protocol/src/protocol.rs`
  - Extend `Event` with `#[serde(skip_serializing_if = "Option::is_none")] pub since_session_ms: Option<u64>`.

### Config (TOML)
- Add parsing for `[tui]` in `~/.codex/config.toml` via `codex-rs/core/src/config_types.rs`:
  - Extend `Tui` with fields directly:
    - `pub statusengine: Option<bool>`,
    - `pub provider: Option<String>`,
    - `pub command: Option<String>`,
    - `pub command_timeout_ms: Option<u64>`.
  - Consumption in TUI: in `codex-rs/tui/src/lib.rs` map to `StatusEngineConfig` (enable flag, provider selection, executable path, timeout clamped 150–500 ms; default 300 ms).
    - Semantics:
      - `statusengine = true` and `provider = "builtin"` → render Line 2 only (plus existing Line 1); no command execution.
      - `statusengine = true` and `provider = "command"` → render Line 2 and Line 3 via command.
      - `statusengine = false` or key absent → StatusEngine disabled; render existing Line 1 only (no Line 2/3).
    - CLI overrides use paths like `-c 'tui.provider="command"' -c 'tui.command="/path/tool"'`.

## Branch & Commit Rules
- Create a new branch: `feat/statusengine` for development.
- After each step: `git add -A && git commit -m "<scope>: <message>"`.
- Example scopes: `network`, `oauth`, `render`, `tests`, `docs`.
- Tests location: place all tests under `<component>/tests/<module>_test.rs`; do not put tests in source files.

## Phased Plan — Milestones (Steps grouped)

- M1 Protocol timing (stdin‑only)
  - Covers steps 1.x: add Event.since_session_ms; Session::emit; replace send sites; tests for monotonic/reset/serialization.
- M2 StatusEngine (engine + command provider + Line 2)
  - Covers steps 2.x: engine types; Line 2 assembly; command provider; tick/throttle; engine‑level tests.
- M3 Git helpers + integration
  - Covers steps 3.x: core working_diff_counts; engine consumption; tests (counts/timeouts/detached HEAD).
- M4 TUI integration & wiring
  - Covers steps 4.x, 5.x, 6.x: render stack (Line 2/3/1), desired_height, ChatWidget→StatusEngine wiring (timing/config/model/approval/sandbox), config mapping from [tui], snapshot tests.
- M5 Validation, performance & rollout
  - Covers steps 7.x, 8.x, 9.x: non‑blocking/throttle/security, repo tooling/tests, CHANGELOG/versioning, acceptance verification.

## Detailed Plan — Steps

1) Protocol timing (stdin-only) [M1]
- 1.1 Add optional field to Event: `#[serde(skip_serializing_if = "Option::is_none")] pub since_session_ms: Option<u64>`
- 1.2 In core Session, track `session_origin: Instant` set when creating Session and whenever emitting SessionConfigured.
- 1.3 Centralize event emission: add helper `Session::emit(msg, id)` that computes `since_session_ms = now - session_origin` and sends Event { id, msg, since_session_ms: Some(ms) }.
- 1.4 Replace direct `tx_event.send(Event { id, msg })` call-sites with `emit()`.
- 1.5 Tests
  - Update protocol serialize_event test to ignore/tolerate the new field (or assert presence with >=0 value).
  - Add core unit test: create session → assert monotonic non-decreasing since_session_ms across a few emitted events; reset to ~0 on SessionConfigured.

2) StatusEngine (TUI) [M2]
- 2.1 Define types: StatusEngineConfig, StatusItem (enum), StatusEngineState, StatusEngineOutput
- 2.2 Implement set_state and merge semantics (only changed fields invalidate cache)
- 2.3 Implement line2 builder per default order: `model | effort | workspace_name | git_branch+counts | sandbox | approval`
  - Style: dim defaults; accent minimal; use Stylize helpers (e.g., .dim(), .bold())
  - Width awareness: center-ellipsis for long branch (helper; see wrapping/line_utils guidance)
- 2.4 Provider subsystem
  - Command provider only: spawn with tokio::process; write input JSON; read first line; enforce `command_timeout_ms`; sanitize env (least privilege)
  - Throttle: enforce 300 ms minimum between command invocations; debounce identical outputs
- 2.5 Tick cadence & scheduling
  - Engine exposes `tick(now)`; bottom pane schedules frames (existing FrameRequester) every ~100–150 ms while visible; engine self-throttles
  - Cache last Line 3 and only request redraw when Line 2 or Line 3 changed
- 2.6 Tests
  - Unit: throttle correctness; command timeout path; output parsing/validation; line2 assembly edge-cases (missing git, long branch)

3) Git signals (reuse core; best-effort, DRY/KISS) [M3]
- 3.1 Extend existing `codex-rs/core/src/git_info.rs` with a lightweight helper
  - `get_branch(cwd)` already available via `collect_git_info`; reuse it.
  - New: `working_diff_counts(cwd) -> Option<(added: u64, removed: u64)>` implemented with `git diff --numstat` against a baseline (e.g., `HEAD`), plus untracked via `ls-files --others --exclude-standard` and `diff --no-index` when feasible. Enforce existing timeouts in git_info to avoid blocking. Return None on timeout/error.
- 3.2 TUI polls core helpers with a throttle (≥1–2 s) and caches last good result. If counts are unavailable, omit `+x -x` without breaking layout.
- 3.3 Tests
  - Core: unit tests around `working_diff_counts` with small temp repos; timeouts and detached HEAD cases.
  - TUI: no new module required; integration covered by snapshot expectations (counts present/omitted).

4) TUI integration (rendering) [M4]
- 4.1 Extend ChatComposer render to a 2–3 line footer stack:
  - Line 1: existing (unchanged)
  - Line 2: engine line2 (dim), single-row
  - Line 3: engine line3 (dim), optional
- 4.2 Update `desired_height` calculations to account for up to 2 extra lines (only when engine has content)
- 4.3 Styling
  - Use Stylize helpers: .dim(), .cyan(), .bold(); avoid `.white()` per style guide
  - Use `vec![…].into()` for lines; avoid manual Style where possible
- 4.4 Snapshot tests (`insta`) in codex-tui to validate footer across widths

5) State wiring [M4]
- 5.1 On SessionConfigured and config mutations, update engine state (model id, effort, cwd/workspace, sandbox, approval)
- 5.2 From TokenCountEvent, keep existing Line 1 behavior; engine not involved
- 5.3 On each Event, pass `since_session_ms` to engine as `timing.since_session_ms`
- 5.4 When exec begins/ends or approvals change, refresh engine state as needed

6) Configuration [M4]
- Location: `~/.codex/config.toml`, section `[tui]`
- Keys (unchanged under [tui]):
  - `statusengine = true` (enable StatusEngine; if false/absent → StatusEngine disabled; Line 1 only)
  - `provider = "command" | "builtin"` (command → Line 3 via command; builtin → Line 2 only)
  - `command = "path/to/executable"` (used when provider = command)
  - `command_timeout_ms = 350` (timeout; clamp 150–500; default 300)
- Document config and environment behavior (no credentials; minimal env)

7) Performance, Safety, and Failure Modes [M5]
- Never block render thread; all IO bounded with timeouts and throttles
- Provider failures reuse last good or omit; git omissions must not disturb layout
- Security: no sensitive env passed to provider; stdin-only timing; sandbox not altered by engine

8) Validation & Tooling [M5]
- Run `just fmt` after Rust changes in codex-rs
- For changed crate(s): `just fix -p codex-tui` (ask before running full suite); run `cargo test -p codex-tui`
- If protocol/core changed, run full suite with `cargo test --all-features` (ask user first)
- Update snapshots with `cargo insta` per repo guidance

9) Rollout & Change Management [M5]
- Protocol change is additive (optional field) and backwards-compatible
- Bump workspace version(s) as needed; update CHANGELOG with “Added: since_session_ms; Added: StatusEngine and TUI status footer”
- Keep golden cases for footer snapshots across widths (narrow/medium/wide)

## Test File Placement
- Protocol: `codex-rs/protocol/tests/event_serialization_test.rs`.
- Core timing: `codex-rs/core/tests/timing_since_session_ms_test.rs`.
- Core git counts: `codex-rs/core/tests/git_counts_test.rs`.
- Core config parsing: `codex-rs/core/tests/statusengine_config_test.rs` (TOML → structs; defaults, clamping, invalid provider fallback).
- TUI unit: `codex-rs/tui/tests/statusengine_test.rs` (command timeout, throttle, invalid output handling); snapshots under `codex-rs/tui/tests/snapshots/`.
- TUI config behavior: `codex-rs/tui/tests/statusengine_config_behavior_test.rs` verifies [tui] combinations:
  - statusengine=false or absent → Line 1 only
  - statusengine=true + provider="builtin" → Line 1 + Line 2
  - statusengine=true + provider="command" (with dummy command) → Line 1 + Line 2 + Line 3
- TUI snapshot: `codex-rs/tui/tests/status_footer_snapshot_test.rs` (narrow/medium/wide).

## Risks & Mitigations
- Long git operations → mitigate with timeouts, caching, and omission
- Provider command hangs → timeouts + throttle + reuse last good output
- Small terminals → width-aware truncation and graceful omission of optional segments
- Event field breaks older tooling → keep field optional; only writers add it

## Implementation Notes
- Use Instant for local timing; convert to ms via saturating_sub; never expose Instant
- For branch center-ellipsis, keep first/last 8–10 chars based on width; prefer middle ‘…’
- Keep allocations low; reuse buffers; avoid per-tick string churn where possible

## Test Plan (summary)
- Protocol/core
  - Unit: since_session_ms monotonic and reset
- Engine
  - Throttle respects 300 ms; multiple ticks within window do not spawn IO
  - Command timeout + malformed JSON → omit or reuse last good
  - File JSON shapes; absent file → None
  - Line 2 assembly with missing fields (no git) → stable separators
- TUI (snapshot)
  - Narrow width: branch ellipsis; counts omitted; no wrapping surprises
  - Medium: all segments render
  - Wide: spacing ok; no flicker; Line 3 truncates at width

## Appendix — Default Line 2 Composition
- `model` (short model id)
- `effort` (if applicable)
- `workspace_name` (basename of cwd)
- `git_branch + git_counts` (optional counts)
- `sandbox` (summarized, e.g., workspace-write)
- `approval` (kebab-case string)
