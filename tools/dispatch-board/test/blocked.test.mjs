// First-class dependencies: a card names the cards it waits on (`blocked_by`),
// reads which are still open (`waiting_on`), and the board emits `unblocked`
// when the last one merges — so a dependency lives on the board, and the
// orchestrator's wait wakes on its release.
import { test } from "node:test";
import assert from "node:assert/strict";
import { BoardError, RULE_ACTOR } from "../dist/board.js";
import { tempBoard, seed, spawnDaemon, mcpClient, call } from "./helpers.mjs";

const PR = (n) => ({ url: `https://github.com/o/r/pull/${n}` });
let prs = 100;

function refused(fn, code, match) {
  let err;
  try {
    fn();
  } catch (e) {
    err = e;
  }
  assert.ok(err instanceof BoardError, `expected a BoardError(${code}), got ${err === undefined ? "success" : err}`);
  assert.equal(err.code, code, err.message);
  if (match) assert.match(err.message, match);
  return err;
}

/** A card in review under a Tester claim, ready for mark_merged. */
function reviewCard(board) {
  return seed(board, { column: "review", pr: PR(++prs), agent: "working" });
}
function merge(board, c, actor = "orchestrator") {
  return board.markMerged(c.id, { pr: c.pr, merge_sha: "b".repeat(40) }, board.getCard(c.id).version, { actor });
}
const unblocks = (board, since) => board.eventsSince(since).events.filter((e) => e.kind === "unblocked");

test("blocked_by: names live cards only, never itself or one twice, and closes no cycle", (t) => {
  const { board } = tempBoard(t);
  const a = seed(board);
  const b = seed(board);
  refused(() => board.createCard({ title: "x", role: "engineer", blocked_by: ["AS-999"] }, { actor: "o" }), "invalid", /AS-999, which is not a card/);
  refused(() => board.updateCard(a.id, { blocked_by: [a.id] }, a.version, { actor: "o" }), "invalid", /blocked by itself/);
  refused(() => board.updateCard(a.id, { blocked_by: [b.id, b.id] }, a.version, { actor: "o" }), "invalid", /twice/);
  refused(() => board.updateCard(a.id, { blocked_by: "AS-2" }, a.version, { actor: "o" }), "invalid", /array of card ids/);

  const a2 = board.updateCard(a.id, { blocked_by: [b.id] }, a.version, { actor: "o" });
  // b -> a would close a cycle (a waits on b); so would c -> a with b -> c.
  refused(() => board.updateCard(b.id, { blocked_by: [a.id] }, b.version, { actor: "o" }), "invalid", /cycle/);
  const c = board.createCard({ title: "c", role: "engineer", blocked_by: [a.id] }, { actor: "o" });
  refused(() => board.moveCard(b.id, "in_progress", b.version, { actor: "o", patch: { blocked_by: [c.id] } }), "invalid", /cycle/);
  // A refused write changed nothing.
  assert.equal(board.getCard(b.id).blocked_by, undefined);
  assert.equal(board.getCard(b.id).column, "backlog");
  // And [] clears it.
  assert.equal(board.updateCard(a.id, { blocked_by: [] }, a2.version, { actor: "o" }).waiting_on, undefined);
});

test("blocked_by: waiting_on lists the blockers not yet merged; a cancelled blocker keeps waiting", (t) => {
  const { board } = tempBoard(t);
  const x = reviewCard(board);
  const y = reviewCard(board);
  const z = seed(board);
  const plain = seed(board);
  const w = board.createCard({ title: "waits", role: "engineer", blocked_by: [x.id, y.id, z.id] }, { actor: "o" });
  assert.deepEqual(board.getCard(w.id).waiting_on, [x.id, y.id, z.id]);
  merge(board, x);
  board.cancelCard(z.id, "dropped", z.version, { actor: "o" });
  assert.deepEqual(board.getCard(w.id).waiting_on, [y.id, z.id], "a merged blocker still listed, or a cancelled one dropped");
  const summary = board.listCards().find((s) => s.id === w.id);
  assert.deepEqual([summary.blocked_by, summary.waiting_on], [[x.id, y.id, z.id], [y.id, z.id]]);
  assert.deepEqual(board.getCard(w.id, { fields: ["waiting_on"] }).waiting_on, [y.id, z.id]);
  // A card with no blockers carries neither field: the existing cards read exactly as before.
  const p = board.listCards().find((s) => s.id === plain.id);
  assert.ok(!("blocked_by" in p) && !("waiting_on" in p) && !("waiting_on" in board.getCard(plain.id)));
});

