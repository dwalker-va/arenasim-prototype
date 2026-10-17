// AS-339: a card write over MCP returns a receipt, not the card. The standing
// cards' bodies run to tens of thousands of tokens and the orchestrator writes
// to them several times a day, so echoing the body was its largest cost.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnDaemon, mcpClient, call, RECEIPT, receiptOf } from "./helpers.mjs";

const o = { actor: "orchestrator" };
const PR = { url: "https://github.com/o/r/pull/9" };
const BODY = "A standing card's spec. ".repeat(400);

/**
 * Every card write the MCP serves, in an order one card can take them, with
 * what each adds to the receipt. `full` is passed to every one.
 */
async function walk(client, full) {
  const f = full ? { full: true } : {};
  const out = [];
  const step = async (tool, args, extra = []) => {
    const r = await call(client, tool, { ...args, ...f, ...o });
    assert.ok(r.ok, `${tool}: ${JSON.stringify(r.value).slice(0, 300)}`);
    out.push({ tool, value: r.value, extra });
    return r.value;
  };
  let c = await step("create_card", { title: "standing", body: BODY, role: "engineer" });
  const id = c.id;
  c = await step("update_card", { id, patch: { priority: "P1" }, expected_version: c.version });
  c = await step("append_to_body", { id, heading: "Tester findings", text: "1. one" });
  c = await step("append_activity", { id, msg: "a note" });
  c = await step("move_card", { id, column: "needs_input", expected_version: c.version, patch: { question: { text: "which way?" } } });
  c = await step("answer_question", { id, answer: "left", expected_version: c.version }, ["question"]);
  c = await step("claim_card", { id, name: "Engineer-A", worktree: "/abs/tree" }, ["worktree"]);
  c = await step("finish_claim", { id, name: "Engineer-A" });
  c = await step("move_card", { id, column: "review", expected_version: c.version, patch: { pr: PR } });
  c = await step("claim_card", { id, name: `${id}-test` });
  c = await step("release_claim", { id });
  c = await step("claim_card", { id, name: `${id}-test` });
  c = await step("record_ruling", { id, text: "ship it", numbers: { z: 3 } }, ["ruling"]);
  c = await step("mark_merged", { id, pr: PR, merge_sha: "a".repeat(40), expected_version: c.version }, ["pr", "merge_sha", "merged_at"]);
  const other = await step("create_card", { title: "dropped", body: BODY, role: "engineer" });
  await step("cancel_card", { id: other.id, reason: "superseded", expected_version: other.version }, ["cancelled"]);
  return { out, id, other: other.id };
}

test("receipt: every card write returns {id, version, column, agent, updated} plus what it produced — never the body", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const { out, id, other } = await walk(client, false);
  // The writes this pins, by name: a write added without a receipt fails here.
  assert.deepEqual([...new Set(out.map((s) => s.tool))].sort(), [
    "answer_question", "append_activity", "append_to_body", "cancel_card", "claim_card", "create_card", "finish_claim",
    "mark_merged", "move_card", "record_ruling", "release_claim", "update_card",
  ]);
  for (const { tool, value, extra } of out) {
    assert.deepEqual(Object.keys(value).sort(), [...RECEIPT, ...extra].sort(), `${tool} returned ${Object.keys(value).join(", ")}`);
    assert.equal(value.body, undefined, `${tool} echoed the body`);
    assert.equal(value.activity, undefined, `${tool} echoed the activity log`);
  }
  // The last write's receipt is the card as it now stands, and its extras are what the write recorded.
  const merged = (await call(client, "get_card", { id })).value;
  const m = out.find((s) => s.tool === "mark_merged").value;
  assert.deepEqual(m, { ...receiptOf(merged), pr: merged.pr, merge_sha: "a".repeat(40), merged_at: merged.merged_at });
  assert.deepEqual(out.find((s) => s.tool === "record_ruling").value.ruling, merged.rulings.at(-1));
  assert.deepEqual(out.find((s) => s.tool === "answer_question").value.question, { text: "which way?", answer: "left" });
  assert.equal(out.find((s) => s.tool === "claim_card").value.worktree, "/abs/tree");
  const cancelled = (await call(client, "get_card", { id: other })).value;
  assert.deepEqual(out.at(-1).value, { ...receiptOf(cancelled), cancelled: cancelled.cancelled });
  // Receipts chain: each versioned write above used the version the previous receipt gave.
  assert.equal(out.find((s) => s.tool === "update_card").value.version, 2);
});

test("receipt: full: true returns today's whole card from every write", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const { out } = await walk(client, true);
  for (const { tool, value } of out) {
    assert.match(value.body, /^A standing card's spec\./, `${tool} under full: true lost the body`);
    assert.ok(Array.isArray(value.activity), `${tool} under full: true lost the activity`);
    assert.equal(typeof value.title, "string", tool);
  }
});

test("receipt: a refusal carries the card's receipt as it is now, not its body; full: true carries the card", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  let c = (await call(client, "create_card", { title: "r", body: BODY, role: "engineer", ...o })).value;
  c = (await call(client, "move_card", { id: c.id, column: "in_progress", expected_version: c.version, ...o })).value;
  c = (await call(client, "claim_card", { id: c.id, name: "Engineer-A", ...o })).value;
  const now = receiptOf((await call(client, "get_card", { id: c.id })).value);
  const refusals = {
    stale_version: await call(client, "update_card", { id: c.id, patch: { priority: "P3" }, expected_version: c.version - 1, ...o }),
    claim_refused: await call(client, "claim_card", { id: c.id, name: "Engineer-B", ...o }),
    gate_refused: await call(client, "move_card", { id: c.id, column: "review", expected_version: c.version, ...o }),
  };
  for (const [code, r] of Object.entries(refusals)) {
    assert.equal(r.ok, false, code);
    assert.equal(r.value.error, code);
    assert.ok(r.value.message, code);
    assert.deepEqual(r.value.card, now, `${code} carried more (or less) than the receipt`);
  }
  const whole = await call(client, "claim_card", { id: c.id, name: "Engineer-B", full: true, ...o });
  assert.equal(whole.value.error, "claim_refused");
  assert.equal(whole.value.card.body, BODY);
  assert.equal(whole.value.card.agent.name, "Engineer-A");
});

test("get_card: fields reads version and activity without the body", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  let c = (await call(client, "create_card", { title: "f", body: BODY, role: "engineer", ...o })).value;
  await call(client, "append_activity", { id: c.id, msg: "first", ...o });
  c = (await call(client, "append_activity", { id: c.id, msg: "latest", ...o })).value;

  const log = (await call(client, "get_card", { id: c.id, fields: ["activity"], activity_limit: 1 })).value;
  assert.deepEqual(Object.keys(log).sort(), ["activity", "activity_total", "id", "version"]);
  assert.deepEqual([log.id, log.version, log.activity.map((a) => a.msg), log.activity_total], [c.id, c.version, ["latest"], 3]);

  const some = (await call(client, "get_card", { id: c.id, fields: ["title", "column", "not_a_field"] })).value;
  assert.deepEqual(some, { id: c.id, title: "f", column: "backlog", version: c.version });

  // Without fields, the whole card, as before.
  const whole = (await call(client, "get_card", { id: c.id })).value;
  assert.equal(whole.body, BODY);
  assert.equal(whole.activity.length, 3);
});
