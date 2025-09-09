**Invocation Model**
- Process model: Short‑lived per update. The binary reads one JSON payload from stdin, generates the statusline, prints to stdout, and exits. Code: `src/main.rs` reads once via `serde_json::from_reader(stdin.lock())`, renders, then returns.
- Lifecycle owner: The client (Claude Code) launches the process for each statusline refresh/event. No background threads; orchestration is event‑driven from stdin (see qna-stdin-windows-jsonl.md).
- Executable path configuration: In Claude Code’s settings.json as a command‑type statusLine.
  - macOS/Linux: `~/.claude/settings.json`
  - Windows: `C:\\ProgramData\\ClaudeCode\\settings.json`
  - Example:
    - `{ "statusLine": { "type": "command", "command": "~/.claude/ccstatus/CCstatus", "padding": 0 } }` (see README_EN.md “Claude Code Activation”).
- Fallback if command not configured/available: None in this repo. No “file mode” writer; behavior is up to the client if command is missing.

**Input Payload (stdin JSON)**
- Top‑level schema (required unless noted):
  - `session_id: string` – required.
  - `transcript_path: string` – required (used by JSONL scanner and Usage segment).
  - `cwd: string` – required (used for directory display via InputData conversion).
  - `model: object` – flexible `serde_json::Value`. `model.display_name` is read when present; defaults to "Unknown" if missing.
  - `workspace: object` – flexible `serde_json::Value` (tests use `{current_dir, project_dir}`); not required by core logic.
  - `version: string` – informational.
  - `output_style: object` – informational; not interpreted by the generator.
  - `cost: object` – required timing/counters:
    - `total_cost_usd: number (f64)`
    - `total_duration_ms: number (u64)` – drives window logic (COLD/RED/GREEN).
    - `total_api_duration_ms: number (u64)`
    - `total_lines_added: number (u32)`
    - `total_lines_removed: number (u32)`
  - `exceeds_200k_tokens: boolean` – informational.
- Unknown fields: Ignored (serde’s default behavior for extra fields).
- Validation: `session_id` and `transcript_path` must be non‑empty; parse errors surface as an error exit.
- Max payload size: No explicit limit; stdin is read fully into memory (Vec<u8>), so practical limits are system memory/OS pipe limits.

**Update Cadence & Triggers**
- Driver: Event‑driven; the client decides when to invoke the command (e.g., per editor/agent event). The binary itself never polls.
- Probe gating (window logic; all derived from `cost.total_duration_ms`):
  - COLD: `total_duration_ms < COLD_WINDOW_MS` (default 5000 ms, env‑override). Highest priority; deduped by `session_id` and valid state.
  - RED: `(total_duration_ms % 10_000) < 1_000` AND recent API error detected in transcript. Deduped per 10 s window.
  - GREEN: `(total_duration_ms % 300_000) < 10_000` (widened window). Deduped per 5 min window.
- Recommended client cadence: Not mandated. Any reasonable event cadence works; window gating prevents excess probes.
- Final payload on shutdown: Not required/used.

**Output Rules (stdout)**
- Lines: The generator may emit one or two lines.
  - Non‑network segments (model, directory, git, usage) are composed into line 1.
  - Network status is composed into line 2 (when the network segment is enabled). In addition, the network renderer may wrap long breakdowns to a new line if length > ~80 chars.
- Length/truncation: No explicit CLI truncation; no ellipsis applied. A width‑aware path exists only for the TUI preview build.
- ANSI/Unicode: Yes. Emoji and ANSI color codes are used in segments and network renderer.
- Empty output: If all segments are disabled, an empty line is printed. Client handling (retain vs clear) is outside this repo.

**Timeouts & Failure Handling**
- Network probe timeouts (per probe mode):
  - RED: fixed 2000 ms.
  - GREEN/COLD: adaptive `clamp(p95 + 500, 2500..=4000)` using rolling P95; default 3500 ms when insufficient samples.
  - Env override: `CCSTATUS_TIMEOUT_MS` (or `ccstatus_TIMEOUT_MS`), capped at 6000 ms.
  - Proxy health check: 1500 ms.
  - OAuth masquerade probe: 10 s internal cap.
- Invalid JSON/parse error: Process exits with error; nothing is rendered.
- Orchestration errors (network segment): Falls back to render last persisted state; if unavailable, prints `⚪ Unknown`.
- Retry/backoff: Governed by window dedup (per 10 s and 5 min windows) and session dedup for COLD; no separate retry loop.
- Debounce identical lines: Not implemented in this binary; the client may choose to debounce.

