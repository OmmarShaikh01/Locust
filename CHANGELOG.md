# Changelog

All notable changes to Locust are documented here. Dates are release dates, not commit dates.

## [v0.0.2] - 2026-11-13

### Added
- Swarm issue tracker: persistent requests/issues with a `Raised -> In Progress -> Self Review ->
  Human Review -> Passed` lifecycle, a 5th Kanban column, a "+ RAISE REQUEST" modal, and
  per-project persistence at `.locust/issues.json`.
- Detailed 4/5-column Swiss Grid Kanban Tracker (Tab 3): directive tasks and agent worker jobs
  alongside the issue tracker, with filter pills, a metrics banner, and a bottom inspection
  drawer.
- Client-server architecture: `locust --server` runs a headless daemon that owns every ACP
  connection and swarm pane; the desktop client auto-spawns one if none is running for the
  project and talks to it over a local IPC pipe/socket. Multiple clients can attach to the same
  daemon.
- Structured in-memory logging (5,000-entry ring buffer) with file rotation to
  `.locust/locust.log`, plus `--debug` for verbose output.
- Session resume: each pane's ACP session id is saved and resumed on the next launch.
- Composer prompt history (Up/Down) with draft preservation when navigating back to the newest
  entry.
- Orchestrator-only human interaction model: only the Controller/Orchestrator pane talks to the
  operator; every other pane is a strictly autonomous worker with a visible verbose
  reasoning/execution-state stream and a one-key `[TALK TO ORCHESTRATOR [O]]` shortcut.

### Changed
- **Presentation layer replaced**: Ratatui/Crossterm removed; the desktop UI is now native GPUI
  (GPU-accelerated), rendering a 12-column Swiss typographic grid with a single sans-serif
  typeface and a Twitter/X-style dark theme. The old TUI keybindings (`Ctrl+P`, `Alt+1..6`,
  `F9`/`F10`, the `b` task-board overlay) no longer apply -- see the README for the current
  shortcuts.
- Navigation consolidated to a single left sidebar; duplicate header tab pills removed.
- `src/gui/app.rs` (3,718 lines, one `CockpitApp` impl) and `src/core/state.rs` (2,333 lines, one
  1,016-line `impl Model`) split into cohesive per-concern submodules (`src/gui/app/*`,
  `src/core/state/*`) for maintainability. No behavior change.
- Full `cargo clippy` cleanup across the codebase (zero warnings on our own code).

### Fixed
- Keystroke registration and focus loss in the Orchestrator composer on Windows (IME/translated
  text vs. raw key tokens).
- Controller agent resolution now checks the configured swarm roster before falling back to
  `acp-claude` / the first configured agent spec.

## [v0.0.1] - 2026-11-05

Initial release: native Rust TUI cockpit (Ratatui/Crossterm) for multi-agent swarms over the
Agent Client Protocol (ACP) 5.0.0.

### Added
- Swarm rosters defined in YAML (`agents/full-software-factory/agents.yaml`), loadable via
  `--swarm-config`.
- Agent-to-agent addressing: `@[Pane Name]: message` directives parsed out of an agent's reply
  and auto-dispatched as that pane's next turn; `@[*]` and `@[Tab Name]` broadcasts.
- Real task-completion tracking from actual turn outcomes (Pending/Done/Failed), never from an
  agent's own claim.
- Run limits (turn cap, timeouts, token limit) with safe defaults, enforced server-side.
- Permission-request modal with allow/deny, defaulting to deny on timeout.
- Transcript export to `.locust/runs/<timestamp>/<pane>.md` with secrets redacted.
- `--check` roster validation and `--usage` telemetry summary.
- Per-project directory isolation via `-p <path>` and a swarm lock file.
