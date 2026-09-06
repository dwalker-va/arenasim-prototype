# Agent Pipeline — ArenaSim Dispatch

Status: **v1 shipped 2026-09-05** (board + orchestrator protocol + Engineer role);
**Tester stage shipped** (card AS-1). The Release Manager stage is card AS-2 on the
board itself.

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
    "column": "backlog | needs_input | in_progress | review | done",
    "role": "engineer | tester | release-manager | pm",
    "priority": "P1 | P2 | P3",
    "links": [{"label": "PR #112", "url": "..."}],
    "question": {"text": "...", "answer": "..."} | null,
    "agent": {"status": "working | done", "started": "ISO", "finished": "ISO"} | null,
    "activity": [{"t": "ISO", "by": "board | claude | orchestrator | engineer | tester", "msg": "..."}],
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
| `done` | verified, merged/mergeable | AS-2 adds Release Manager bundling |

**The `agent` field is the dedup guard.** Any gesture that moves a card *into*
`in_progress` (drag, edit, question answered) sets `agent: null`, which means
"needs a spawn". The orchestrator sets `agent: {status: "working", ...}` **before**
spawning, so overlapping notifications never double-spawn. Bouncing a card from
Review back to In Progress is therefore automatically a respawn. The same guard
covers the Review column: the Engineer's completion leaves `agent.status: "done"`,
which in `review` means "needs a Tester spawn"; the orchestrator flips it back to
`working` before spawning the Tester, so overlapping notifications never
double-spawn there either.

## The orchestrator

Any long-running interactive Claude Code session on this machine. It is the only
thing that spawns worker sessions; the board page never does.

**Start one:** open a session at the repo (or a worktree), then
`Artifact action:"watch" url:<board url>`. A session that published or read the
board recently usually gets its watch restored on `--resume`.

**On each republish notification:**

1. `Artifact action:"read"` the board; save the HTML to a local file; extract the
   state JSON block.
2. For every card with `column == "in_progress"` and `agent == null`:
   a. Set `agent: {status: "working", started: <now>}`, append an activity entry
      (`by: "orchestrator"`), and **republish the board first** (edit the state
      line in the saved HTML, publish that file with `url:` the board URL; on a
      publish conflict, re-read and re-apply).
   b. Spawn the card's role agent via the Agent tool —
      `subagent_type: "engineer"` (fall back to `general-purpose` carrying the
      role prompt from `.claude/agents/engineer.md` if the definition isn't
      loaded in this session), `isolation: "worktree"`, `name: <card id>`,
      prompt = card id + title + full spec + any prior findings/answers from the
      activity log and `question.answer`.
3. For every card with `column == "review"`, `agent.status == "done"`, and an
   open-PR link in `links`:
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
   - `FAILED` → `column: "needs_input"` with the failure as the question text.
5. On a Tester's completion notification, parse its `VERDICT:` report and
   republish the board accordingly:
   - `APPROVE` → `column: "done"`, `agent.status: "done"`, append the FINDINGS
     note as activity (`by: "tester"`).
   - `REJECT` → `column: "in_progress"`, append the FINDINGS **verbatim** to the
     card `body` under a dated `## Tester findings` heading (they are the next
     Engineer's spec addendum), set `agent: null` (step 2 then spawns a fresh
     Engineer, who receives the findings as part of the spec), activity entry.
   - A malformed report (no parseable `VERDICT:`) → `column: "needs_input"` with
     the raw report as the question text; never guess a verdict.
6. Cards the *user* must see promptly (needs_input) warrant a mention in the
   orchestrator session's next visible message.

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
- **Release Manager** (AS-2, not yet built) — bundles Done cards into a tagged
  release with notes.

## Known limits (v1)

- Workers are in-process subagents: they die if the orchestrator session dies
  (their worktree changes survive; the card stays `agent: working` — reset
  `agent: null` to respawn).
- Watches are session-local; if no orchestrator session is open, drags simply
  queue up as board state until one connects and reads the board.
- Board writes are last-writer-wins with conflict-reload; fine for one human +
  a couple of sessions, not for a team.