test("unblocked: the LAST blocker's merge emits one event on the waiting card, by the board's own actor", (t) => {
  const { board } = tempBoard(t);
  const x = reviewCard(board);
  const y = reviewCard(board);
  const w = board.createCard({ title: "waits", role: "engineer", blocked_by: [x.id, y.id] }, { actor: "o" });
  const also = board.createCard({ title: "also waits on y", role: "engineer", blocked_by: [y.id] }, { actor: "o" });
  let head = board.head();
  merge(board, x);
  assert.deepEqual(unblocks(board, head), [], "unblocked while another blocker is open");
  head = board.head();
  merge(board, y);
  const evs = board.eventsSince(head).events;
  assert.deepEqual(evs.map((e) => [e.kind, e.card]), [["merged", y.id], ["unblocked", w.id], ["unblocked", also.id]], "the merge, then each card it unblocked");
  for (const e of evs.slice(1)) assert.equal(e.actor, RULE_ACTOR);
  assert.deepEqual(evs[1].data, { blocked_by: [x.id, y.id], last: y.id });
  assert.deepEqual(board.getCard(w.id).waiting_on, []);
  // Later moves of a merged blocker (merged -> done) unblock nothing again.
  head = board.head();
  board.moveCard(y.id, "done", board.getCard(y.id).version, { actor: "o" });
  assert.deepEqual(unblocks(board, head), []);
});

test("unblocked: a pm blocker finishing in done counts; a waiting card that is itself finished gets no event", (t) => {
  const { board } = tempBoard(t);
  const pm = seed(board, { role: "pm" });
  const x = reviewCard(board);
  const live = board.createCard({ title: "live", role: "engineer", blocked_by: [pm.id] }, { actor: "o" });
  const shipped = board.createCard({ title: "already done", role: "pm", blocked_by: [x.id] }, { actor: "o" });
  board.moveCard(shipped.id, "done", shipped.version, { actor: "o" });
  let head = board.head();
  board.moveCard(pm.id, "done", pm.version, { actor: "o" });
  assert.deepEqual(unblocks(board, head).map((e) => e.card), [live.id]);
  head = board.head();
  merge(board, x);
  assert.deepEqual(unblocks(board, head), [], "a finished card was announced as unblocked");
});

test("MCP: the orchestrator's own mark_merged of the last blocker wakes its own wait", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const o = { actor: "orchestrator" };
  let x = (await call(client, "create_card", { title: "blocker", role: "engineer", column: "review", pr: PR(1), ...o })).value;
  x = (await call(client, "claim_card", { id: x.id, name: `${x.id}-test`, ...o })).value;
  const w = (await call(client, "create_card", { title: "waits", role: "engineer", blocked_by: [x.id], ...o })).value;
  const listed = (await call(client, "list_cards", {})).value.find((s) => s.id === w.id);
  assert.deepEqual([listed.blocked_by, listed.waiting_on], [[x.id], [x.id]]);

  const head = (await call(client, "events_since", { cursor: 0, limit: 1000 })).value.head;
  const m = await call(client, "mark_merged", { id: x.id, pr: PR(1), merge_sha: "c".repeat(40), expected_version: x.version, ...o });
  assert.ok(m.ok, JSON.stringify(m.value));
  // What `wait --ignore-actor orchestrator` sees: the merge is the orchestrator's own and filtered; the unblock is not.
  const seen = (await call(client, "events_since", { cursor: head, ignore_actors: ["orchestrator"] })).value.events;
  assert.deepEqual(seen.map((e) => [e.kind, e.card, e.actor]), [["unblocked", w.id, "board-rules"]]);

  // The patch path: blocked_by can be set and cleared through update_card.
  const v = (await call(client, "update_card", { id: w.id, patch: { blocked_by: [] }, expected_version: w.version, ...o })).value;
  assert.equal(v.version, w.version + 1);
  assert.deepEqual((await call(client, "get_card", { id: w.id, fields: ["blocked_by"] })).value.blocked_by, []);
});
