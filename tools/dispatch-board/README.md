# dispatch-board

The ArenaSim Dispatch board as a local service: **one daemon** owns a SQLite
board and serves, on `127.0.0.1:7453`,

- `/mcp` — an MCP server over Streamable HTTP, registered in the repo's
  `.mcp.json` as the `dispatch-board` `http` server, so every session
  (orchestrator, PM, subagents) talks to the same process;
- `/` — the web UI (the ported artifact page: six columns, drag, drawer,
  answering a question, the attach-PR dialog);
- `/api/events` — the event feed the `wait` CLI blocks on;
- `/favicon.svg`, `/favicon-32.png`, `/favicon-16.png` — the tab icon: the
  game's own `packaging/icon.svg` and `packaging/icon/icon_{32,16}.png`
  (emitted by `packaging/generate_icon.py`), read in place from this checkout
  on every request, never copied here. A missing file 404s the icon only.

The protocol it implements is `docs/design/agent-pipeline.md`.

## Setup (required on a fresh checkout)

`dist/` and `node_modules/` are gitignored:

```bash
npm install
npm run build      # tsc: src/*.ts -> dist/*.js
```

Node 20+. The SQLite driver is `better-sqlite3` (prebuilt binaries; Node 20
has no `node:sqlite`).

## Run

```bash
node dist/cli.js serve            # default DB: <main checkout>/.dispatch/board.db
```

The daemon must be running before a session starts (or reconnect with `/mcp`
afterwards): an `http` MCP server that is down simply shows as failed.
`DISPATCH_BOARD_DB` / `--db` and `DISPATCH_BOARD_PORT` / `--port` override
the defaults. The default DB is resolved from git's own files to the MAIN
checkout, so a daemon started from any worktree opens the same board. A second
daemon on the same DB refuses to start.

**Scratch board** (for trying the UI without touching the real one):

```bash
node dist/cli.js serve --db /tmp/dispatch-scratch/board.db --port 17453
# then open http://127.0.0.1:17453/
```

## Wake-up (`wait`)

```bash
node dist/cli.js head    # {"cursor": N, "board": "<id>"}
node dist/cli.js wait --follow --since <cursor> --board <id> --ignore-actor orchestrator
```

prints one JSON line per board event (`{"cursor", "t", "actor", "kind",
"card", "data"}`) — the shape Claude Code's `Monitor` tool turns into one
notification per line. Without `--follow` it prints the first batch and exits
(for `Bash run_in_background`). `--ignore-actor` drops a session's own writes;
the cursor on each line is what to resume from. An unreachable daemon prints
`{"error": "daemon_unreachable", ...}` rather than going silent, and a cursor
that no longer belongs to the served board — past its head (`cursor_ahead`), or
from a database other than the one `--board` names (`board_replaced`: each db
mints an id, and cursors restart when a board is re-created) — prints an error
line and resumes from head (one-shot mode exits 3). Without `--board` the id
is learned on first contact, which protects only a waiter that was already
running across the swap.

## Backup, import, rollback

```bash
node dist/cli.js export --out board-backup.json     # safe while the daemon runs
node dist/cli.js import state.json                  # EMPTY db only; daemon stopped
node dist/cli.js import saved-artifact-page.html    # reads its <script id="state"> block
```

`export` writes the artifact's own `{schema: 1, nextId, cards}` shape, so it
is also the way back to an artifact board.

Import migrates artifact-era cards to the current model: each gains `pr` (the
card's own PR, derived from its hand-off record — never from `links`, which
are references) and `worktree` (null). The rule is in
`docs/design/agent-pipeline.md` (*State schema*); `import` prints what it
derived, what it left null, every ambiguous or url-constructed card, every
in-flight card left without a `pr` (`live_without_pr` — set those by hand),
and every hand-off entry naming more than one PR.

`npm run test:mutation` runs an unmutated copy first as a control: every test a
mutant names must pass there, or no kill is counted. The copies live under
`.mutants/`, outside the checkout layout, so the harness hands them the real
`packaging/` and board fixture (`DISPATCH_BOARD_PACKAGING_DIR`,
`DISPATCH_BOARD_FIXTURE`).

## Test

```bash
npm test                 # the suite: concurrency, rules, round trip, daemon, wake-up, UI drawer (jsdom)
npm run test:mutation    # removes each guard from dist/ and proves the suite fails
```

The real-board round trip reads the gitignored saved page at
`<main checkout>/.claude/pm-outbox/dispatch-board-artifact.html` (or
`$DISPATCH_BOARD_FIXTURE`) in place and skips with a message when it is
absent; a synthetic fixture with every legacy card shape always runs.

This suite is not wired into `cargo test` the way `scripts/tests/` is: it
needs `npm install` (network and a native module), and the repo's rule for
fixture suites is that a missing toolchain FAILS rather than skips — which
would fail `cargo test` for every checkout that never built this tool. Run it
with `npm test` when changing the board.
