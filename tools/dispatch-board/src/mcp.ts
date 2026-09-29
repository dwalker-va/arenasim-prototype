/**
 * The MCP surface: one tool per board operation, each a thin call into Board.
 *
 * Every WRITE takes `actor` — the writing session's tag. It is recorded on
 * the event the write produces, which is what lets a session's `wait` ignore
 * its own writes; "orchestrator" is reserved for the orchestrator, whose wait
 * ignores it (the tag is caller-supplied: trusted localhost, not auth). `by` (optional)
 * is the activity-log author and defaults to the actor.
 *
 * Refusals (stale version, failed claim, PR-link gate) come back as tool
 * errors whose JSON carries `error`, `message` and the card as it is now.
 */
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { AREAS, Board, BoardError, COLUMNS, ROLES, SWEEP_STATUSES } from "./board.js";

const actor = z
  .string()
  .min(1)
  .describe('Your session/actor tag, recorded on the event this write makes. "orchestrator" is the orchestrator\'s alone — its wait ignores that tag, so no other session may use it. A `wait --ignore-actor <tag>` skips events carrying it.');
const by = z.string().min(1).optional().describe("Activity-log author (e.g. orchestrator, tester, pm). Defaults to actor.");
const id = z.string().min(1).describe("Card id, e.g. AS-7");
const expectedVersion = z
  .number()
  .int()
  .describe("The card version you read. A stale version is REFUSED with the current card; re-read and re-apply.");
const column = z.enum(COLUMNS);
const link = z.object({ label: z.string(), url: z.string() }).describe("A reference (a prerequisite PR, where a finding was made, a workshop page) — never the card's own PR");
const pr = z
  .object({ url: z.string().describe("The card's OWN pull request, https://.../pull/<n>"), number: z.number().int().optional() })
  .nullable()
  .describe("The card's own implementing PR. The review/merged gate and the Tester spawn read this, not links.");
const worktree = z
  .string()
  .nullable()
  .describe("Absolute path of the worktree the card's branch is checked out in (the Engineer's tree). Informational: recorded, never checked.");
const milestoneName = z.string().min(1).describe('A milestone name, e.g. "0.7"');
/** The structured fields the milestone review page is assembled from. */
const milestoneFields = {
  milestone: milestoneName.nullable().describe("The milestone this card belongs to (it must exist and not be released); null for none"),
  iteration: z.number().int().min(1).nullable().describe("1 for the milestone's planned cards, 2+ for review feedback rounds; defaults to 1 when a milestone is set"),
  area: z.enum(AREAS).nullable().describe("What the card changed, for the review's WHAT CHANGED grouping"),
  summary: z.string().nullable().describe("A plain-language, player-facing-ish paragraph: what changed"),
  human_testing: z
    .string()
    .nullable()
    .describe("The PR's human-testing steps, one per line (the review checklist is built from these lines); \"Nothing needs human testing\" when so"),
  sweep: z
    .object({ status: z.enum(SWEEP_STATUSES), summary: z.string().optional().describe("One line: the result, or why none") })
    .strict()
    .nullable()
    .describe("Where the card's balance sweep happened"),
  gaps: z.string().nullable().describe("Stated gaps: what the card knowingly does not cover"),
};
const patch = z
  .object({
    title: z.string().min(1),
    body: z.string(),
    role: z.enum(ROLES),
    priority: z.string().regex(/^P[1-4]$/),
    links: z.array(link),
    pr,
    worktree,
    question: z.object({ text: z.string(), answer: z.string().optional() }).nullable(),
    agent: z
      .object({
        status: z.literal("done").describe("Close out the card's working claim. A claim is only ever TAKEN with claim_card."),
        started: z.string().optional(),
        finished: z.string().optional(),
        name: z.string().optional(),
      })
      .nullable(),
    released: z.string().min(1),
    ...milestoneFields,
  })
  .partial()
  .strict();

function ok(value: unknown) {
  return { content: [{ type: "text" as const, text: JSON.stringify(value, null, 1) }] };
}

function run(fn: () => unknown) {
  try {
    return ok(fn());
  } catch (e) {
    if (e instanceof BoardError) {
      return { isError: true, content: [{ type: "text" as const, text: JSON.stringify(e.toJSON(), null, 1) }] };
    }
    throw e;
  }
}