**Security & Isolation**
- Sandboxing: None in the binary; reads/writes under `~/.claude/ccstatus/` (state, logs). No privileged operations.
- Secret handling: Enhanced debug logger redacts tokens/authorization strings and logs token length only; JSONL operational log is always‑on and designed to avoid leaking credentials.
- Resource profile (from README): startup < 50 ms; memory < 10 MB; binary ~3–4 MB typical.

**Configuration & Feature Flags**
- Enable/disable: Controlled by Claude Code settings (presence/absence of the command). Within the app, segments can be turned on/off via config, and features are compile‑time flags.
- Priority of modes: Only "command" mode is documented/used here; no file‑mode writer in this repo.
- Scope of config: Global per user under `~/.claude/ccstatus/config.toml`. Themes under `~/.claude/ccstatus/themes/`.
- Feature flags (build): `network-monitoring`, `self-update`, `timings-curl`, `timings-curl-static`, `tui`.
- Environment knobs: `CCSTATUS_DEBUG`, `CCSTATUS_COLD_WINDOW_MS`, `CCSTATUS_TIMEOUT_MS`, optional test vars.

**Compatibility with Claude’s Ecosystem**
- Field names observed:
  - `model.display_name` (used for model segment; optional).
  - `workspace.current_dir` (present in tests), but directory display actually uses `cwd` from stdin.
  - `output_style` accepted but not interpreted.
- Window tokens: Not supplied by the client; windows are computed internally (COLD/GREEN/RED). No case‑sensitive tokens required.
- Reserved keywords: None declared beyond fields above; extra fields are ignored.

**Extensibility**
- Additional fields: Safe to add; ignored unless wired. No capabilities negotiation today.
- Correlation IDs: Internal probes use a UUID (e.g., `probe_<uuid>`) for debug logging; there is no `request_id`/`sequence_number` in stdin.
- Considerations (future): A `sequence_number` could help detect dropped updates client‑side; `request_id` might improve log correlation when available.

**Testing & Validation**
- Sample payloads: See `tests/core/network/network_segment_tests.rs` for constructed `StatuslineInput` examples covering window logic and parsing.
- E2E: `tests/run_e2e_ccstatus.sh` includes a smoke check that the renderer emits emoji on stdout.
- Narrow terminals (< 40 cols): No CLI truncation logic. TUI preview mode wraps segments intelligently by width; CLI output is fixed‑string with optional newline in the network renderer when long.

**Exact stdin JSON (Rust struct)**
- From `src/core/network/network_segment.rs`:

```
pub struct StatuslineInput {
    pub session_id: String,
    pub transcript_path: String,
    pub cwd: String,
    pub model: serde_json::Value,
    pub workspace: serde_json::Value,
    pub version: String,
    pub output_style: serde_json::Value,
    pub cost: CostInfo,
    pub exceeds_200k_tokens: bool,
}

pub struct CostInfo {
    pub total_cost_usd: f64,
    pub total_duration_ms: u64,
    pub total_api_duration_ms: u64,
    pub total_lines_added: u32,
    pub total_lines_removed: u32,
}
```

Notes:
- Unknown fields are ignored.
- `model.display_name` is read when present; otherwise "Unknown" is shown.

**Stdout Formatting Expectations**
- The generator composes segments into up to two lines and prints with ANSI/emoji. The client should accept multi‑line output; there is no built‑in truncation. Long network breakdowns may wrap to a second line internally.

**Recommended Timeout & Cadence**
- Timeouts: Use built‑in mode‑specific timeouts (RED=2 s; GREEN/COLD adaptive 2.5–4.0 s), optionally override with `CCSTATUS_TIMEOUT_MS` (≤ 6 s).
- Cadence: Invoke on normal editor/agent events; the internal window gating (10 s RED; 5 min GREEN; COLD on startup) prevents over‑probing.

**Key File Pointers**
- Entry/invocation: `src/main.rs`, `src/cli.rs`
- Statusline generator: `src/core/statusline.rs`
- Segments: `src/core/segments/*`
- Network orchestration & schema: `src/core/network/network_segment.rs`, `src/core/network/types.rs`
- Network renderer: `src/core/network/status_renderer.rs`
- Config: `src/config/{types.rs,loader.rs,defaults.rs}`
- Q&A on stdin/windows/JSONL: `qna-stdin-windows-jsonl.md`

