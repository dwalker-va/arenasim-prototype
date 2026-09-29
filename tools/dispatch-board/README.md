# dispatch-board

The ArenaSim Dispatch board as a local service: **one daemon** owns a SQLite
board and serves, on `127.0.0.1:7453`,

- `/mcp` — an MCP server over Streamable HTTP, registered in the repo's
  `.mcp.json` as the `dispatch-board` `http` server, so every session
  (orchestrator, PM, subagents) talks to the same process;
- `/` — the web UI (the ported artifact page: six columns, drag, drawer,
  answering a question, the attach-PR dialog), with a milestone filter
  (`/?m=0.7`) and a link to one card's drawer (`/?card=AS-7`);
- `/milestones/<name>` — a milestone's review page (see *Milestones*);
- `/api/events` — the event feed the `wait` CLI blocks on;
- `/favicon.svg`, `/favicon-32.png`, `/favicon-16.png` — the tab icon: the
  game's own `packaging/icon.svg` and `packaging/icon/icon_{32,16}.png`
  (emitted by `packaging/generate_icon.py`), read in place from this checkout
  on every request, never copied here. A missing file 404s the icon only.

The protocol it implements is `docs/design/agent-pipeline.md`.

## Milestones

A milestone ("0.7") is a release's worth of cards, reviewed by the user once
rather than card by card. The columns are Backlog, Needs Input, In Progress,
Review (the Tester), **Merged** (Tester-approved and merged to `main` by the
orchestrator, awaiting the milestone review), Done (its milestone closed,
awaiting the release) and the archive.

- **The entity:** `name`, `status` (`open` → `in_review` → `released`),
  `created` / `released_at`, `tag`, `release_url`, `review_sha` (the main
  commit the review checklist applies to), `sweep` (the milestone sweep's
  summary and link), and `submissions`. `released` is reached only by
  `close_milestone`, which moves every one of its merged cards to done in the
  same write, is refused while any of its cards is unfinished, and is final.
- **Move rules**, for every writer: a card enters `merged` only through
  `mark_merged` (a plain move needs its merge already recorded), and a work card
  on an unreleased milestone reaches `done` only through `close_milestone`.
- **Card fields:** `milestone` and `iteration` (1 for planned work, 2+ for review
  feedback; a card given a milestone defaults to 1), `area` (combat, visuals,
  ai, ui, tooling), `summary` (what changed, in a player's words),
  `human_testing` (the PR's steps, one per line), `sweep` (`done-on-card` /
  `deferred-to-milestone` / `none`, plus a line), `gaps`, and — never
  patchable — `rulings` (dated user decisions with their numbers, appended by
  `record_ruling`), `merge_sha` / `merged_at` (set by `mark_merged`) and, on a
  feedback card, `source`. A released milestone takes no new cards.
- **The review page** is assembled by the daemon (`get_milestone` is the same
  payload) from those fields alone: *What changed* (finished cards by area),
  *What to check* (every step of every finished card's `human_testing`, as one
  checklist, with the build it applies to; an explicit "Nothing needs human
  testing" is listed as nothing to check, not as a step), *Your decisions* (every ruling),
  *Balance* (the milestone sweep; which cards deferred to it), *Known gaps and
  follow-ups* (stated gaps; the cards filed while it was open, by id range),
  and *Feedback*.
- **Ticks and comments** persist, each item under its own version (a comment
  changed in another window is shown, never overwritten). They are not board
  events — nothing acts on them, and the orchestrator's `wait` would otherwise
  wake for every box — but open pages refresh on them. **Submitting** files each
  comment as a backlog card on the same milestone at its source's iteration + 1
  (source = the card, or the card owning the checklist step; the milestone's
  latest iteration for a comment on the whole), linked back (`source`, the
  source PR as a reference link, an activity line on the source card), clears
  the comments, and records one `review_submitted` event naming the new cards —
  none, when there was no feedback. A submission names the version of every
  comment it submits, so one changed meanwhile refuses it.
- **Events** added: `ruling`, `merged`, `milestone_created`, `milestone_updated`,
  `milestone_sweep`, `milestone_closed`, `review_submitted`, `migrated`. Every
  line keeps the `{cursor, t, actor, kind, card, data}` shape; a milestone's
  name is in `data.milestone`.

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

## Backup, import, migrate, rollback

```bash
node dist/cli.js export --out board-backup.json     # safe while the daemon runs
node dist/cli.js import state.json                  # EMPTY db only; daemon stopped
node dist/cli.js import saved-artifact-page.html    # reads its <script id="state"> block
node dist/cli.js migrate                            # schema 1 -> 2, daemon stopped
```

`export` writes `{schema: 2, nextId, cards, milestones}` (each milestone with
its ticks and comments); on a database not yet migrated it writes the
`{schema: 1, nextId, cards}` it always did. `import` takes either.

**Schema.** The database records its layout in `meta.schema`: 1 is the AS-153
board, with a `human_review` column; 2 adds milestones and replaces that column
with `merged`. This build refuses to serve (or write) a schema 1 database —
`serve` exits naming the fix — and refuses one newer than itself. `migrate`
first writes a complete copy of the database (`<db>.schema1-<time>.bak`, or
`--backup FILE`), then in one transaction creates the milestone tables, moves
every `human_review` card to `merged` with no milestone and no merge recorded
(each gains one activity line; `mark_merged` records the merge later), records
one `migrated` event and sets the schema. Nothing else changes: the board id
and every event cursor survive, so a running `wait` just reconnects. Restoring
the backup file (with the previous build) is the rollback. Importing a schema 1
state applies the same `human_review` → `merged` move.

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
`packaging/` and board fixtures (`DISPATCH_BOARD_PACKAGING_DIR`,
`DISPATCH_BOARD_FIXTURE`, `DISPATCH_BOARD_EXPORT_FIXTURE`).

## Test

```bash
npm test                 # the suite: concurrency, rules, round trip, daemon, wake-up, UI drawer (jsdom),
                         # milestones, the review page, the schema 1 -> 2 migration
npm run test:mutation    # removes each guard from dist/ and proves the suite fails
```

The real-board round trip reads the gitignored saved page at
`<main checkout>/.claude/pm-outbox/dispatch-board-artifact.html` (or
`$DISPATCH_BOARD_FIXTURE`) in place and skips with a message when it is
absent; a synthetic fixture with every legacy card shape always runs. The
real-board migration test likewise reads the newest schema 1 export in
`<main checkout>/.dispatch/backups/` (or `$DISPATCH_BOARD_EXPORT_FIXTURE`) in
place, loads it into a schema 1 database built by `test/legacy.mjs` (the AS-153
DDL, frozen), plants three `human_review` cards so the move is not vacuous, and
compares the database row by row before and after `migrate`.

This suite is not wired into `cargo test` the way `scripts/tests/` is: it
needs `npm install` (network and a native module), and the repo's rule for
fixture suites is that a missing toolchain FAILS rather than skips — which
would fail `cargo test` for every checkout that never built this tool. Run it
with `npm test` when changing the board.