export function buildMcpServer(board: Board): McpServer {
  const server = new McpServer(
    { name: "dispatch-board", version: "1.0.0" },
    {
      instructions:
        "ArenaSim Dispatch board (docs/design/agent-pipeline.md). list_cards returns SUMMARIES; call get_card for a body. " +
        "Every write needs `actor`; versioned writes need the `version` you read and are refused if it is stale. " +
        "claim_card is compare-and-set: if it fails, someone else holds the card — do not spawn.",
    },
  );

  server.registerTool(
    "list_cards",
    {
      description:
        "Card SUMMARIES (id, title, column, role, priority, agent, pr, worktree, links, updated, released, milestone, iteration, area, version) — never bodies or activity. " +
        "Archived cards are excluded unless you ask for column 'archived' or include_archived. `fields` picks other doc fields (e.g. [\"question\"]); [\"*\"] returns every field except activity. " +
        "`milestone` filters to one milestone's cards (null: cards on none); `iteration` to one feedback round.",
      inputSchema: {
        column: z.union([column, z.array(column)]).optional(),
        role: z.enum(ROLES).optional(),
        fields: z.array(z.string()).optional(),
        include_archived: z.boolean().optional(),
        milestone: milestoneName.nullable().optional(),
        iteration: z.number().int().min(1).optional(),
      },
      annotations: { readOnlyHint: true },
    },
    async (a) => run(() => board.listCards(a)),
  );

  server.registerTool(
    "get_card",
    {
      description: "One full card: body, question, links, agent, version, and activity (the last `activity_limit` entries if given; activity_total says how many exist).",
      inputSchema: { id, activity_limit: z.number().int().min(0).optional() },
      annotations: { readOnlyHint: true },
    },
    async (a) => run(() => board.getCard(a.id, { activity_limit: a.activity_limit })),
  );

  server.registerTool(
    "create_card",
    {
      description:
        "File a new card. The id is allocated server-side. Defaults: column backlog, priority P2. The milestone fields (milestone, iteration, area, summary, human_testing, sweep, gaps) are optional; a card given a milestone and no iteration is iteration 1.",
      inputSchema: {
        title: z.string().min(1),
        body: z.string().optional(),
        role: z.enum(ROLES),
        priority: z.string().regex(/^P[1-4]$/).optional(),
        column: column.optional(),
        links: z.array(link).optional(),
        pr: pr.optional(),
        worktree: worktree.optional(),
        ...Object.fromEntries(Object.entries(milestoneFields).map(([k, v]) => [k, v.optional()])),
        actor,
        by,
      },
    },
    async (a) => run(() => board.createCard(a as Parameters<Board["createCard"]>[0], { actor: a.actor, by: a.by })),
  );

  server.registerTool(
    "update_card",
    {
      description:
        "Patch card fields (title, body, role, priority, links, pr, worktree, question, agent, released, and the milestone fields: milestone, iteration, area, summary, human_testing, sweep, gaps) under optimistic concurrency. Rulings are appended with record_ruling, never patched. Column changes go through move_card. `agent` may be null (clear the claim) or {status: done} (close out a WORKING claim) — never working: claims come only from claim_card. Optionally append an activity line in the same write.",
      inputSchema: { id, patch, expected_version: expectedVersion, activity: z.string().optional(), actor, by },
    },
    async (a) => run(() => board.updateCard(a.id, a.patch, a.expected_version, { actor: a.actor, by: a.by, activity: a.activity })),
  );

  server.registerTool(
    "move_card",
    {
      description:
        "Move a card to a column, atomically with an optional patch, body append and activity line — all one write. Enforces the column rules: a non-pm card needs its own PR (`pr`, already set or in this move's patch; `links` are references and never count) to enter review or merged (use mark_merged to record a merge); entering in_progress sets agent: null. `append` adds `## <heading>` + text to the body in the same write (a Tester REJECT: findings + the move back to in_progress).",
      inputSchema: {
        id,
        column,
        expected_version: expectedVersion,
        patch: patch.optional(),
        append: z.object({ heading: z.string().min(1), text: z.string().min(1) }).optional(),
        activity: z.string().optional(),
        actor,
        by,
      },
    },
    async (a) =>
      run(() =>
        board.moveCard(a.id, a.column, a.expected_version, { actor: a.actor, by: a.by, activity: a.activity, patch: a.patch, append: a.append }),
      ),
  );

  server.registerTool(
    "claim_card",
    {
      description:
        "Compare-and-set claim BEFORE spawning: sets agent {status: working, started, name} only if the card is claimable right now (in_progress with agent null; review with agent null or status done; never a pm card). A second claim FAILS — if this fails, do not spawn. `worktree` records the spawned Engineer's tree (omit it for a Tester, whose tree is not the card's branch).",
      inputSchema: {
        id,
        name: z.string().min(1).describe("The agent being spawned, e.g. Engineer-AS-7"),
        worktree: z.string().optional().describe("Absolute path of the worktree the Engineer will work in, if known"),
        activity: z.string().optional(),
        actor,
        by,
      },
    },
    async (a) => run(() => board.claimCard(a.id, a.name, { actor: a.actor, by: a.by, activity: a.activity, worktree: a.worktree })),
  );

  const claimClose = {
    id,
    name: z.string().optional().describe("If given, the claim must belong to this agent"),
    expected_version: expectedVersion.optional(),
    activity: z.string().optional(),
    actor,
    by,
  };
  server.registerTool(
    "release_claim",
    {
      description: "working claim -> agent: null (startup recovery, the live-claim audit, or a dropped spawn). Refused when there is no working claim.",
      inputSchema: claimClose,
    },
    async (a) => run(() => board.releaseClaim(a.id, a)),
  );
  server.registerTool(
    "finish_claim",
    {
      description: "working claim -> {status: done, finished: now}. Use with move_card for the Engineer/Tester hand-offs, and for a release trigger card's closeout.",
      inputSchema: claimClose,
    },
    async (a) => run(() => board.finishClaim(a.id, a)),
  );

  server.registerTool(
    "append_activity",
    {
      description: "Append one activity entry {t: now, by, msg}. Activity is append-only.",
      inputSchema: { id, msg: z.string().min(1), actor, by },
    },
    async (a) => run(() => board.appendActivity(a.id, a.msg, { actor: a.actor, by: a.by })),
  );

  server.registerTool(
    "append_to_body",
    {
      description: "Append text to the card body under `## <heading>` — Tester and User findings (put the date in the heading).",
      inputSchema: { id, heading: z.string().min(1), text: z.string().min(1), activity: z.string().optional(), actor, by },
    },
    async (a) => run(() => board.appendToBody(a.id, a.heading, a.text, { actor: a.actor, by: a.by, activity: a.activity })),
  );

  server.registerTool(
    "answer_question",
    {
      description: "Answer a card's pending question: records the answer, moves the card to in_progress with agent: null (the board's Answer & resume).",
      inputSchema: { id, answer: z.string().min(1), expected_version: expectedVersion, actor, by },
    },
    async (a) => run(() => board.answerQuestion(a.id, a.answer, a.expected_version, { actor: a.actor, by: a.by })),
  );

  server.registerTool(
    "events_since",
    {
      description:
        "Board events (created, moved, edited, answered, claimed, claim_released, claim_finished, activity, body_appended, deleted, ruling, merged, milestone_created, milestone_updated, milestone_sweep, milestone_closed, review_submitted, migrated) after `cursor`, oldest first. Returns the next cursor to pass. ignore_actors drops your own writes. cursor 0 = from the beginning; `head` gives the current cursor without events. A cursor past `head` is refused (cursor_ahead): it belongs to a re-created board — re-read the board and resume from head.",
      inputSchema: {
        cursor: z.number().int().min(0),
        ignore_actors: z.array(z.string()).optional(),
        limit: z.number().int().min(1).max(1000).optional(),
      },
      annotations: { readOnlyHint: true },
    },
    async (a) => run(() => ({ ...board.eventsSince(a.cursor, a), head: board.head(), board: board.boardId() })),
  );

  // ---------------------------------------------------------------- milestones

  server.registerTool(
    "record_ruling",
    {
      description:
        "Record a user decision on a card: dated, append-only, with the numbers it turned on (e.g. {\"win rate\": \"+36pt\", \"z\": 5.2}). The milestone review lists every ruling of its cards. expected_version is optional (an append).",
      inputSchema: {
        id,
        text: z.string().min(1).describe("The decision, in the user's terms"),
        numbers: z.record(z.union([z.number(), z.string()])).optional(),
        expected_version: expectedVersion.optional(),
        actor,
        by,
      },
    },
    async (a) => run(() => board.recordRuling(a.id, { text: a.text, numbers: a.numbers }, { actor: a.actor, by: a.by, expected_version: a.expected_version })),
  );

  server.registerTool(
    "mark_merged",
    {
      description:
        "After a Tester APPROVE, once YOU have merged the card's PR: review -> merged, recording pr and its merge commit and closing out the working (Tester) claim — one write. Also records the merge on a merged card that has none (a card migrated from human_review). Refused from any other column, or for a PR that is not the card's own.",
      inputSchema: {
        id,
        pr: z.object({ url: z.string(), number: z.number().int().optional() }).describe("The card's own PR, as merged"),
        merge_sha: z.string().describe("The merge commit on main (gh pr view --json mergeCommit)"),
        expected_version: expectedVersion,
        activity: z.string().optional().describe("e.g. the Tester's FINDINGS note"),
        actor,
        by,
      },
    },
    async (a) => run(() => board.markMerged(a.id, { pr: a.pr, merge_sha: a.merge_sha }, a.expected_version, { actor: a.actor, by: a.by, activity: a.activity })),
  );

  const milestoneVersion = z.number().int().describe("The milestone version you read. A stale version is REFUSED with the current milestone.");

  server.registerTool(
    "create_milestone",
    {
      description: "Create a milestone (status open). Cards join it with create_card/update_card `milestone`.",
      inputSchema: { name: milestoneName, actor },
    },
    async (a) => run(() => board.createMilestone(a.name, { actor: a.actor })),
  );

  server.registerTool(
    "list_milestones",
    {
      description: "Every milestone: name, status, created, released_at, tag, version, and its card count per column.",
      inputSchema: {},
      annotations: { readOnlyHint: true },
    },
    async () => run(() => board.listMilestones()),
  );

  server.registerTool(
    "get_milestone",
    {
      description:
        "The milestone review payload, as the review page shows it: the milestone; what_changed (finished cards by area, with summary, pr, merge_sha); in_flight; checklist (every finished card's human-testing steps, ticks, and the SHA/tag it applies to); decisions (every ruling); balance (the milestone sweep, deferred and on-card sweeps); gaps (stated gaps, and cards filed while it was open); feedback (drafts and past submissions).",
      inputSchema: { name: milestoneName },
      annotations: { readOnlyHint: true },
    },
    async (a) => run(() => board.getMilestone(a.name)),
  );

  server.registerTool(
    "update_milestone",
    {
      description:
        "Patch a milestone: status (open | in_review — released is close_milestone's, and final), tag, release_url, review_sha (the main SHA the review checklist applies to).",
      inputSchema: {
        name: milestoneName,
        patch: z
          .object({
            status: z.enum(["open", "in_review"]),
            tag: z.string().min(1).nullable(),
            release_url: z.string().nullable(),
            review_sha: z.string().nullable(),
          })
          .partial()
          .strict(),
        expected_version: milestoneVersion,
        actor,
      },
    },
    async (a) => run(() => board.updateMilestone(a.name, a.patch, a.expected_version, { actor: a.actor })),
  );

  server.registerTool(
    "set_milestone_sweep",
    {
      description: "Record the milestone sweep's result on the milestone: a one-line summary and a link to the committed doc/CSV (a URL or repo path).",
      inputSchema: { name: milestoneName, summary: z.string().min(1), link: z.string().min(1).optional(), expected_version: milestoneVersion, actor, by },
    },
    async (a) => run(() => board.setMilestoneSweep(a.name, { summary: a.summary, link: a.link }, a.expected_version, { actor: a.actor, by: a.by })),
  );

  server.registerTool(
    "close_milestone",
    {
      description:
        "After the user approves the milestone: every merged card on it moves to done and the milestone becomes released — one write. Refused while any of its cards is unfinished (not merged/done/archived). Returns the done cards (id, title, pr, merge_sha, summary, area, iteration) for the release bundle and notes.",
      inputSchema: {
        name: milestoneName,
        expected_version: milestoneVersion,
        tag: z.string().min(1).optional(),
        release_url: z.string().optional(),
        actor,
        by,
      },
    },
    async (a) => run(() => board.closeMilestone(a.name, a.expected_version, { actor: a.actor, by: a.by, tag: a.tag, release_url: a.release_url })),
  );

  return server;
}
