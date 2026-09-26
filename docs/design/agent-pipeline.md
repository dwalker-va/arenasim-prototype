# Agent Pipeline — ArenaSim Dispatch

Status: **v1 shipped 2026-09-05** (board + orchestrator protocol + Engineer role);
**Tester stage shipped** (card AS-1); **Release Manager stage shipped** (card AS-2);
**`human_review` column shipped 2026-09-10** (card AS-39); **board moved off the
claude.ai artifact onto a local MCP daemon** (card AS-153).

A lightweight kanban board drives autonomous agent sessions on the dev machine.
The board is the **source of truth for workflow state**; PRs are the source of
truth for code. GitHub issues are not used.

## The board

- **ArenaSim Dispatch** is a local service, `tools/dispatch-board/` (setup in
  its README and in CLAUDE.md, *Dispatch board MCP*). **One daemon** owns the
  board database and serves, on `127.0.0.1:7453`:
  - `/mcp` — MCP over Streamable HTTP, registered in `.mcp.json` as the
    `dispatch-board` `http` server. Every session — orchestrator, PM — talks to
    the same process through the `mcp__dispatch-board__*` tools.
  - `/` — the web UI: the six columns plus the archived toggle, drag between
    columns, the card drawer, answering a question, the PR-link dialog. It is
    the artifact page ported, refreshing live as other writers change the board.
  - an event feed, which `dist/cli.js wait` blocks on (see *Waking the
    orchestrator*).
- The database is SQLite (WAL) at the **main checkout's** gitignored
  `.dispatch/board.db`, so every worktree sees one board.
  `node tools/dispatch-board/dist/cli.js export` writes the whole board as JSON
  — the backup, and the rollback path to an artifact board.
- The daemon is the user's to run (a terminal of its own), and it must be up
  before a session that uses it starts, or that session reconnects with `/mcp`.
- Localhost only, by design: there is no phone or remote access.

**All writes go through the daemon, which enforces versions and claims.** This
replaces the single-board-writer rule, which existed because the artifact was
last-writer-wins: one automated publisher was safe, several were not. The
daemon refuses what that rule used to prevent by convention:

- every card carries a `version`, and a write naming a stale one is **refused
  with the current card** — nothing is ever overwritten by a writer that had
  not seen the latest state. That includes the web UI's card drawer: its edits
  are based on the version it opened on, Save sends only the fields the user
  changed, and a write that landed meanwhile either survives untouched or
  refuses the save — the drawer then says what changed and the user chooses to
  keep their edits on the new version or discard them;
- a claim is **compare-and-set** (`claim_card`): of two sessions claiming the
  same card, exactly one wins and the other is refused, so a claim can no
  longer be double-spawned by overlapping reads;
- the column rules (the PR-link gate, the `in_progress` claim reset) hold for
  every writer, the web UI and MCP alike.

What that **relaxes**: a PM session may file cards itself with `create_card`
(the id is allocated by the daemon) and move its own scoping card, so the
`.claude/pm-outbox/` handoff becomes optional (see Roles). What it **does not**
relax: there is still **ONE orchestrator** spawning workers. The claim guard
stops a double spawn of one card, but a second orchestrator would still be
running the whole pipeline alongside the first — and its startup sweep would
release the first one's live claims (see Known limits). Workers (Engineer,
Tester, Release Manager) never write to the board; they report to the
orchestrator.

Every write names an **actor** — the writing session's tag (`orchestrator` for
the orchestrator, `board` for every web-UI gesture, e.g. `pm-AS-28` for a PM
session). It is recorded on the write's event, which is how a waiting session
ignores its own writes. Activity entries keep their `by` (defaulting to the
actor). **`orchestrator` is the orchestrator's tag alone:** its wait ignores
that tag, so a write any other session made under it would never wake the
orchestrator. The tag is caller-supplied — the daemon trusts localhost, it does
not authenticate — so this is a rule every session keeps, not one the daemon
enforces.

### State schema (tables)

The card document keeps the artifact's `schema: 1` fields and meanings, so
the protocol reads as it always has:

```json
{ "id": "AS-1", "title": "...", "body": "<spec>",
  "column": "backlog | needs_input | in_progress | review | human_review | done | archived",
  "role": "engineer | tester | release-manager | pm",
  "priority": "P1 | P2 | P3",
  "links": [{"label": "PR #112", "url": "..."}],
  "question": {"text": "...", "answer": "..."} | null,
  "agent": {"status": "working | done", "started": "ISO", "finished": "ISO", "name": "Engineer-AS-1"} | null,
  "released": "v0.2.0" | absent (release tag; set by the orchestrator when a release bundles the card),
  "activity": [{"t": "ISO", "by": "board | claude | orchestrator | engineer | tester | release-manager | pm | user", "msg": "..."}],
  "created": "ISO", "updated": "ISO" }
```

Stored as four tables:

| Table | Holds |
|---|---|
| `cards` | one row per card: the document above minus `activity`, its `version`, and `column`/`role` as indexed generated columns. Every write bumps `version` by one. |
| `activity` | one row per activity entry, **append-only** (triggers refuse UPDATE and DELETE). |
| `events` | one row per write — `cursor` (monotonic), `t`, `actor`, `kind` (`created`, `moved`, `edited`, `answered`, `claimed`, `claim_released`, `claim_finished`, `activity`, `body_appended`, `deleted`), `card`, `data`. The wake-up feed. |
| `meta` | `next_id` and the id prefix — id allocation is the daemon's, so no caller keeps `nextId`. |

