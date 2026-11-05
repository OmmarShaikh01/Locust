# Locust

Locust is a native Rust TUI cockpit and orchestrator for multi-agent swarms.
It runs as a single binary with zero external runtime dependencies (no Node.js, no tmux, no Bash).

## Features

- Native TUI Cockpit: Multi-tab, multi-pane terminal interface powered by Ratatui and Crossterm.
- Swarm rosters: Define tabs, panes and agents in YAML (`agents/full-software-factory/agents.yaml`). Pick another file with `--swarm-config`.
- Directory Isolation: Run Locust inside any target project directory via `-p <path>`.
- Agent-to-Agent Addressing: Any agent can hand off work or send a message to another pane by writing `@[Exact Pane Name]: message` in its own reply -- Locust parses it out and auto-dispatches it as that pane's next turn, no human relay required.
- Real Task Completion Tracking: Every dispatched handoff is tracked as Pending/Done/Failed from the real outcome of the turn it triggered, never from an agent's own claim. Counts show in the Dashboard's Frame 1.
- Real-Time Telemetry: Track agent states and token consumption live in the header and monitor dashboard.
- Fuzzy Agent Picker: Quick-jump to any agent across tabs using `Ctrl+P` or `F6`.
- Session resume: Each pane's ACP session id is saved and resumed on the next launch.
- Roster check: `locust --check` validates the roster and exits.

## Installation

Compile and install the native binary with Cargo:

```bash
cargo install --path .
```

Or build a release binary:

```bash
cargo build --release
```

The compiled executable is located at `target/release/locust`.

## Quick Start

### 1. Launch Default Cockpit

Run in the current directory:

```bash
locust
```

Run in a specific project directory:

```bash
locust -p /path/to/project
```

### 2. Select Swarm Roster

Use the default roster, or point at another YAML file:

```bash
locust --swarm-config agents/full-software-factory/agents.yaml
locust --check --swarm-config my-roster.yaml
```

### 3. Keyboard Shortcuts

| Key | Action |
| --- | --- |
| `Tab` / `Shift+Tab` | Cycle focus between panes |
| `Alt+1` .. `Alt+6` | Jump to specific tab |
| `F9` / `F10` | Previous / Next tab |
| `Alt+Left` / `Alt+Right` | Previous / Next tab |
| `Ctrl+P` / `F6` | Quick fuzzy agent search and jump |
| `PgUp` / `PgDn` | Scroll the selected agent's output by 10 lines. With the Controller chat focused (`Tab`), they scroll the chat; `Up`/`Down` scroll 1 line |
| `Home` / `End` | Jump to oldest line / follow newest line |
| `z` | Zoom the agent detail to the full screen (`Esc` leaves) |
| `t` | Show or hide the model's thinking text |
| `?` | Key help overlay |
| `b` | Task board: agent-to-agent handoffs and their status |
| `o` | Switch project (one swarm per project; recent list with Up/Down) |
| `r` | Retry turn on selected pane |
| `x` | Cancel in-flight turn on selected pane |
| `e` | Export transcripts to `.locust/runs/<timestamp>/<pane>.md` |
| `Esc` | Close modal / Clear input |
| `Ctrl+C` | Quit Locust |

### 4. Agent-to-Agent Addressing & Broadcasts

Every Swarm pane (and the Controller chat) is briefed once, on its first turn, with the
roster of other agents in the swarm. From then on, any agent can hand a task to another
agent by writing this on its own line in its reply:

```
@[Exact Pane Name]: your message to that agent
@[*]: broadcast message to all other agents in swarm
@[Tab Name]: broadcast message to all agents on a specific tab
```

Locust parses the directive out of the reply and auto-dispatches it as the target pane's
next turn -- framed as `[Message from <source>]: <message>` so the receiving agent knows
who is talking to it. Unknown pane names are rejected and logged to the activity tracer
instead of silently dropped. Configured limits (turn cap, token limit, timeout) prevent
runaway loops. Agent permission requests trigger a modal where the operator can allow or deny
(default is deny on timeout). Transcripts are exported to `.locust/runs/<timestamp>/<pane>.md`
with secrets automatically redacted.

## Projects, timeouts and message ids

- One swarm per project. `.locust/swarm.lock` blocks a second Locust in the same project.
  Chats, sessions and usage live in that project's `.state/` and `.locust/` folders.
- Roster lookup: `.locust/agents.yaml`, then `agents/full-software-factory/agents.yaml`, then
  the built-in roster.
- Timeouts (`limits:` in the roster): `timeout_secs` is the idle limit (default 180 s with no
  agent output; it restarts on every event). `max_turn_secs` is the hard cap per turn
  (default 1800 s). Waiting for your permission answer counts toward neither. On a timeout
  Locust cancels the turn; press `r` to retry.
- Every agent-to-agent message has an id. The same message inside 10 minutes is sent once.

See `SECURITY.md` for the security review.

## Testing

Run all unit and integration tests:

```bash
cargo test --all-targets
```

## License

MIT
