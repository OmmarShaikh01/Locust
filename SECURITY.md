# Locust security review

Scope: the Locust binary and how it runs agent CLIs (claude, agy) over ACP. Locust does not
sandbox agents. The agent CLI and the operator's permission answers are the trust boundary.

## Threat model

- The project directory can hold untrusted files (cloned repos, downloaded code).
- An agent can read untrusted text (files, web pages) that tries to steer it (prompt injection).
- Two Locust instances, or two projects, must not share chats or state.

## Findings

| # | Severity | Finding | Status |
| --- | --- | --- | --- |
| 1 | High | Windows: `cmd /C claude` searches the current directory first. A planted `claude.cmd` or `agy.exe` in the project would run instead of the real tool. | Fixed. Spawn sets `NoDefaultCurrentDirectoryInExePath=1`. |
| 2 | High | Two Locust instances in one project share `.state/` and both start agents. Session store writes race. | Fixed. `.locust/swarm.lock` allows one live swarm per project. Stale after 30 s without a heartbeat. |
| 3 | High | Saved agent sessions had no project identity. A copied or moved directory resumed another project's chats. | Fixed. `.locust/project.json` records the root; on mismatch saved sessions are dropped. |
| 4 | Medium | Prompt injection through quoted text: an agent that quotes a file containing `@[Agent]: ...` would forward it to other agents. | Fixed. Directives inside code fences and `>` quotes are ignored. |
| 5 | Medium | Agent-to-agent messages could be replayed (turn retry, duplicate parse), repeating work and spending tokens. | Fixed. Messages have an id (`fnv1a(source, target, text)`). The same id inside 10 minutes is suppressed. Ids persist in `.state/messages.json`. A failed message can be resent. |
| 6 | Medium | A turn that ran past a fixed 180 s was dropped while the agent kept working, then collided with the next prompt. | Fixed. Idle and total limits; on timeout Locust sends `session/cancel` and waits up to 3 s. |
| 7 | Medium | Permission requests show only the tool title, not the command or path. | Fixed. The modal shows the tool's raw input (command, path), masked by the redactor, cut to 400 chars. |
| 8 | Medium | Agents inherit Locust's full environment, including API keys. | Accepted. Agent CLIs need their own credentials. Do not run untrusted rosters. |
| 9 | Low | Transcript exports could be read by other users. | Fixed on Unix (mode 0600). On Windows the folder ACL applies. |
| 10 | Low | Roster from the project (`.locust/agents.yaml`) sets agent prompts. | Accepted. Same trust level as the project's own files. Falls back to the built-in roster. |
| 11 | Low | Redaction is pattern-based and runs only on exports. | Partly fixed. Every activity line is redacted and written to `.locust/locust.log` (rotates at 1 MiB; `--debug` adds debug lines). Pattern-based, so unknown secret formats still pass through. |

## Behaviour that is safe by design

- A permission request that times out or errors is denied (30 s, default deny).
- Unknown agent names in a roster are rejected by `--check`.
- Pane names are sanitized before use as file names.
- The turn cap (default 50) and the optional token limit bound auto-routed work.

## One swarm per project, and switching

- Press `o` to switch project. The new project's lock is taken first, so a refusal leaves
  the current swarm running.
- Switching stops every agent process and drops chats, tasks and message ids of the old
  project. Nothing carries over.
- Recent projects are kept per user in `~/.locust/recent.json`.

## Reporting

Open a private security advisory on the repository.
