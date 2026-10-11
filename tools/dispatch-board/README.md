# dispatch-board

The ArenaSim Dispatch board as a local service: **one daemon** owns a SQLite
board and serves, on `127.0.0.1:7453`,

- `/mcp` — an MCP server over Streamable HTTP, registered in the repo's
  `.mcp.json` as the `dispatch-board` `http` server, so every session
  (orchestrator, PM, subagents) talks to the same process;
- `/` — the web UI (the ported artifact page: six columns, drag, drawer,
  answering a question, the attach-PR dialog), with a milestone filter
  (`/?m=0.7`) and a link to one card's drawer (`/?card=AS-7`); its header
  links every open and in-review milestone's review page, and the released
  ones under *Past milestones*, all from the live milestone list;
- `/milestones/<name>` — a milestone's review page (see *Milestones*), linking
  back to the whole board and to that milestone's cards on it;
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
  summary and link), and `submissions` (the rounds submitted before feedback
  batches, each listing the cards it auto-filed). `released` is reached only by
  `close_milestone`, which moves every one of its merged cards to done in the
  same write, is refused while any of its cards is unfinished, and is final.
- **Move rules**, for every writer: a card enters `merged` only through
  `mark_merged` (a plain move needs its merge already recorded); a work card
  on an unreleased milestone reaches `done` only through `close_milestone`, and
  a work card already in `done` cannot join one (detach-then-finish — a move to
  `done` with `milestone: null` — still works); and `archived` means shipped, so
  a move enters it only carrying `released: <tag>`.
