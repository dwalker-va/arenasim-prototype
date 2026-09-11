# Agent Pipeline — ArenaSim Dispatch

Status: **v1 shipped 2026-09-05** (board + orchestrator protocol + Engineer role);
**Tester stage shipped** (card AS-1); **Release Manager stage shipped** (card AS-2);
**`human_review` column shipped 2026-09-10** (card AS-39).

A lightweight kanban board drives autonomous agent sessions on the dev machine.
The board is the **source of truth for workflow state**; PRs are the source of
truth for code. GitHub issues are not used.

## The board

- Artifact: **ArenaSim Dispatch**. The board URL is deliberately not written
  into the repo (it is a private artifact URL): it lives in the orchestrator's
  project memory, and the artifact appears in the owner's `/artifacts` list by
  title.
- The page declares the `artifact` capability: every user interaction (drag,
  edit, answer) republishes the page with the new state embedded. State lives in
  the `<script id="state" type="application/json">` block of the published HTML.
- Because republishes push **watch notifications** to any local session watching
  the artifact, the board is a *push* trigger — no polling.

**Single board writer.** Exactly one session publishes the board artifact: the
orchestrator. Every other session — PM sessions included — reads it freely but
never publishes; the in-page save from a user gesture (drag, edit, answer) is
the one designed-in second writer. Card creation or moves requested by a PM
session flow through the orchestrator (see Roles). This is the general form of
the single-orchestrator rule in Known limits: last-writer-wins publishing
tolerates one automated writer, not several.

### State schema (`schema: 1`)

```json
{ "schema": 1, "nextId": 3, "cards": [ {
    "id": "AS-1", "title": "...", "body": "<spec>",
    "column": "backlog | needs_input | in_progress | review | human_review | done | archived",
    "role": "engineer | tester | release-manager | pm",
    "priority": "P1 | P2 | P3",
    "links": [{"label": "PR #112", "url": "..."}],
    "question": {"text": "...", "answer": "..."} | null,
    "agent": {"status": "working | done", "started": "ISO", "finished": "ISO"} | null,
    "released": "v0.2.0" | absent (release tag; set by the orchestrator when a release bundles the card),
    "activity": [{"t": "ISO", "by": "board | claude | orchestrator | engineer | tester | release-manager", "msg": "..."}],
    "created": "ISO", "updated": "ISO"
} ] }
```

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