`agent.name` is new: `claim_card` records the spawned agent's name there, so
the live-claim audit joins claims to live agents mechanically. Archived cards
from the artifact era keep their legacy shapes (a bare-string `agent`, a
missing `priority`); `export` returns them exactly as imported.

**Reading it costs little.** `list_cards` returns **summaries** — id, title,
column, role, priority, agent, links, updated, released, version — never a
body or activity, and leaves `archived` out unless asked. `get_card` is the
one call that returns a body, and `activity_limit` trims its log. That is the
context fix: the artifact put ~15k tokens of page into the orchestrator on
every read.

### Column semantics

| Column | Meaning | Automation |
|---|---|---|
| `backlog` | written, not started | none |
| `needs_input` | agent blocked on the user; `question.text` holds the question | answering on the board moves the card back to `in_progress` with `agent: null` |
| `in_progress` | an agent should be / is working it | orchestrator spawns the card's role agent when `agent == null` |
| `review` | PR open, awaiting the **Tester's** verification | orchestrator spawns the Tester on entry; APPROVE → `human_review`, REJECT → back to `in_progress` with findings |
| `human_review` | Tester-approved, PR open, awaiting the user's eyeball and/or merge | **none** — both exits are the user's gesture (see The eyeball loop) |
| `done` | **merged** — the PR is in `main` and the work is finished | bundled into the next release (see Release flow) |
| `archived` | shipped in a release (or was that release's trigger card); `released` holds the tag | none — terminal |

**The `agent` field is the dedup guard, and the daemon enforces it.** Any
gesture that moves a card *into* `in_progress` (drag, edit, question answered —
or any `move_card`) sets `agent: null`, which means "needs a spawn". The
orchestrator takes the card with `claim_card` **before** spawning, and
`claim_card` is compare-and-set: it writes `agent: {status: "working", started,
name}` only if the card is claimable *at that instant* — `in_progress` with
`agent == null`, or `review` with `agent == null` *or* `agent.status == "done"`
(the Engineer's normal hand-off; `null` there is a claim reset by startup
recovery). A second claim is refused, so overlapping wakes can never
double-spawn, and a refused claim means "someone holds it — do not spawn".
`claim_card` is also the **only** way a claim is taken: a patch (`update_card`,
`move_card`) may set `agent` to `null` or close a `working` claim out to
`{status: "done"}`, and the daemon refuses any patch that sets `working` or
marks a card `done` that holds no working claim. The web UI never patches
`agent` at all; its one claim gesture is an explicit **Release claim** in the
card drawer (for a claim the user judges stale with no orchestrator running),
which is versioned, logged in the card's activity, and wakes the orchestrator
like any other gesture. A claim never ends silently: every write that changes
`agent` (a patch, or a move into `in_progress`) adds an activity line saying
whose claim was cleared or finished.
Bouncing a card from Review back to In Progress is therefore automatically a
respawn. **Scoping cards invert the guard's meaning:** nothing is ever spawned
for a `role: "pm"` card, so `agent: null` on one is a *resting state*, not a
spawn request, and stays `null` for the card's whole life. Step 2 skips pm
cards by role, and `claim_card` refuses them outright as a backstop.

#### The eyeball loop (`human_review`)

A Tester APPROVE means "no machine objects", not "finished". The user still reads
the PR, and eyeball feedback routinely sends a card back: AS-10 (PR #125) went six
rounds — three Tester REJECTs and **two user bounces**. `human_review` is the home
for that state, so approved-but-open work stops parking in `done` beside genuinely
shipped work, indistinguishable from it.

**The PR says what to look at.** Every PR ends with a `**Human testing:**` line saying
what a human has to check and why a machine could not — `screencapture` and `osascript`
are permission-blocked on this machine, so an agent cannot see pixels, and plenty of
cards turn on a judgment only the user can make (whether a replacement reads cleanly,
whether an impact feels right, whether a joke lands). It is the *inverse* of the banned
Proof/Testing section: that one lists what passed, this one lists what was never
verified. "Nothing needs human testing" is written out rather than omitted, so a
genuinely empty eyeball pass is distinguishable from an author who never considered one.
The Engineer writes it; the Tester verifies the claim before APPROVE.

From `human_review` there are exactly **two exits, and both are the user's
gesture**. No automation runs on the column:

- **Merged** → `done`. Merging is the pipeline's one human gate; no role merges.
- **Eyeball feedback** → `in_progress`, with the findings appended to the card
  `body` under a dated heading (`## User findings`, the counterpart of the
  Tester's `## Tester findings`) and `agent: null`, so step 2 spawns a fresh
  Engineer on the **same PR branch**.

The feedback exit is *mechanically identical to a Tester REJECT* — same body
append, same claim reset, same respawn onto the same PR — and differs only in who
wrote the findings. That symmetry is why the column needs no new machinery.

**Why a separate column, and not "leave approved cards in `review`".** Step 3
spawns a Tester for any `review` card with a PR link whose `agent` is `null`
**or** `agent.status == "done"` — and the APPROVE path sets exactly
`agent.status: "done"`. An approved card left in `review` would therefore be
respawned on every wake, forever. Keeping the approved-but-unmerged state in its
own column leaves that dedup guard untouched.

**Board behaviour:** the PR-link gate AS-4 added for `review` also covers
`human_review`, and the daemon enforces it for every writer: a non-pm card
cannot enter either column without a PR link in `links` (a `move_card` may add
the link in its own `patch`), and a card already there cannot have its last
link patched away. In the web UI, dragging a linkless card there opens the
attach-PR dialog; `role: "pm"` cards remain exempt. A link counts only if its
`url` is non-empty. Cards in `human_review`
render an "awaiting your merge" tag.

#### Scoping cards (`role: "pm"`)

A scoping card travels the same columns with different meanings. It ships no
code and spawns no agent: the work happens in an interactive PM session the
*user* opens (see Roles).

| Column | Meaning for a `pm` card | Automation |
|---|---|---|
| `backlog` | topic scoped, no PM session opened yet | none |
| `in_progress` | a PM session is actively refining the card with the user | **none** — the orchestrator never spawns for a pm card (step 2) |
| `needs_input` | the PM session has written its draft card specs to `.claude/pm-outbox/<card-id>.md` and is waiting on the user's agreement; `question.text` points at that file and says what is being asked | answering on the board returns the card to `in_progress` with `agent: null` — for a pm card that means "keep refining", not "respawn" |
| `review` | **skipped by design** — the user's agreement to the drafts *is* the review, and it happens inside the PM session | none |
| `human_review` | **skipped by design** — a pm card carries no PR and has nothing to merge, so there is no eyeball-and-merge state for it to sit in | none |
| `done` | the derived cards are filed on the board **and** the user agreed to them — *not* the work-card meaning of "merged" | excluded from release bundles, but stamped and archived alongside one (see Release flow) |
| `archived` | unchanged — terminal | none |

**The draft/agreement loop.** A scoping card cycles `in_progress` →
`needs_input` (a draft is in the outbox) → `in_progress` (the user wants
changes) → `needs_input` (the next draft) → … → `done` (the user agrees and the
cards are filed — by the PM session itself with `create_card`, or by the
orchestrator from the outbox). `needs_input`'s existing automation already
implements this — the return to `in_progress` with `agent: null` is exactly
"the user sent the draft back", and step 2's pm exemption guarantees no agent
is spawned to meet it. Agreement on the *first* draft is explicitly not
expected; the bounce is the normal path, not a failure. The final hop is
`in_progress` → `done` **directly**: a pm card never passes through `review` or
`human_review`, because neither of the things those columns wait on — a Tester
verdict, a merge — exists for a card that ships no code.

**Review and Human Review are skipped, not policed.** There is no Tester for card
text, and no PR to merge. A pm card misfiled into either column is already inert —
step 3 spawns a Tester only for a card carrying an open-PR link, `human_review`
has no automation at all, `claim_card` refuses pm cards, and the PR-link gate on
both columns exempts them (AS-4). This documents a property the pipeline already
has; nothing new enforces it.

**Done means *filed*, not *written* — and not *merged*.** A scoping card is done
when its derived cards exist on the board and the user has agreed to them. An
outbox file alone is not done: AS-28's outbox was written 2026-09-06 and sat
unfiled with the card still in `backlog`. The work-card sense of `done` (the PR
is merged) simply does not apply — a pm card has no PR, which is also why release
bundles exclude it (see Release flow).

## The orchestrator

Any long-running interactive Claude Code session on this machine. It is the only
thing that spawns worker sessions; the board never does. Every board read and
write below is an `mcp__dispatch-board__*` tool call with `actor:
"orchestrator"`; `<main>` is the main checkout's absolute path (the session's
CWD flaps — see *Worktree discipline* — so never rely on a relative one).

**Refusals are answers, not errors to retry blindly.** `stale_version` means
the card changed since you read it: the refusal carries the current card —
re-apply your change to that version. `claim_refused` means another claim holds the card:
do not spawn. `gate_refused` means a non-pm card is missing its PR link.

### Waking the orchestrator

A user gesture in the web UI (drag, edit, answer) — or another session's write
— must wake the orchestrator. The daemon records every write as an event with
a monotonic cursor, and `dist/cli.js wait` turns that feed into the shape
Claude Code's **Monitor** tool consumes: each stdout line becomes one
notification, lines within 200ms arrive as one, and a monitor expires after
at most 30 minutes (`timeout_ms` is capped at 1800000) and must be re-armed.

Arm it with:

```
Monitor({
  command: "node <main>/tools/dispatch-board/dist/cli.js wait --follow --since <cursor> --board <board> --ignore-actor orchestrator",
  description: "Dispatch board events",
  timeout_ms: 1800000
})
```

- Each event is one JSON line: `{"cursor": 42, "t": "...", "actor": "board",
  "kind": "moved", "card": "AS-7", "data": {"from": "backlog", "to":
  "in_progress"}}`. The orchestrator's own writes never appear
  (`--ignore-actor orchestrator`); user gestures (`actor: "board"`) and other
  sessions' writes (a PM's `create_card`) do.
- **On expiry, re-arm** with `--since` set to the `cursor` of the last line it
  printed (or the cursor you armed it with, if it printed none), and the same
  `--board`. Nothing is lost across the gap: events wait in the database, and
  the new monitor prints everything after that cursor at once.
- Take the starting cursor **before** the startup read
  (`node <main>/tools/dispatch-board/dist/cli.js head`, which prints
  `{"cursor": N, "board": "<id>"}`), then read, then arm from it — an event
  landing between the read and the arm is then replayed rather than skipped.
  (`--since head` means "from now" and has exactly that gap.)
- `--board` is the id of the database the cursor counts. Cursors restart when
  the board is re-created (a restore: export, fresh db, import), so a re-armed
  monitor that did not carry the id could read the new board's events as news
  past its old cursor; with it, the monitor notices the swap even though it was
  not running when it happened.
- If the daemon is down, the monitor prints `{"error": "daemon_unreachable",
  ...}` rather than going silent, keeps retrying, and prints `{"reconnected":
  true}` when it is back. Tell the user; the daemon is theirs to start.
- A cursor that no longer means anything is printed too: `{"error":
  "cursor_ahead", ...}` (the cursor is past the board's newest event) or
  `{"error": "board_replaced", ...}` (the daemon now serves a different
  database than `--board` names). The monitor resumes from the board's head;
  events between the two boards cannot be replayed, so **re-read the board**
  (run the wake steps) on either line, and carry the line's `board` into later
  re-arms.
- A notification is only a wake-up. Its line says what changed, but the steps
  below always re-read the board rather than acting on the line alone — several
  events can batch into one notification, and the board is the truth.

(`wait` without `--follow` prints the first batch and exits — the shape for
`Bash run_in_background`, one notification per arm. The orchestrator uses
`--follow`, which needs no re-arm per event.)

**Start one:** open a session at the repo (or a worktree); confirm the daemon
answers and take the cursor (`dist/cli.js head`); run the startup recovery
below; then arm the monitor from that cursor.

**Becoming orchestrator (startup recovery).** Workers are in-process subagents,
so they die with the orchestrator session that spawned them — while their card
keeps its `agent: {status: "working"}` claim, which the dedup guard then reads
as "already being worked" forever. A fresh orchestrator has spawned nothing
yet, so **every `working` claim it finds at startup was made by a previous
session and is stale by definition** — no liveness probing is needed. (That
argument holds only under the single-orchestrator assumption — see Known
limits.) Therefore, before arming the monitor, `list_cards(column:
["in_progress", "review"])` once and sweep:

- Every `in_progress` card with `agent.status == "working"` (an Engineer or
  other role agent died mid-run): `release_claim(id, activity: <noting the
  recovery>)`. The normal spawn rule (step 2 below) then respawns it on this
  same pass.
- Every `review` card with `agent.status == "working"` (a Tester died
  mid-run): the same `release_claim`. The review-entry rule (step 3 below)
  then respawns the Tester, since the PR link is still in `links`.

The sweep touches nothing else. `human_review` and `done` cards are past their
agent work (no role runs on either); `needs_input` cards already carry
`agent: null` by protocol; and `archived`
cards are terminal with no automation — step 6 closes the trigger card's claim
when it archives it, so a lingering `agent: working` on an archived trigger
implies a crash mid-archival; either way `archived` is terminal, and the sweep
must leave it alone.

Then run the wake steps below against the recovered state.

**Orchestrator death + resume.** When the orchestrator session dies (terminal
closed, machine rebooted, session killed), its in-process workers die with it —
but their PRs and branches survive on GitHub, their worktree changes survive on
disk, and the board survives in the daemon, which is its own process. Two ways
back:

1. **First choice: resume the session.** `claude --resume` (or `--continue`)
   in the orchestrator's directory restores the session but **not** its
   monitor: re-arm it from the last cursor you processed, which replays every
   event since.
2. **Fresh session as the new orchestrator.** In order:
   a. Confirm the old orchestrator session is actually dead (single-orchestrator
      rule — a live predecessor means stop here).
   b. Run *Start one* above: the cursor, the startup-recovery sweep (every
      `working` claim is stale by definition), the live-claim audit, then the
      monitor.

Either way, nothing that matters is lost with the process: the sweep re-spawns
workers from card state, and open PRs are re-discovered via `gh pr list`.

**On each wake** (a monitor notification, or a worker's completion). Every
versioned write passes the `version` of the card as you last read it — from
`list_cards`, `get_card`, or the previous write's result:

1. `list_cards()` — summaries of every non-archived card. `get_card(id)` only
   for a card you are about to spawn for (its body is the spec) or whose
   question/activity you need.
2. For every card with `column == "in_progress"` and `agent == null`, **except
   cards with `role: "pm"`** — a scoping card in `in_progress` is worked by an
   interactive session the *user* opens, the orchestrator never spawns for it,
   and its `agent` stays `null` permanently (see Scoping cards). Without this
   exemption the orchestrator would try to spawn a `pm` subagent, which does not
   exist as a role definition, and the documented fallback — a `general-purpose`
   subagent — cannot wait on user input, which is the whole reason PM work is an
   interactive session. The orchestrator may note the skip in the activity log
   (*"PM session expected; no agent spawned"*), but only when no such entry is
   already the card's latest (`get_card(id, activity_limit: 1)`): it wakes on
   every board event, and an unconditional append would spam the log for as
   long as the card sits there.
   a. `claim_card(id, name: <the agent's name, e.g. "Engineer-AS-7">)`
      **first**. If it is refused, someone holds the card: skip it, do not
      spawn.
   b. Spawn the card's role agent via the Agent tool —
      `subagent_type: <card role>` (fall back to `general-purpose` carrying the
      role prompt from `.claude/agents/<role>.md` if the definition isn't
      loaded in this session), `isolation: "worktree"`, `name: <the claimed
      name>`, prompt = card id + title + full spec + any prior findings/answers
      from the activity log and `question.answer`. A `release-manager` card
      additionally gets the Done-card bundle in its prompt (see Release flow) —
      the agent does not read the board. Give it a worktree of its own for the
      card's branch, or refuse to reuse one whose branch is another card's —
      see *Worktree discipline*.
3. For every card matching **all three** of: `column == "review"`; **and**
   (`agent == null` **or** `agent.status == "done"`); **and** an open-PR link
   in `links` (`done` is the normal Engineer hand-off; `null` is a claim reset
   by startup recovery):
   a. `claim_card(id, name: "<card id>-test")` first; refused means skip, as
      in 2a.
   b. Spawn the Tester via the Agent tool — `subagent_type: "tester"` (fall back
      to `general-purpose` carrying the role prompt from
      `.claude/agents/tester.md` if the definition isn't loaded in this session),
      `isolation: "worktree"`, `name: <card id>-test`, prompt = card id + title +
      full spec + the PR URL. Same worktree allocation rule as 2b.
   A `review` card *without* a PR link is not spawnable — it needs a human (or a
   board fix), so treat it like `needs_input`. (The daemon's PR-link gate makes
   that a pm card's state only.)
4. On an Engineer's completion notification, parse its `STATUS:` report and
   write the board accordingly:
   - `READY_FOR_REVIEW` → one `move_card(id, "review", patch: {links:
     <existing + the PR>, agent: {...agent, status: "done", finished: <now>}},
     activity: <the SUMMARY>, by: "engineer")`. Column and claim change in the
     same write, so no interrupted turn can leave one without the other.
     (Step 3 then spawns the Tester on this same pass.)
   - `NEEDS_INPUT` → `move_card(id, "needs_input", patch: {question: {text:
     QUESTION}, agent: null}, activity: …)`.
   - `FAILED` → the same single `move_card` as NEEDS_INPUT, with the failure as
     the question text (same claim reset).
5. On a Tester's completion notification, parse its `VERDICT:` report and
   write the board accordingly:
   - `APPROVE` → one `move_card(id, "human_review", patch: {agent:
     {...agent, status: "done", finished: <now>}}, activity: <the FINDINGS
     note>, by: "tester")`. The card now waits on the **user**, who either merges the PR — the pipeline's
     one human gate, which the Engineer and Tester contracts both forbid them
     from passing — or bounces it back to `in_progress` with eyeball feedback.
     Both exits are user gestures and the orchestrator runs nothing on the
     column (see The eyeball loop). It is `agent.status: "done"` that makes the
     separate column necessary: an approved card left in `review` would match
     step 3's spawn condition and be handed back to a Tester on every wake.
   - `REJECT` → one `move_card(id, "in_progress", append: {heading: "Tester
     findings — <date>", text: <the FINDINGS verbatim>}, activity: …, by:
     "tester")`. The findings (the next Engineer's spec addendum) and the move
     land in the same write, so no interrupted turn can leave findings appended
     to a card still in `review` under the Tester's claim. Entering
     `in_progress` sets `agent: null` itself — step 2 then spawns a fresh
     Engineer, who receives the findings as part of the spec.
   - A malformed report (no parseable `VERDICT:`) → `move_card(id,
     "needs_input", patch: {question: {text: <the raw report>}, agent: null})`;
     never guess a verdict.
6. On a Release Manager's completion notification, parse its `STATUS:` report:
   - `RELEASED` → for every card in `CARDS:`, `move_card(id, "archived",
     patch: {released: <TAG>})`; the release-manager trigger card itself (if
     the run was card-triggered) is archived like the bundled cards — the same
     `move_card` with `released: <TAG>` — with one extra step the bundled cards
     don't need: close out its claim in the same write (`patch.agent:
     {...agent, status: "done", finished: <now>}` — the trigger entered
     `in_progress` under a `working` claim; archiving without closing it would
     leave a dangling `working` with no `finished` timestamp), appending the tag
     + release URL as activity (`by: "release-manager"`). It never parks in
     `done`: a release run produces no PR, so a trigger card left in `done`
     would block every subsequent bundle. Any `done` `role: "pm"` card without a
     `released` field is stamped and archived on the same pass for the same
     reason, even though it was never bundled (see Release flow).
   - `NEEDS_INPUT` / `FAILED` → same handling as the Engineer's (step 4): the
     triggering card (if the run was card-triggered) goes to `needs_input` with
     the question or failure text; a user-requested run just surfaces it to the
     user. Either way, no board changes to the bundled Done cards — they stay
     in `done` for the next attempt.
   - A malformed report (no parseable `STATUS:`) → treat like `FAILED`: the
     triggering card (if the run was card-triggered) goes to `needs_input` with
     the raw report as the question text, naming the run; a user-requested run
     surfaces it to the user the same way. Never guess whether the release
     happened; the bundled Done cards stay in `done` untouched.
7. Cards the *user* must see promptly (needs_input) warrant a mention in the
   orchestrator session's next visible message.

**Live-claim audit — every wake, not just startup.** Steps 2a and 3a claim
before spawning, so a turn interrupted between the two leaves `agent: working`
on the board with no agent ever spawned — under a *live* orchestrator, where
startup recovery never fires because there was no restart. So on each wake,
before processing the steps above, the orchestrator audits the `working`
claims on `in_progress` and `review` cards (the same two columns the startup
sweep covers — never `archived`) against its actual live agent list (the
agents addressable in this session), joining on `agent.name`: any `working`
claim with no matching live agent is a dropped spawn — respawn it (run the
spawn half of step 2b/3b under the existing claim) or `release_claim(id, name:
<that name>)` and let the normal spawn rules pick it up on the same pass.
Claims with a matching live agent are untouched.

**Writing cards as Claude:** `create_card(title, body, role, priority?, actor:
<your session tag>, by: "claude")` — the daemon allocates the id and the card
lands in `backlog`. Any session may do this, a PM session included (see Roles);
it is also how the orchestrator files specs a PM session left in the outbox.

## Worktree discipline

A session's worktree pin **flaps between tool calls**: the process CWD can move to
another card's tree mid-run, with no warning and no gesture from the agent. Why it
flaps is a harness question outside this repo; the pipeline's job is to survive it.
In a single day's session it fired 15+ times across at least four engineers, and
both ways it goes wrong have already happened:

- **A lost commit** — a write lands in another card's tree. `card-AS-60-dual-wield`
  was assigned to one Engineer and re-checked-out onto `card/AS-68-trap-dispellers`
  by a second session that had made no worktree of its own. It is still there, and
  still being written to: its HEAD moved under the fix's own author, mid-review.
- **A stale pass** — a Tester read another tree's files and nearly graded them as the
  PR's, caught only by re-fetching each one via `gh api` at the PR head SHA.

Everything below follows from that one mechanism, and is not re-argued per rule.

**The spawn requirement — allocate a worktree per card, or refuse to reuse one.**
Before spawning a role agent for a card, the orchestrator either creates a worktree
of its own for that card's branch, or — if it hands over a tree that already exists
— confirms that tree's `git branch --show-current` equals the card's branch and
refuses the spawn otherwise. **This is the orchestrator's defect, not an agent's:**
the spawn path today hands an agent whatever tree happens to be current, and handing
over a tree sitting on another card's branch *is* the collision, at the one moment it
is still cheap to prevent. The rules below teach agents to survive the fault; this is
the half that stops it happening.

There is **no orchestrator code in this repo** — the orchestrator is an interactive
Claude Code session following this document — so this is a written requirement to
check compliance against, not an implemented mechanism, and nothing enforces it.

**Checking it** means joining `git worktree list` against the board's live cards: each
one's worktree is on *that card's branch and no other*. The join is the whole check,
and it only runs where a card→tree mapping exists — most trees are harness-named
`agent-a<hash>`, which records no card, so branch name is the only handle and the check
says nothing about the rest. When this was written the live list held 49 worktrees, 27
of them `agent-a<hash>` and 16 detached, and the check **failed** for at least three:
`card-AS-60-dual-wield` sitting on `card/AS-68-trap-dispellers` (the incident above,
still live and still being committed to), `card-AS-101-same-role-beats` on
`card/AS-103-banter-vocab-tokens`, and `as87` on `as97/shaman-weapon-damage`. Nothing
had asked for the requirement yet — that is the point of writing it down.

**The rules — every agent, every run.**

1. **Every command names its tree.** `git -C <absolute worktree path> <cmd>`, and the
   same for anything else that reads the tree
   (`cargo --manifest-path <abs>/Cargo.toml ...`). Pinning the tool to the tree in the
   same process as the action fails loudly against a wrong path, where a bare command
   fails silently against a wrong tree.
2. **Know which tree you are in before anything that writes** — commit, reset,
   checkout, push; the push is the one that loses *other people's* work, but it is the
   earlier writes that do the damage locally. `pwd` alone will not tell you: it reports
   the flapped location, not your tree's branch. A detached HEAD **in your card's
   worktree** means stop and recover; a second tree kept deliberately detached for
   before/after baselining is not a fault. (The Tester's whole checkout is detached by
   sanctioned fallback, so it pins on the PR head SHA, and its trigger is *before it
   measures and again before it reports* rather than a push it never makes. See
   `.claude/agents/tester.md`.)
3. **The push precondition.** Immediately before pushing, confirm that
   `git -C <abs> branch --show-current` equals the card's branch **and**
   `git -C <abs> rev-parse HEAD` equals the SHA the verification actually ran on. If
   either has moved, **discard the measurement and re-run the gates** rather than
   reasoning about whether the move could have mattered. AS-87's Engineer re-ran after
   a move and was right to, even though its evidence later showed the earlier run had
   been on the correct commit.
4. **The check cannot itself be a git command.** After a drift the guard inverts: it
   refuses a correct `git -C <the right tree>` and a `cd` to it, while permitting a
   bare `git` against the wrong one — so `branch --show-current`, the obvious check, is
   exactly what you may be unable to run. It also refuses anything it cannot *prove* is
   not git, which is a wider net than it sounds: one Tester's `for` loop over `git -C`
   came back "too complex to verify", and AS-117's own run was refused for a heredoc
   whose only offence was containing the word and for a `sed` the guard could not rule
   out. **Keep every check a single plain command.** What always works is reading the tree's
   own files, which the guard does not mediate: `<abs>/.git` gives the gitdir, and
   `<gitdir>/HEAD` gives the branch or SHA. Use `gh api` for the remote side. That read
   is the identity check — not a fallback to it.

**When it happens anyway — re-pin the session first.** Once the CWD has drifted, the
guard refuses *both* obvious ways back: a correct `git -C <the right tree>` and a `cd`
to it. So the first move is **`EnterWorktree` at your worktree's explicit path**, which
re-pins the session; only then re-attach with `git -C <abs> checkout <branch>` (the
branch ref usually survives — a rebase moves the ref before HEAD can be disturbed) and
re-run the gates. Treat `EnterWorktree` as an **observed** remedy, not a guarantee: it
is what worked for AS-68's run and repeatedly during AS-117's own, and no one has
tested where it fails.

Rule 4's file read and `EnterWorktree` are the two halves of this and neither
substitutes for the other: the read is guard-immune but tells you **only where you
are**, and `EnterWorktree` is the one that gets you out.

*Last resort, only when committing locally would switch a branch out from under
another session's live work:* push through the GitHub Git Data API (blobs → tree →
commit → ref update), then verify by reading the commit back off GitHub. One Engineer
has already had to. It **bypasses local hooks and authors through the `gh`
credentials**, so it is the last option and never the convenient one.

**Scope.** This section is about *losing commits*. The same flap has a second
consequence — a headless run reading another tree's **assets** and silently measuring
the wrong content — which the rules here do not address and which needs its own fix
(a binary run beside its own pinned assets). Keep the two apart: one costs work, the
other costs a result you believed.

## Roles

- **PM** — a **dedicated interactive session** the user opens for scoping and
  product discussions that span many user-paced turns. Not the orchestrator
  session (scoping turns would interleave with orchestration updates and
  scroll or compact out of history) and not a spawned subagent (workers cannot
  wait on user input). A `role: "pm"` scoping card is the handoff: its body
  carries the full context; the PM session reads the board and the relevant
  files, refines with the user for as long as needed, and ends with the
  derived cards filed on the board. PM sessions do not spawn engineers; their
  actionable output is card text, not code.

  **Starting one.** The start gesture is the same as an engineer card's: the
  **user drags the scoping card into In Progress**. The only difference is who
  opens the session — a human, because step 2 skips pm cards and the
  orchestrator never spawns one. So the PM session's *first* turn reads the
  board (`get_card`) and, if its card is still in `backlog`, asks the user to
  drag it. From there the card follows the draft/agreement loop in Scoping
  cards, and is done only once the derived cards are filed on the board and the
  user has agreed to them.

  **The handoff.** A PM session writes to the board directly, through the
  daemon, with its own actor tag (e.g. `pm-AS-28`): once the user agrees, it
  files the derived cards with `create_card` and moves its scoping card to
  `done` itself. Its writes wake the orchestrator like any other session's, so
  there is nothing to relay. The `.claude/pm-outbox/<card-id>.md` file
  (gitignored: working files, not repo content) is now **optional** — still
  the right home for a long draft the user is asked to agree to (the
  `needs_input` question can point at it), and the fallback when the daemon is
  down: the user tells the orchestrator ("AS-25 scoping is done"), which reads
  the file and files the cards itself.
- **Engineer** — `.claude/agents/engineer.md`. Isolated worktree → PR, whose description
  states what a human must check (see The eyeball loop). Reports
  `READY_FOR_REVIEW / NEEDS_INPUT / FAILED` in a fixed format.
- **Tester** — `.claude/agents/tester.md`. Verification only: no Edit/Write
  tools by definition. Checks out the card's PR branch in its own isolated
  worktree (`gh pr checkout`, detached-fetch fallback), runs
  `cargo build --release` + `cargo test` plus the probe/snapshot suites the diff
  touches (movement probes, registration audit, layout/egui snapshots), and does
  an independent review of the diff (correctness, repo conventions,
  byte-identity constraints, missing registrations) plus a check that the PR's
  human-testing statement is present and true. Reports a machine-parsed
  `VERDICT: APPROVE | REJECT` with a PR URL and FINDINGS; the orchestrator moves
  the card to Human Review on APPROVE — where the user merges it or bounces it
  back with eyeball feedback — or straight back to In Progress (findings appended
  to the spec, `agent: null`) on REJECT. Never fixes the Engineer's work, never
  pushes.
- **Release Manager** — `.claude/agents/release-manager.md`. Bundles the Done
  cards the orchestrator hands it into a tagged GitHub release: verifies each
  listed PR is merged to the `origin/main` HEAD it will tag, drafts
  **player-facing** notes grouped by what a player experiences — no card ids, no
  PR links, no pipeline section — with the standing install section from
  `packaging/release-notes.md` appended verbatim, picks the next `v0.x.y` tag
  from the existing scheme, and publishes with `gh release create --target
  <verified sha>`. Bash/Read/Grep/Glob only — it writes no repo files (notes go straight
  through `gh`), never merges, never pushes branches, never touches the board.
  Reports `RELEASED / NEEDS_INPUT / FAILED` in a fixed format; board archival
  of the bundled cards — and of the trigger card, when the run was
  card-triggered — is the orchestrator's job (see Release flow).

## Release flow

Releases are on-demand, not automatic — Done cards accumulate until someone asks.

**Requesting a release.** Either the user asks the orchestrator directly, or a
card with `role: "release-manager"` is dragged to In Progress (the normal step-2
spawn path then fires). Both routes converge on the same spawn.

**What the orchestrator passes.** The Release Manager cannot read the board, so
the orchestrator assembles the bundle from board state and puts it in the spawn
prompt: every card in `done` without a `released` field **and without
`role: "pm"`**, each as its id, title, PR link(s) from `links`, and the
Engineer's SUMMARY from the activity log. Two kinds of card are excluded by
construction:

- A **release-manager trigger card** — the current run's trigger sits in
  `in_progress`, and every previous run's trigger was stamped and archived with
  its bundle (see post-release archival below).
- A **scoping card** (`role: "pm"`) — it ships no code and carries no PR, so
  handing it to the Release Manager would trip the missing-PR-link blocker on a
  card that cannot satisfy it. It is stamped and archived with the release
  anyway (below), just never bundled.

So this rule only ever collects work cards, each of which has a PR. An empty
bundle is not spawnable — tell the user there is nothing to release.

**The bundle's shape is not the notes' shape.** The ids and PR links travel in
the bundle so the Release Manager can verify each PR is merged and in the tagged
HEAD — they are verification *input*, and nothing in the bundle's structure
implies a section, a heading or a bullet in the published notes. v0.3.0 shipped
with a card id on every bullet and a `## Pipeline & tooling` section listing
twelve of them because that separation was not stated anywhere; it now is, in
the agent's contract (items 1 and 2).

**The bundle is correct by construction.** `done` means *merged*: Tester-approved
work whose PR is still open waits in `human_review`, which the rule never
collects. So every work card the bundle picks up already has its PR in `main`,
and the Release Manager's merged-and-ancestor check is a genuine safety net that
should never fire — rather than the expected failure mode it was when `done` also
held approved-but-unmerged cards and a single straggler could block a whole
bundle. If the check ever does fire, a card reached `done` ahead of its merge:
fix that card (or merge its PR), never release around it.

**What the agent does** (`.claude/agents/release-manager.md` is authoritative):
verifies each listed PR is `MERGED` and its merge commit is an ancestor of the
`origin/main` HEAD it records; drafts player-facing notes — Steam-patch-note
register, grouped by what a player experiences, with no card ids, no PR links
and no pipeline section, and the standing install section from
`packaging/release-notes.md` appended verbatim; picks the next tag by inspecting
`git tag` / `gh release list` (pre-1.0 semver — minor bump for any feature in
the bundle, patch for fix-only; `v0.1.0` if the repo had no version tags); and
publishes with `gh release create <tag> --target <verified sha>`. The tag push
triggers `.github/workflows/release.yaml`, whose idempotent create step reuses
the agent's release (notes survive) and attaches the platform binaries — the
release is public for a few minutes before its assets land, which is expected.
Any unmerged PR, tag collision, or empty bundle is a NEEDS_INPUT, never a
silent drop. The merged-and-ancestor check guards the pipeline's one human gate
from the far side: merging is the **user's** step (no role merges), but work
still awaiting that merge sits in `human_review` and is never bundled — so an
unmerged PR in a bundle means a card reached `done` early, not that it is
simply waiting its turn. The agent's NEEDS_INPUT naming it is the prompt to
correct the board (or merge the PR) and re-request the release.

**Post-release board archival — orchestrator, not agent.** On a `RELEASED`
report the orchestrator sets `released: <tag>` on every bundled card and moves
it to `column: "archived"` (off the Done column; the card and its history stay
in board state). A card-triggered run's trigger card gets the identical stamp —
`released: <tag>`, `column: "archived"` — plus the claim closeout
(`agent.status: "done"`, `agent.finished: <now>`; see step 6) — so it is never
left in `done` to leak into the next bundle, nor archived with a dangling
`working` claim. **Scoping cards are stamped from the other side:** every
`done` `role: "pm"` card without a `released` field gets the same
`released: <tag>` + `column: "archived"` treatment on the same pass, even
though it was never in the bundle — so Done stays clean and no pm card lingers
to be re-considered by a later run. The `released` field is also the dedup
guard for the *next* bundle: only Done cards without it are release candidates.

## Known limits

- **One orchestrator at a time.** The startup-recovery argument — "every
  `working` claim found at startup is stale by definition" — is only true
  because a fresh orchestrator has spawned nothing yet *and no other session
  has either*. A second orchestrator starting while the first still has live
  workers would read those live claims as stale, release them to `agent:
  null`, and duplicate-spawn every card the first orchestrator is already
  working. The daemon's compare-and-set claim stops two claims on one card; it
  cannot stop a sweep that deliberately releases a live claim, and nothing
  records which orchestrator a claim belongs to. The protocol simply assumes one
  orchestrator session exists at a time, and the user must not start a second
  while one is running.
- Workers are in-process subagents: they die if the orchestrator session dies
  (their worktree changes survive). The stale `agent: working` claim they leave
  behind is handled by the next orchestrator's startup recovery sweep (see
  *Becoming orchestrator*), which resets it so the normal spawn rules respawn
  the card — but until an orchestrator session connects, the card simply sits
  claimed.
- Wake-up needs a live monitor. If no orchestrator session is open — or its
  monitor expired and was not re-armed — gestures simply queue up as events and
  board state until one arms a monitor from its last cursor (which replays
  them) or reads the board.
- The daemon is a single point of failure by design: while it is down, the
  MCP tools fail, the web UI shows a banner, and `wait` prints
  `daemon_unreachable`. No state is lost — the database is on disk — and
  `export` works without it.
- Localhost only: one human and a few sessions on one machine, not a team, and
  no phone or remote access.