- **Cancelling:** `cancel_card(id, reason)` (the drawer's *Cancel card*) retires
  a card that will not ship. It moves to `archived` with `cancelled: {t, by,
  reason}` in place of a release tag and stays there. The review page lists it
  under *Cancelled — not shipped* and in no section that reads as work,
  `close_milestone` neither waits on it nor returns it, and no release bundles
  it. Refused for a card with a recorded merge or a release tag, and for one
  under a working claim. No schema change: `cancelled` is a field of the card
  document, and an archived card with no tag (retired by a plain move before
  this) can be cancelled after the fact.
- **Card fields:** `milestone` and `iteration` (1 for planned work, 2+ for review
  feedback; a card given a milestone defaults to 1), `area` (combat, visuals,
  ai, ui, tooling), `summary` (what changed, in a player's words),
  `human_testing` (the PR's steps, one per line), `sweep` (`done-on-card` /
  `deferred-to-milestone` / `none`, plus a line), `gaps`, and — never
  patchable — `rulings` (dated user decisions with their numbers, appended by
  `record_ruling`), `merge_sha` / `merged_at` (set by `mark_merged`),
  `from_feedback` (the review comments the card was filed for or merged with —
  `[{batch, comment, milestone, outcome}]`, written only by `triage_feedback`)
  and, on a card auto-filed before batches, `source`. A released milestone
  takes no new cards.
- **The review page** is assembled by the daemon (`get_milestone` is the same
  payload) from those fields alone: *What changed* (finished cards by area),
  *What to check* (every step of every finished card's `human_testing`, as one
  checklist, with the build it applies to; an explicit "Nothing needs human
  testing" is listed as nothing to check, not as a step — unless its qualifying
  clause still asks for something, as in "…, but watch the glow"), *Your decisions* (every ruling),
  *Balance* (the milestone sweep; which cards deferred to it), *Known gaps and
  follow-ups* (stated gaps; the cards filed while it was open, by id range),
  and *Feedback*.
- **The review guide** leads the review page, with the sections above folded
  under a collapsed *Full detail* (a milestone with no guide shows them as the
  page, saying so). The orchestrator writes it with
  `set_review_guide(name, guide, expected_version, actor)`, which replaces the
  whole guide under the milestone's version: `guide = {units: [{id, title,
  summary, checks: [{id, text, replay?}] (1-3), cards: [ids on this milestone],
  changed_since?}]}`. Unit and check ids are stable (`[A-Za-z0-9._-]`, ≤32);
  `replay` is the game's headless/replay match config as a JSON object naming
  both teams. It is stored on the milestone document as `review_guide`
  (`{units, updated, by}`) — no schema change — and `get_milestone` returns it
  once, as `guide`, with the units exactly as stored and the tick state beside
  them (`ticks`, keyed `guide:<unit>:<check>`), so `guide.units` can be passed
  straight back. A check's tick is a review item under that key and **carries
  over** across rewrites, except a tick made before its unit's `changed_since`
  (a board time no later than the write) and a tick on a check the new guide
  drops: the write clears both, each under its own version, so a check that is
  new or comes back starts unticked. Refused on a released milestone.
- **Replay button.** A guide check with a `replay` config gets a ▶ on the
  review page, which POSTs `{key: "guide:<unit>:<check>"}` to
  `/api/milestones/<name>/replay`. The daemon looks the config up on the stored
  guide (an unknown milestone or key is 404, a check without a replay 422, and
  a body carrying anything besides `key` — a config, a command — is refused
  with 400), writes it to `<db dir>/replays/replay-<time>-<rand>.json` (files
  over a day old are pruned), and starts the game binary detached, argv exactly
  `[binary, "--replay", <file>]`, its stderr to a `.stderr.log` beside it. It
  answers after 2s: `launched` (with a rebuild warning when the default binary
  is older than the main checkout's HEAD last moved), or `exited` with the code
  and stderr's tail when the game died inside the window. A missing binary is
  503 *build first: cargo build --release* — there is no `cargo run` fallback.
  A second launch of the same check while one is starting, or within 2s of its
  `launched`, is ignored (429); a launch that failed can be retried at once.
  The binary is `--game-bin PATH` on `serve`, else `$DISPATCH_BOARD_GAME_BIN`,
  else `<main checkout>/target/release/arenasim` (the main checkout found the
  way the DB is).
- **Ticks and comments** persist, each item under its own version (a comment
  changed in another window is shown, never overwritten). They are not board
  events — nothing acts on them, and the orchestrator's `wait` would otherwise
  wake for every box — but open pages refresh on them. A comment can be left on
  a guide unit (`guide:<unit>`), a guide check (`guide:<unit>:<check>`, the key
  its tick uses), a card (`card:<id>`), a checklist step (`check:<card>:<hash>`)
  or the milestone as a whole (`milestone`). With a guide, the whole-milestone
  comment and **Submit** sit right below it, outside the collapsed detail.
- **Submitting files no card.** It saves every comment as one **feedback
  batch** on the milestone — `{id: "FB-<n>", milestone, t, by, comments: [{id:
  "c1", key, target, text, triage}]}`, in the `review_batches` table with its
  own `version` — clears the comments, and records one `review_submitted` event
  `{milestone, batch, comments: <count>}`. A comment's `target` is what it was
  written on, as it read then: `{kind: milestone}`, `{kind: card, card,
  title}`, `{kind: step, card, text}`, `{kind: guide_unit, unit, title}` or
  `{kind: guide_check, unit, check, title, text}` (a snapshot is null when its
  item had already left the page). A batch with no comments is the approval. A
  submission names the version of every comment it submits, so one changed
  meanwhile refuses it.
- **Triage is the orchestrator's.** `list_feedback_batches(milestone?,
  pending?)` lists batches (`pending` = comments not yet triaged);
  `get_feedback_batch(id)` reads one whole; `triage_feedback(id, triage: [{comment,
  outcome, card?, reason?}], expected_version, actor)` records each comment as
  `filed` (as `card`), `merged_into` (`card`) or `no_action` (with a `reason`),
  under the batch's version, and in the same write links each named card back
  (`from_feedback`, plus an activity line) — re-triaging a comment moves its
  link. One `feedback_triaged` event. The review page lists every past batch
  with each comment's outcome, or *pending* until one is recorded; rounds
  submitted before batches still show the cards they auto-filed.
- **Events** added: `ruling`, `merged`, `cancelled`, `milestone_created`, `milestone_updated`,
  `milestone_sweep`, `milestone_guide`, `milestone_closed`, `review_submitted`, `feedback_triaged`, `migrated`. Every
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
`DISPATCH_BOARD_DB` / `--db`, `DISPATCH_BOARD_PORT` / `--port` and
`DISPATCH_BOARD_GAME_BIN` / `--game-bin` (the game the replay button launches)
override the defaults. The default DB is resolved from git's own files to the MAIN
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
its ticks, comments and feedback `batches`); on a database not yet migrated it
writes the `{schema: 1, nextId, cards}` it always did. `import` takes either,
and a schema 2 export from before batches (no `batches`) too.

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

The feedback-batch table is additive and needs no migration: the daemon creates
it (`CREATE TABLE IF NOT EXISTS`) when it opens a schema 2 database, and leaves
every existing row — the auto-filed cards and their `source`, a milestone's
`submissions` — as it was. An older build opening the database ignores the
table.

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
                         # milestones, the review page and its guide, the replay button (an injected
                         # spawn or a stub script — never the game), feedback batches and triage,
                         # the review page's escaping, the schema 1 -> 2 migration
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