**The `agent` field is the dedup guard.** Any gesture that moves a card *into*
`in_progress` (drag, edit, question answered) sets `agent: null`, which means
"needs a spawn". The orchestrator sets `agent: {status: "working", ...}` **before**
spawning, so overlapping notifications never double-spawn. Bouncing a card from
Review back to In Progress is therefore automatically a respawn. The same guard
covers the Review column: a `review` card needs a Tester spawn when `agent` is
`null` (a claim reset by startup recovery) *or* `agent.status` is `"done"` (the
Engineer's normal hand-off); the orchestrator flips it to `working` before
spawning the Tester, so overlapping notifications never double-spawn there
either. **Scoping cards invert the guard's meaning:** nothing is ever spawned
for a `role: "pm"` card, so `agent: null` on one is a *resting state*, not a
spawn request, and stays `null` for the card's whole life. Step 2 skips pm
cards by role rather than relying on the guard to hold them back.

#### The eyeball loop (`human_review`)

A Tester APPROVE means "no machine objects", not "finished". The user still reads
the PR, and eyeball feedback routinely sends a card back: AS-10 (PR #125) went six
rounds — three Tester REJECTs and **two user bounces**. `human_review` is the home
for that state, so approved-but-open work stops parking in `done` beside genuinely
shipped work, indistinguishable from it.

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

**Board behaviour** (already live; documented here, not proposed): the PR-link
gate AS-4 added for `review` now also covers `human_review` — dragging a non-pm
card there without a PR link opens the attach-PR dialog, and `role: "pm"` cards
remain exempt. Cards in `human_review` render an "awaiting your merge" tag.

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
orchestrator files the cards). `needs_input`'s existing automation already
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
has no automation at all, and the board's PR-link gate on both columns exempts pm
cards (AS-4). This documents a property the pipeline already has; nothing new
enforces it.

**Done means *filed*, not *written* — and not *merged*.** A scoping card is done
when its derived cards exist on the board and the user has agreed to them. An
outbox file alone is not done: AS-28's outbox was written 2026-09-06 and sat
unfiled with the card still in `backlog`. The work-card sense of `done` (the PR
is merged) simply does not apply — a pm card has no PR, which is also why release
bundles exclude it (see Release flow).

## The orchestrator

Any long-running interactive Claude Code session on this machine. It is the only
thing that spawns worker sessions; the board page never does.

**Start one:** open a session at the repo (or a worktree), then
`Artifact action:"watch" url:<board url>`. A session that published or read the
board recently usually gets its watch restored on `--resume`.

**Becoming orchestrator (startup recovery).** Workers are in-process subagents,
so they die with the orchestrator session that spawned them — while their card
keeps its `agent: {status: "working"}` claim, which the dedup guard then reads
as "already being worked" forever. A fresh orchestrator has spawned nothing
yet, so **every `working` claim it finds at startup was made by a previous
session and is stale by definition** — no liveness probing is needed. (That
argument holds only under the single-orchestrator assumption — see Known
limits.) Therefore, immediately after starting the watch (before processing any
notification), read the board once and sweep:

- Every `in_progress` card with `agent.status == "working"` (an Engineer or
  other role agent died mid-run): set `agent: null` and append an activity
  entry (`by: "orchestrator"`, noting the recovery). The normal spawn rule
  (step 2 below) then respawns it on this same pass.
- Every `review` card with `agent.status == "working"` (a Tester died
  mid-run): set `agent: null`, same activity entry. The review-entry rule
  (step 3 below) then respawns the Tester, since the PR link is still in
  `links`.

The sweep touches nothing else. `human_review` and `done` cards are past their
agent work (no role runs on either); `needs_input` cards already carry
`agent: null` by protocol; and `archived`
cards are terminal with no automation — step 6 closes the trigger card's claim
when it archives it, so a lingering `agent: working` on an archived trigger
implies a crash mid-archival; either way `archived` is terminal, and the sweep
must leave it alone.

Republish the board once with all resets applied, then run the notification
steps below against the recovered state.

**Orchestrator death + resume.** When the orchestrator session dies (terminal
closed, machine rebooted, session killed), its in-process workers die with it —
but their PRs and branches survive on GitHub, and their worktree changes
survive on disk. Two ways back:

1. **First choice: resume the session.** `claude --resume` (or `--continue`)
   in the orchestrator's directory restores the session. The artifact watch on
   the most-recently-used artifact is usually restored automatically, so board
   drags wake it again — but verify with the Artifact `status` action before
   trusting it, and re-arm with an explicit `watch` if it did not come back.
2. **Fresh session as the new orchestrator.** The board URL is deliberately
   not in the repo (see The board); recover it from the orchestrator's project
   memory, or find the artifact in the owner's `/artifacts` list by title.
   Then, in order:
   a. Confirm the old orchestrator session is actually dead (single-orchestrator
      rule — a live predecessor means stop here).
   b. Read the artifact and adopt the live copy as the local base.
   c. Explicitly re-arm the watch (`Artifact action:"watch"` — a read alone
      does not subscribe; only a publish or an explicit watch action does).
   d. Run the startup-recovery sweep above (every `working` claim is stale by
      definition) and the live-claim audit.

Either way, nothing that matters is lost with the process: the sweep re-spawns
workers from card state, and open PRs are re-discovered via `gh pr list`.

**On each republish notification:**

1. `Artifact action:"read"` the board; save the HTML to a local file; extract the
   state JSON block.
2. For every card with `column == "in_progress"` and `agent == null`, **except
   cards with `role: "pm"`** — a scoping card in `in_progress` is worked by an
   interactive session the *user* opens, the orchestrator never spawns for it,
   and its `agent` stays `null` permanently (see Scoping cards). Without this
   exemption the orchestrator would try to spawn a `pm` subagent, which does not
   exist as a role definition, and the documented fallback — a `general-purpose`
   subagent — cannot wait on user input, which is the whole reason PM work is an
   interactive session. The orchestrator may note the skip in the activity log
   (*"PM session expected; no agent spawned"*), but only when no such entry is
   already the card's latest: it wakes on every republish, and an unconditional
   append would spam the log for as long as the card sits there.
   a. Set `agent: {status: "working", started: <now>}`, append an activity entry
      (`by: "orchestrator"`), and **republish the board first** (edit the state
      line in the saved HTML, publish that file with `url:` the board URL; on a
      publish conflict, re-read and re-apply).
   b. Spawn the card's role agent via the Agent tool —
      `subagent_type: <card role>` (fall back to `general-purpose` carrying the
      role prompt from `.claude/agents/<role>.md` if the definition isn't
      loaded in this session), `isolation: "worktree"`, `name: <card id>`,
      prompt = card id + title + full spec + any prior findings/answers from the
      activity log and `question.answer`. A `release-manager` card additionally
      gets the Done-card bundle in its prompt (see Release flow) — the agent
      cannot read the board.
3. For every card matching **all three** of: `column == "review"`; **and**
   (`agent == null` **or** `agent.status == "done"`); **and** an open-PR link
   in `links` (`done` is the normal Engineer hand-off; `null` is a claim reset
   by startup recovery):
   a. Set `agent: {status: "working", started: <now>}`, append an activity entry
      (`by: "orchestrator"`), and **republish the board first** (same conflict
      rule as 2a).
   b. Spawn the Tester via the Agent tool — `subagent_type: "tester"` (fall back
      to `general-purpose` carrying the role prompt from
      `.claude/agents/tester.md` if the definition isn't loaded in this session),
      `isolation: "worktree"`, `name: <card id>-test`, prompt = card id + title +
      full spec + the PR URL.
   A `review` card *without* a PR link is not spawnable — it needs a human (or a
   board fix), so treat it like `needs_input`.
4. On an Engineer's completion notification, parse its `STATUS:` report and
   republish the board accordingly:
   - `READY_FOR_REVIEW` → `column: "review"`, `agent.status: "done"`, add the PR
     to `links`, append the SUMMARY as activity. (Step 3 then spawns the Tester
     on this same pass.)
   - `NEEDS_INPUT` → `column: "needs_input"`, `question: {text: QUESTION}`,
     `agent: null`, activity entry.
   - `FAILED` → `column: "needs_input"` with the failure as the question text,
     `agent: null`, activity entry (same claim reset as the NEEDS_INPUT branch).
5. On a Tester's completion notification, parse its `VERDICT:` report and
   republish the board accordingly:
   - `APPROVE` → `column: "human_review"`, `agent.status: "done"`, append the
     FINDINGS note as activity (`by: "tester"`). The card now waits on the
     **user**, who either merges the PR — the pipeline's one human gate, which
     the Engineer and Tester contracts both forbid them from passing — or
     bounces it back to `in_progress` with eyeball feedback. Both exits are user
     gestures and the orchestrator runs nothing on the column (see The eyeball
     loop). It is `agent.status: "done"` that makes the separate column
     necessary: an approved card left in `review` would match step 3's spawn
     condition and be handed back to a Tester on every wake.
   - `REJECT` → `column: "in_progress"`, append the FINDINGS **verbatim** to the
     card `body` under a dated `## Tester findings` heading (they are the next
     Engineer's spec addendum), set `agent: null` (step 2 then spawns a fresh
     Engineer, who receives the findings as part of the spec), activity entry.
   - A malformed report (no parseable `VERDICT:`) → `column: "needs_input"` with
     the raw report as the question text; never guess a verdict.
6. On a Release Manager's completion notification, parse its `STATUS:` report:
   - `RELEASED` → for every card in `CARDS:`, set `released: <TAG>` and
     `column: "archived"`; the release-manager trigger card itself (if the run
     was card-triggered) is archived like the bundled cards — set
     `released: <TAG>` and `column: "archived"` on it too — with one extra step
     the bundled cards don't need: close out its claim,
     `agent.status: "done"`, `agent.finished: <now>` (the trigger entered
     `in_progress` under a `working` claim; archiving without closing it would
     leave a dangling `working` with no `finished` timestamp), appending the
     tag + release URL as activity (`by: "release-manager"`). It never parks in
     `done`: a release run produces no PR, so a trigger card left in `done`
     would block every subsequent bundle. Any `done` `role: "pm"` card without a
     `released` field is stamped and archived on the same pass for the same
     reason, even though it was never bundled (see Release flow). Republish.
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
before spawning (republish first, then spawn), so a turn interrupted between
the two leaves `agent: working` on the board with no agent ever spawned — under
a *live* orchestrator, where startup recovery never fires because there was no
restart. So on each wake, before processing the steps above, the orchestrator
audits the `working` claims on `in_progress` and `review` cards (the same two
columns the startup sweep covers — never `archived`) against its actual live
agent list (the agents addressable in this session): any `working` claim with
no matching live agent is a dropped spawn — respawn it (run the spawn half of
step 2b/3b under the existing claim) or reset it to `agent: null` and let the
normal spawn rules pick it up on the same pass. Claims with a matching live
agent are untouched.

**Writing cards as Claude:** read the board, edit the state JSON (append a
card, bump `nextId`, activity `by: "claude"`), republish with `url:`. Only the
orchestrator does this (single-board-writer rule) — it is also how card specs
handed off from a PM session get filed.

## Roles

- **PM** — a **dedicated interactive session** the user opens for scoping and
  product discussions that span many user-paced turns. Not the orchestrator
  session (scoping turns would interleave with orchestration updates and
  scroll or compact out of history) and not a spawned subagent (workers cannot
  wait on user input). A `role: "pm"` scoping card is the handoff: its body
  carries the full context; the PM session reads the board and the relevant
  files, refines with the user for as long as needed, and ends by handing the
  **orchestrator** the card specs to file (under the single-board-writer rule
  the PM session never publishes the board itself). PM sessions do not spawn
  engineers; their actionable output is card text, not code.

  **Starting one.** The start gesture is the same as an engineer card's: the
  **user drags the scoping card into In Progress**. The only difference is who
  opens the session — a human, because step 2 skips pm cards and the
  orchestrator never spawns one. So the PM session's *first* turn reads the
  board and, if its card is still in `backlog`, asks the user to drag it. From
  there the card follows the draft/agreement loop in Scoping cards, and is done
  only once the derived cards are filed on the board and the user has agreed to
  them.

  **The handoff.** The durable artifact is a file, not a message: the PM
  session writes its final output — the card specs to file — to
  `.claude/pm-outbox/<card-id>.md` in the repo checkout (gitignored: handoffs
  are working files, not repo content), then tells the user it is done. The
  default path from there is user-relayed: the user notifies the orchestrator
  ("AS-25 scoping is done"), which reads the outbox file and files the cards.
  When live cross-session messaging is available, the PM session may
  additionally message the orchestrator as a wake-up — but it still writes the
  outbox file first; the message is the wake-up, the file is the payload.
  (Cross-session discovery is not reliable — sessions under different
  accounts or clients may simply not reach each other — so the file-plus-user
  path is the protocol, and the message is the nice-to-have.)
- **Engineer** — `.claude/agents/engineer.md`. Isolated worktree → PR. Reports
  `READY_FOR_REVIEW / NEEDS_INPUT / FAILED` in a fixed format.
- **Tester** — `.claude/agents/tester.md`. Verification only: no Edit/Write
  tools by definition. Checks out the card's PR branch in its own isolated
  worktree (`gh pr checkout`, detached-fetch fallback), runs
  `cargo build --release` + `cargo test` plus the probe/snapshot suites the diff
  touches (movement probes, registration audit, layout/egui snapshots), and does
  an independent review of the diff (correctness, repo conventions,
  byte-identity constraints, missing registrations). Reports a machine-parsed
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

## Known limits (v1)

- **One orchestrator at a time.** The startup-recovery argument — "every
  `working` claim found at startup is stale by definition" — is only true
  because a fresh orchestrator has spawned nothing yet *and no other session
  has either*. A second orchestrator starting while the first still has live
  workers would read those live claims as stale, sweep them to `agent: null`,
  and duplicate-spawn every card the first orchestrator is already working.
  There is no claim-ownership mechanism; the protocol simply assumes one
  orchestrator session exists at a time, and the user must not start a second
  while one is running. The single-board-writer rule (see The board) is this
  constraint generalized to publishing: other sessions, PM included, read the
  board but never publish it.
- Workers are in-process subagents: they die if the orchestrator session dies
  (their worktree changes survive). The stale `agent: working` claim they leave
  behind is handled by the next orchestrator's startup recovery sweep (see
  *Becoming orchestrator*), which resets it so the normal spawn rules respawn
  the card — but until an orchestrator session connects, the card simply sits
  claimed.
- Watches are session-local; if no orchestrator session is open, drags simply
  queue up as board state until one connects and reads the board.
- Board writes are last-writer-wins with conflict-reload; fine for one human +
  a couple of sessions, not for a team.
