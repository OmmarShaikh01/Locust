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
- src/core/state.rs: Pure state tree and state mutation engine with serializable snapshots (ModelSnapshot). Zero UI dependencies.
- src/core/telemetry.rs: Usage aggregation and system/process metrics calculations. Zero UI dependencies.
- src/ipc: Length-delimited framed IPC protocol over Windows Named Pipes (`\\.\pipe\locust-{hash}`) and POSIX Unix Domain Sockets (`/tmp/locust-{hash}.sock`). Defines ClientCommand and ServerEvent schemas.
- src/daemon: Headless service maintaining persistent ACP agent subprocesses, turn capping, directive auto-routing, and multi-client IPC broadcast.
- src/gui: GPUI desktop frontend implementing the 12-column Swiss typographic grid, Twitter/X theme palette, timeline streaming cards, composer, inspector, and approval modals.
- src/gui/input.rs: Pure keyboard event translation engine (`key_to_char`) converting platform-specific `gpui::Keystroke` events into standard characters across Windows and POSIX layouts.

## UI Navigation and Views
- Tab 0: Dashboard (full-bleed 4-cell metrics banner, tabular swarm agent matrix, activity tracer).
- Tab 1: ACP Servers (flush server pool list with 1px hairline divider rules, server specification sheet).
- Tab 2: Swarm Cockpit (timeline transcript feed, flat composer bar, telemetry limits inspector).
- Tab 3: Kanban Tracker (detailed 4-column Swiss Grid task/job board: Backlog/Queued, In Progress, Completed, Blocked/Failed/Permissions, metrics banner, filter pills, and bottom inspection drawer).
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

## Swarm Configuration Rosters
- Default Roster: `agents/full-software-factory/agents.yaml` (5 tabs, 21 agents across Claude and Antigravity models).
- Test Swarm Roster: `agents/test-swarm/agents.yaml` and `agents/test-swarm.yaml` (5 tabs, 21 agents where all agents run on Claude 3.5 Haiku `claude-haiku-3-5` with `effort: low` to minimize token expenditure during testing).
- Custom Roster Loading: Passing `--swarm-config <path>` to `locust` or `--server` loads an alternative YAML roster.

## Test Suite Status
- Unit tests: 105 tests passing offline.
- Integration tests: 26 tests passing offline (tests/mock_acp.rs, 1 test ignored requiring agy CLI on PATH).
- Execution command: cargo test.
- Headless verification: cargo run -- --check, cargo run -- --usage.
