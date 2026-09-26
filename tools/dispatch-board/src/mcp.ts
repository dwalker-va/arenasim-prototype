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
import { Board, BoardError, COLUMNS, ROLES } from "./board.js";

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
  .describe("The card's own implementing PR. The review/human_review gate and the Tester spawn read this, not links.");
const worktree = z
  .string()
  .nullable()
  .describe("Absolute path of the worktree the card's branch is checked out in (the Engineer's tree). Informational: recorded, never checked.");
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
        "Card SUMMARIES (id, title, column, role, priority, agent, pr, worktree, links, updated, released, version) — never bodies or activity. " +
        "Archived cards are excluded unless you ask for column 'archived' or include_archived. `fields` picks other doc fields (e.g. [\"question\"]); [\"*\"] returns every field except activity.",
      inputSchema: {
        column: z.union([column, z.array(column)]).optional(),
        role: z.enum(ROLES).optional(),
        fields: z.array(z.string()).optional(),
        include_archived: z.boolean().optional(),
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
      description: "File a new card. The id is allocated server-side. Defaults: column backlog, priority P2.",
      inputSchema: {
        title: z.string().min(1),
        body: z.string().optional(),
        role: z.enum(ROLES),
        priority: z.string().regex(/^P[1-4]$/).optional(),
        column: column.optional(),
        links: z.array(link).optional(),
        pr: pr.optional(),
        worktree: worktree.optional(),
        actor,
        by,
      },
    },
    async (a) => run(() => board.createCard(a, { actor: a.actor, by: a.by })),
  );

  server.registerTool(
    "update_card",
    {
      description:
        "Patch card fields (title, body, role, priority, links, pr, worktree, question, agent, released) under optimistic concurrency. Column changes go through move_card. `agent` may be null (clear the claim) or {status: done} (close out a WORKING claim) — never working: claims come only from claim_card. Optionally append an activity line in the same write.",
      inputSchema: { id, patch, expected_version: expectedVersion, activity: z.string().optional(), actor, by },
    },
    async (a) => run(() => board.updateCard(a.id, a.patch, a.expected_version, { actor: a.actor, by: a.by, activity: a.activity })),
  );

  server.registerTool(
    "move_card",
    {
      description:
        "Move a card to a column, atomically with an optional patch, body append and activity line — all one write. Enforces the column rules: a non-pm card needs its own PR (`pr`, already set or in this move's patch; `links` are references and never count) to enter review or human_review; entering in_progress sets agent: null. `append` adds `## <heading>` + text to the body in the same write (a Tester REJECT: findings + the move back to in_progress).",
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
        "Board events (created, moved, edited, answered, claimed, claim_released, claim_finished, activity, body_appended, deleted) after `cursor`, oldest first. Returns the next cursor to pass. ignore_actors drops your own writes. cursor 0 = from the beginning; `head` gives the current cursor without events. A cursor past `head` is refused (cursor_ahead): it belongs to a re-created board — re-read the board and resume from head.",
      inputSchema: {
        cursor: z.number().int().min(0),
        ignore_actors: z.array(z.string()).optional(),
        limit: z.number().int().min(1).max(1000).optional(),
      },
      annotations: { readOnlyHint: true },
    },
    async (a) => run(() => ({ ...board.eventsSince(a.cursor, a), head: board.head(), board: board.boardId() })),
  );

  return server;
}
