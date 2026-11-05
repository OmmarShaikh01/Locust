# SPEC: Locust cockpit

## Problem
Operators need one terminal app to run, watch and steer a roster of LLM agents, with agents
able to hand work to each other, and with hard limits so a run cannot burn tokens unattended.

## Scope
Locust is a single Rust binary. Agents are external ACP CLIs (claude, agy). No message bus.

## Functional requirements
- FR-1: Load a swarm roster from YAML and show one pane per agent.
- FR-2: An agent can address another with `@[Pane Name]: message`; Locust dispatches it.
- FR-3: Track each handoff as Pending, Done or Failed from the real turn outcome.
- FR-4: `--check` validates the roster and exits 0, 1 (problems) or 2 (cannot read).
- FR-5: Operator can cancel and retry a turn.
- FR-6: Agent permission requests show in a modal. Default is deny on timeout.
- FR-7: Run limits (turn cap, timeouts, token limit) come from config with safe defaults.
- FR-8: Pane transcripts export to `.locust/runs/<timestamp>/<pane>.md`.
- FR-9: Broadcast with `@[*]` and `@[<Tab>]`, under the same turn cap.

## Non-functional requirements
- NFR-1: No panic on bad input or bad config. One clear error line, non-zero exit.
- NFR-2: Core paths are testable offline with a mock ACP agent.
- NFR-3: Debug log at `.locust/locust.log`; secrets are redacted in logs and exports.
- NFR-4: CI runs fmt, clippy, tests on Linux, Windows and macOS.

## Acceptance criteria
- AC-1: `locust --check` on a bad roster prints every problem. Verified by unit tests.
- AC-2: `cargo test --all-targets` passes with no network and no real LLM.
- AC-3: A reply loop between two agents stops at the turn cap. Verified by test.
- AC-4: A denied or timed-out permission request never grants access.

## Out of scope
NATS or other brokers, MCP tool servers, container sandboxes.
