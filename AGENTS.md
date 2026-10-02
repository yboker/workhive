## Shared agent skills

`.agents/skills/` is the canonical home for project-specific skills shared by
Claude Code, Codex, Pi, and other agents. Use the repository-local
`.agents/skills/<skill-name>/` for a skill that belongs to this repository; use
`$HOME/.agents/skills/<skill-name>/` for a user-wide skill shared across
repositories.

- Put the canonical `SKILL.md` and its supporting files in `.agents/skills/`.
- Search the shared skill directory before creating a new skill; extend an
  existing skill when it already covers the task.
- Keep provider directories such as `.claude/skills/`, `.codex/skills/`, and
  `.pi/agent/skills/` as discovery adapters or runtime state, not as separate
  canonical copies. A provider adapter should point to the shared skill.
- Keep `AGENTS.md` as the shared repository contract. Provider-specific
  instruction files are local overlays, not a second source of truth.

### Issue tracker

Issues and specs for this repository live in GitHub Issues and are managed with the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the default labels `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, and `wontfix`. See `docs/agents/triage-labels.md`.

### Operational escalation

- Report abnormal conditions separately when they affect review, delegation, CI, dependencies, credentials, or runtime tooling; do not bury them in a normal completion summary.
- Treat review timeouts, runner crashes, stale-run reconciliation, missing output, and partial reviewer output as failed reviews, never as approval. Retry the same review at most three times. After the third failed attempt, stop and ask the user how to proceed.
- Do not modify the host environment without explicit user approval. This includes installing, upgrading, removing, or repairing global packages; changing PATH, Node/Python environments, Pi, LoopX, Codex, or other agent runners; altering system services; and changing credentials or authentication. Diagnose and report the issue first, then wait for approval.
- A successful build, test suite, or `loopx check` does not substitute for an independent review verdict, and an issue must not be closed while the required review is unresolved.

### Runtime/backend readiness

- Treat Herdr availability as two separate gates: the `herdr` executable must be installed, and the Herdr server must be alive and protocol-ready on the configured socket. `herdr --version`, `command -v herdr`, `stat`, or `test -S` prove only installation/path/socket existence; they do not prove server readiness. Before claiming an integration path works, run `herdr status server` and require `status: running`, then exercise the real Workhive command from the target project.
- Interpret `Connection refused` as a stopped server or stale socket, not as a missing installation. Preserve the typed distinction between `BackendBinaryNotFound`, `ServerNotRunning`, `SocketNotFound`, `CapabilityMissing`, protocol incompatibility, permission failures, and other transport errors. User-facing dependency checks must name the actual recovery action; an installed Herdr with a stopped server is not a missing dependency.
- Keep backend selection explicit. Workhive may start a missing configured Herdr named session through Herdr's documented headless launcher, but it never stops, deletes, or rebinds a session it does not own; verify readiness afterward with `herdr status server`. Never delete a socket, retry indefinitely, switch to tmux, or add a legacy/compatibility path to hide an unavailable Herdr runtime.
- A Herdr session is a shared routing scope, not proof of Workhive ownership. Existing bindings and native resources remain untouched when a session is configured or changed; only the exact persisted workspace binding may be operated on.
- Use `HERDR_SOCKET_PATH` only for an intentional alternate-socket test. Production Workhive ignores it so an inherited user environment cannot redirect operations to an unowned endpoint. Normal development and acceptance checks use Herdr's discovered/configured endpoint and the same user/configuration as Workhive; do not hard-code a machine-specific socket.
- Any change touching runtime preflight, backend resolution, init checks, listing, attach, or lifecycle must cover the stopped-server/stale-socket case and a live-server case. A successful build or unit suite is not runtime evidence. The completion gate is: focused regression coverage passes, workspace validation passes, `herdr status server` is checked, and at least one explicit real-project Workhive command reaches the selected backend.

### Domain docs

This is a single-context repository using root `CONTEXT.md` and `docs/adr/`. See `docs/agents/domain.md`.

- TypeScript packages are deep modules: see [packages/README.md](./packages/README.md) before adding or importing one.
