# Locust Codebase Facts

## Architecture Overview
- Name: Locust (Native Rust cockpit for multi-agent swarms over Agent Client Protocol).
- Language: Rust 2024 edition (rustc 1.88+).
- Presentation Layer: GPUI 0.2.2 (GPU-accelerated desktop interface with Swiss 12-column grid layout and Twitter/X dark theme). Ratatui and Crossterm completely removed.
- Architecture: Decoupled Client-Server model (`locust --server` daemon managing persistent sessions over local IPC + `locust` GPUI client).
- Protocol: Agent Client Protocol (ACP) 5.0.0.
- Primary Entry: src/main.rs.

## Module Breakdown
- src/acp: ACP client and process manager. Reusable across all frontends.
- src/swarm: Swarm configuration loader, validator, and directive parser.
- src/project: Lockfile and project directory isolation.
- src/redact: Secret redaction filters.
- src/logging: Structured in-memory ring buffer (5,000 entries) with log rotation file sink (.locust/locust.log).
- src/core/state/: Pure state tree and state mutation engine with serializable snapshots (ModelSnapshot). Zero UI dependencies. Split by concern (one `impl Model` block per file; `Model`'s own fields stay private but a few cross-file helpers are `pub(super)`):
  - mod.rs: data types (AcpStatus, AcpServer, SwarmPaneState, Task, RequestState, SwarmRequest, PaneLive, ProjectModal, PermissionModal), the `Model` struct + `Default` impl, core nav/snapshot methods (`snapshot`, `select_tab`/`next_tab`/`prev_tab`, `push_activity`, `refresh_telemetry`), and the private `write_private`/`sanitize_filename` file helpers.
  - directives.rs: agent-to-agent message/task routing and dedup (`approved_directives`, `create_task`, `task_counts`, `cancel_swarm_pane`); `now_secs` is `pub(super)` here since `requests.rs` reuses it.
  - permissions.rs: permission modal set/timeout (`set_permission_modal`, `check_permission_timeout`).
  - export.rs: transcript export to `.locust/runs/<timestamp>/` (`export_transcripts`).
  - controller.rs: Controller composer input, scroll, and reply handling.
  - acp_servers.rs: ACP Servers tab selection/connect/telemetry (`next_server`, `begin_connect_selected`, `acp_tokens_used`, etc.).
  - acp_events.rs: `apply_acp_update`, the single place any agent's real connection status/usage lands.
  - swarm_panes.rs: Swarm pane selection, run lifecycle, turn events, scrolling, zoom/thinking toggles.
  - project_modal.rs: switch-project dialog state.
  - requests.rs: the swarm issue tracker -- `enable_request_store`/`raise_request`/`update_request_state`/`save_requests`, persisted to `.locust/issues.json` per project (see Swarm Issue Tracker below).
  - tests.rs: the full unit test module (unchanged by the split; see Test Suite Status).
- src/core/telemetry.rs: Usage aggregation and system/process metrics calculations. Zero UI dependencies.
- src/ipc: Length-delimited framed IPC protocol over Windows Named Pipes (`\\.\pipe\locust-{hash}`) and POSIX Unix Domain Sockets (`/tmp/locust-{hash}.sock`). Defines ClientCommand and ServerEvent schemas.
- src/daemon: Headless service maintaining persistent ACP agent subprocesses, turn capping, directive auto-routing, and multi-client IPC broadcast.
- src/gui: GPUI desktop frontend implementing the 12-column Swiss typographic grid, Twitter/X theme palette, timeline streaming cards, composer, inspector, and approval modals.
- src/gui/app/: `CockpitApp` (the root GPUI entity) and `run_desktop`, split by view so no single file mixes unrelated tabs. Rendering methods defined outside mod.rs are `pub(super)` so the `Render` impl in mod.rs can dispatch to them:
  - mod.rs: struct `CockpitApp`, `new`/`send_command`/pane-lookup helpers, the `Render` impl (layout shell only -- delegates key handling to `keymap.rs` and each tab's render to its own file), and `run_desktop`.
  - keymap.rs: `handle_key_down`, the single key-event router for modal intercept, composer editing, and global tab/scroll navigation (previously an inline 487-line closure on `Render::render`).
  - shell.rs: `render_header`, `render_sidebar` (persistent chrome around every tab).
  - dashboard.rs / servers.rs / cockpit.rs / kanban.rs: one file per tab (Dashboard, ACP Servers, Swarm Cockpit, Kanban Tracker) matching the UI Navigation Views below. kanban.rs also owns `render_raise_modal` (the "+ RAISE REQUEST" form) and the issue-tracker column/card rendering; keymap.rs intercepts its keyboard input before the normal composer/nav routing.
- src/gui/input.rs: Pure keyboard event translation engine (`key_to_char`) converting platform-specific `gpui::Keystroke` events into standard characters across Windows and POSIX layouts.

## UI Navigation and Views
- Tab 0: Dashboard (full-bleed 4-cell metrics banner, tabular swarm agent matrix, activity tracer).
- Tab 1: ACP Servers (flush server pool list with 1px hairline divider rules, server specification sheet).
- Tab 2: Swarm Cockpit (timeline transcript feed, flat composer bar, telemetry limits inspector).
- Tab 3: Kanban Tracker (detailed 5-column Swiss Grid board: Raised & Queued, In Progress, Self Review, Human Review, Passed/Completed; metrics banner, filter pills, "+ RAISE REQUEST" button, and bottom inspection drawer). Directive tasks and worker jobs also render into this board (filters: All Items / Requests & Issues / Directive Tasks / Agent Worker Jobs) -- a "failed"/"errored" task or job lands in the Human Review column slot (index 3) purely by column-index reuse, not because it was actually reviewed.
- Interactive Navigation: Left vertical sidebar serves as the single primary navigation spine. Duplicate top header tab pills are eliminated. Keyboard shortcuts (`0`, `1`, `2`, `3`, `Tab`, `[`, `]`, `t`, `o`, `PageUp`, `PageDown`, `Home`, `End`, `Esc`) provide quick navigation.

## Keyboard Input and Focus Model
- Default Orchestrator Focus: When viewing the Orchestrator on Tab 2, the composer is focused by default, presenting a visible active cursor and blue border.
- Resilient Character Translation: `key_to_char` handles both `key_char` (IME/translated text) and raw `key` tokens (mapping `"space"` to `" "`, punctuation keys, and Shift modifications), preventing dropped characters on Windows.
- Auto-Focus Typing: Pressing any printable key while the Orchestrator composer is blurred automatically claims focus and inserts the character without loss.
- Modal Keyboard Intercept: Active permission prompts capture `Enter`/`y` for approval and `Esc`/`n` for denial before normal routing.
- Clipboard and Editing Shortcuts: Supports `Ctrl+V`/`Cmd+V` clipboard paste, `Ctrl+C`/`Cmd+C` copy, `Ctrl+U`/`Ctrl+Backspace` line clearing, `Enter` submission, `Backspace` deletion, and `Esc` blurring.
- History Navigation: Up and Down arrow keys in the focused Orchestrator composer navigate backwards and forwards through submitted prompt history, restoring any in-progress draft text when navigating back to the newest prompt. When composer is blurred, Up and Down scroll viewport offsets.
- Controller Agent Resolution: Daemon dynamically resolves the active Orchestrator agent spec from configured swarm roster panes first, then falls back to `acp-claude` and `AGENT_SPECS.first()`. Spec lookup matches both short names (`claude`, `agy`, `opencode`, `codex`) and registered `acp-*` spec IDs.

## Swarm Interaction Model
- Orchestrator-Only Human Interaction Spine: Only the Orchestrator (`Mission Orchestrator` / `Controller`) communicates with human operators. All sub-agents are strictly autonomous execution workers that print out their verbose reasoning stream (internal thoughts & decisions) and real-time execution state (phase, status, tokens, turns, runtime, output).
- Non-Interactive Agent UI: Viewing non-orchestrator agent panes displays a `NON-INTERACTIVE AGENT` badge, prominent verbose reasoning stream, execution state ribbon, and a non-interactive status bar with an instant `[TALK TO ORCHESTRATOR [O]]` shortcut button.
- Scrolling System: Full GPU-accelerated mouse-wheel, trackpad, and keyboard scrolling across all main panels (`id(...).overflow_y_scroll().track_scroll(...)`), including Swarm Cockpit timeline, sidebar agent list, dashboard agent matrix, and activity tracer.

## UI Typography and Layout Standard
- Font: Enforces a single sans-serif typeface across all elements (`Segoe UI` on Windows, `Helvetica Neue` on macOS, `Arial` on Linux). Monospace and serif fonts are prohibited in the presentation layer.
- Spacing System: 4px base Swiss Grid tokens (`spacing_unit: 4px`, `gap_xs: 4px`, `gap_sm: 8px`, `gap_md: 12px`, `gap_lg: 16px`, `gap_xl: 24px`, `padding_xs: 4px`, `padding_sm: 8px`, `padding_md: 12px`, `padding_lg: 16px`, `padding_xl: 24px`).
- Border Radiuses: `radius_none: 0px` for panels, cards, and grid cells; `radius_xs: 2px` for buttons, badges, and inputs; `radius_sm: 4px`; `radius_md: 6px`; `radius_lg: 8px`; `radius_xl: 12px`. Floating bento boxes are prohibited.
- Grid Philosophy: Full-bleed mathematical grid divided by 1px hairline borders (`border_b_1`, `border_r_1`, `border_t_1`, `border_l_1`), 2px solid accent left indicators on active items, and high-contrast uppercase tracked section headers.

## Swarm Issue Tracker
- Lifecycle: `Raised -> InProgress -> SelfReview -> HumanReview -> Passed`, with `Rejected` as a dead-end state and "REWORK"/"FAIL" actions sending items back to `InProgress`. States round-trip through `.as_str()`/`.from_str()`/`.label()` on `RequestState` (src/core/state/mod.rs).
- Persistence: every mutation (`raise_request`, `update_request_state`) immediately rewrites `<project_root>/.locust/issues.json` (src/core/state/requests.rs); `DaemonServer::run` wires this up via `enable_request_store` right after the message store, so requests resume per-project across daemon restarts the same way messages do.
- IPC: `ClientCommand::RaiseRequest { title, target_agent, description }` and `ClientCommand::UpdateRequestState { request_id, new_state, review_note }`; both broadcast a `StateUpdated` snapshot afterward. `new_state` is a free-text string parsed leniently by `RequestState::from_str` (unrecognized input silently falls back to `Raised`).
- UI entry point: the "+ RAISE REQUEST" button on Tab 3 opens a 3-field modal (title / assigned agent / description) with its own Tab-to-advance, Enter-to-submit keyboard routing in `gui/app/keymap.rs`, intercepted before every other key handler.

## Swarm Configuration Rosters
- Default Roster: `agents/full-software-factory/agents.yaml` (5 tabs, 21 agents across Claude and Antigravity models).
- Test Swarm Roster: `agents/test-swarm/agents.yaml` and `agents/test-swarm.yaml` (5 tabs, 21 agents where all agents run on Claude 3.5 Haiku `claude-haiku-3-5` with `effort: low` to minimize token expenditure during testing).
- Custom Roster Loading: Passing `--swarm-config <path>` to `locust` or `--server` loads an alternative YAML roster.

## Test Suite Status
- Unit tests: 105 tests passing offline.
- Integration tests: 26 tests passing offline (tests/mock_acp.rs, 1 test ignored requiring agy CLI on PATH).
- Execution command: cargo test.
- Headless verification: cargo run -- --check, cargo run -- --usage.
- Lint: cargo clippy --all-targets is warning-clean on our own code (ignore the unrelated proc-macro-error2 future-incompat notice from a dependency).
