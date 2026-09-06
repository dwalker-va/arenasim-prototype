# Agent Pipeline — ArenaSim Dispatch

Status: **v1 shipped 2026-09-05** (board + orchestrator protocol + Engineer role);
**Tester stage shipped** (card AS-1); **Release Manager stage shipped** (card AS-2).

A lightweight kanban board drives autonomous agent sessions on the dev machine.
The board is the **source of truth for workflow state**; PRs are the source of
truth for code. GitHub issues are not used.

## The board

- Artifact: **ArenaSim Dispatch** — `https://claude.ai/code/artifact/c0a5ab22-6889-4ffd-b53c-93cd2c11e86d`
- The page declares the `artifact` capability: every user interaction (drag,
  edit, answer) republishes the page with the new state embedded. State lives in
  the `<script id="state" type="application/json">` block of the published HTML.
- Because republishes push **watch notifications** to any local session watching
  the artifact, the board is a *push* trigger — no polling.

### State schema (`schema: 1`)

```json
{ "schema": 1, "nextId": 3, "cards": [ {
    "id": "AS-1", "title": "...", "body": "<spec>",
    "column": "backlog | needs_input | in_progress | review | done | archived",
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
| `review` | PR open, awaiting verification | orchestrator spawns the Tester on entry; APPROVE → `done`, REJECT → back to `in_progress` with findings |
| `done` | verified by the Tester; the user merges the PR (the one human gate in the loop — no role merges) | bundled into the next release (see Release flow) |
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
either.

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

The sweep touches nothing else. `done` cards are past their agent work;
`needs_input` cards already carry `agent: null` by protocol; and `archived`
cards are terminal with no automation — step 6 closes the trigger card's claim
when it archives it, so a lingering `agent: working` on an archived trigger
implies a crash mid-archival; either way `archived` is terminal, and the sweep
must leave it alone.

Republish the board once with all resets applied, then run the notification
steps below against the recovered state.

**On each republish notification:**

1. `Artifact action:"read"` the board; save the HTML to a local file; extract the
   state JSON block.
2. For every card with `column == "in_progress"` and `agent == null`:
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
   - `APPROVE` → `column: "done"`, `agent.status: "done"`, append the FINDINGS
     note as activity (`by: "tester"`). The PR itself now awaits the **user's**
     merge — merging is the pipeline's one human gate (the Engineer and Tester
     contracts both forbid it), and the Release flow's merged-and-ancestor
     check is the safety net that catches any Done card the user has not
     merged yet.
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
     would block every subsequent bundle. Republish.
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

**Writing cards as Claude (PM role):** read the board, edit the state JSON
(append a card, bump `nextId`, activity `by: "claude"`), republish with `url:`.

## Roles

- **PM** — interactive 1:1 sessions with the user; output is well-specified cards.
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
  the card to Done on APPROVE, or back to In Progress (findings appended to the
  spec, `agent: null`) on REJECT. Never fixes the Engineer's work, never pushes.
- **Release Manager** — `.claude/agents/release-manager.md`. Bundles the Done
  cards the orchestrator hands it into a tagged GitHub release: verifies each
  listed PR is merged to the `origin/main` HEAD it will tag, drafts grouped
  release notes (features / fixes / pipeline, plus the standing install section
  from `packaging/release-notes.md`), picks the next `v0.x.y` tag from the
  existing scheme, and publishes with `gh release create --target <verified
  sha>`. Bash/Read/Grep/Glob only — it writes no repo files (notes go straight
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
prompt: every card in `done` without a `released` field, each as its id, title,
PR link(s) from `links`, and the Engineer's SUMMARY from the activity log. A
release-manager trigger card is never a bundle candidate: the current run's
trigger sits in `in_progress`, and every previous run's trigger was stamped and
archived with its bundle (see post-release archival below) — so this rule only
ever collects work cards, each of which has a PR. An empty bundle is not
spawnable — tell the user there is nothing to release.

**What the agent does** (`.claude/agents/release-manager.md` is authoritative):
verifies each listed PR is `MERGED` and its merge commit is an ancestor of the
`origin/main` HEAD it records; drafts notes grouped features / fixes /
pipeline & tooling with the standing install section from
`packaging/release-notes.md` appended; picks the next tag by inspecting
`git tag` / `gh release list` (pre-1.0 semver — minor bump for any feature in
the bundle, patch for fix-only; `v0.1.0` if the repo had no version tags); and
publishes with `gh release create <tag> --target <verified sha>`. The tag push
triggers `.github/workflows/release.yaml`, whose idempotent create step reuses
the agent's release (notes survive) and attaches the platform binaries — the
release is public for a few minutes before its assets land, which is expected.
Any unmerged PR, tag collision, or empty bundle is a NEEDS_INPUT, never a
silent drop. The merged-and-ancestor check is the safety net for the
pipeline's one human gate: merging is the **user's** step (no role merges), so
a Done card whose PR the user has not merged yet is a normal straggler, not a
pipeline fault — the agent's NEEDS_INPUT naming it is precisely the prompt for
the user to merge and re-request the release.

**Post-release board archival — orchestrator, not agent.** On a `RELEASED`
report the orchestrator sets `released: <tag>` on every bundled card and moves
it to `column: "archived"` (off the Done column; the card and its history stay
in board state). A card-triggered run's trigger card gets the identical stamp —
`released: <tag>`, `column: "archived"` — plus the claim closeout
(`agent.status: "done"`, `agent.finished: <now>`; see step 6) — so it is never
left in `done` to leak into the next bundle, nor archived with a dangling
`working` claim. The `released` field is also the dedup guard for the
*next* bundle: only Done cards without it are release candidates.

## Known limits (v1)

- **One orchestrator at a time.** The startup-recovery argument — "every
  `working` claim found at startup is stale by definition" — is only true
  because a fresh orchestrator has spawned nothing yet *and no other session
  has either*. A second orchestrator starting while the first still has live
  workers would read those live claims as stale, sweep them to `agent: null`,
  and duplicate-spawn every card the first orchestrator is already working.
  There is no claim-ownership mechanism; the protocol simply assumes one
  orchestrator session exists at a time, and the user must not start a second
  while one is running.
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
