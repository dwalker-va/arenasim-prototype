// The board's guarantees, one Board over a temp DB. The concurrency rules are
// the feature, so they carry most of the weight here; test/mutation/run.mjs
// proves the guard tests below fail when each guard is removed.
import { test } from "node:test";
import assert from "node:assert/strict";
import { BoardError } from "../dist/board.js";
import { tempBoard, seed } from "./helpers.mjs";

function refused(fn, code) {
  let err;
  try {
    fn();
  } catch (e) {
    err = e;
  }
  assert.ok(err instanceof BoardError, `expected a BoardError(${code}), got ${err === undefined ? "success" : err}`);
  assert.equal(err.code, code, err.message);
  return err;
}

// ---------------------------------------------------------------- versions

test("version guard: a stale expected_version is refused with the current card", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board);
  const v1 = c.version;
  const after = board.updateCard(c.id, { title: "first writer" }, v1, { actor: "a" });
  assert.equal(after.version, v1 + 1);

  const err = refused(() => board.updateCard(c.id, { title: "second writer" }, v1, { actor: "b" }), "stale_version");
  assert.equal(err.card.title, "first writer", "the refusal carries the card as it is now");
  assert.equal(err.card.version, v1 + 1);
  assert.equal(board.getCard(c.id).title, "first writer", "the stale write changed nothing");
});

test("version guard: stale move_card and answer_question are refused too", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board);
  board.appendActivity(c.id, "bump", { actor: "a" });
  refused(() => board.moveCard(c.id, "in_progress", c.version, { actor: "b" }), "stale_version");
  assert.equal(board.getCard(c.id).column, "backlog");

  let q = board.updateCard(c.id, { question: { text: "which?" } }, c.version + 1, { actor: "a" });
  board.appendActivity(c.id, "bump", { actor: "a" });
  refused(() => board.answerQuestion(c.id, "this one", q.version, { actor: "board" }), "stale_version");
  assert.equal(board.getCard(c.id).question.answer, undefined);
});

test("every write bumps the version exactly once", (t) => {
  const { board } = tempBoard(t);
  let c = seed(board);
  const v = c.version;
  c = board.updateCard(c.id, { priority: "P1" }, v, { actor: "a" });
  assert.equal(c.version, v + 1);
  c = board.moveCard(c.id, "in_progress", c.version, { actor: "a" });
  assert.equal(c.version, v + 2);
  c = board.claimCard(c.id, "Engineer-X", { actor: "a" });
  assert.equal(c.version, v + 3);
});

// ---------------------------------------------------------------- claims

test("claim guard: a second claim on the same card fails", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board, { column: "in_progress" });
  const won = board.claimCard(c.id, "Engineer-1", { actor: "orchestrator" });
  assert.equal(won.agent.status, "working");
  assert.equal(won.agent.name, "Engineer-1");

  const err = refused(() => board.claimCard(c.id, "Engineer-2", { actor: "orchestrator" }), "claim_refused");
  assert.equal(err.card.agent.name, "Engineer-1", "the refusal shows who holds it");
  assert.equal(board.getCard(c.id).agent.name, "Engineer-1");
});

test("claim rule: review is claimable at agent null or status done, not while working", (t) => {
  const { board } = tempBoard(t);
  const link = [{ label: "PR #1", url: "https://github.com/x/y/pull/1" }];
  const handedOff = seed(board, { column: "review", links: link, agent: { status: "done", started: "s", finished: "f" } });
  assert.equal(board.claimCard(handedOff.id, "Tester-1", { actor: "o" }).agent.status, "working");

  const reset = seed(board, { column: "review", links: link, agent: null });
  assert.equal(board.claimCard(reset.id, "Tester-2", { actor: "o" }).agent.status, "working");

  refused(() => board.claimCard(handedOff.id, "Tester-3", { actor: "o" }), "claim_refused");
});

test("claim rule: pm cards and non-spawn columns are never claimable", (t) => {
  const { board } = tempBoard(t);
  const pm = seed(board, { role: "pm", column: "in_progress" });
  refused(() => board.claimCard(pm.id, "PM", { actor: "o" }), "claim_refused");
  const backlog = seed(board);
  refused(() => board.claimCard(backlog.id, "E", { actor: "o" }), "claim_refused");
  const done = seed(board, { column: "done" });
  refused(() => board.claimCard(done.id, "E", { actor: "o" }), "claim_refused");
});

test("release_claim and finish_claim act only on a working claim", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board, { column: "in_progress" });
  refused(() => board.releaseClaim(c.id, { actor: "o" }), "claim_refused");
  board.claimCard(c.id, "Engineer-1", { actor: "o" });
  refused(() => board.finishClaim(c.id, { actor: "o", name: "Engineer-2" }), "claim_refused");

  const released = board.releaseClaim(c.id, { actor: "o", activity: "startup recovery" });
  assert.equal(released.agent, null);
  assert.equal(board.claimCard(c.id, "Engineer-3", { actor: "o" }).agent.name, "Engineer-3", "a released card is claimable again");

  const finished = board.finishClaim(c.id, { actor: "o", name: "Engineer-3" });
  assert.equal(finished.agent.status, "done");
  assert.ok(finished.agent.finished);
});

// ---------------------------------------------------------------- column rules

test("PR-link gate: a linkless non-pm card is refused into review and human_review", (t) => {
  const { board } = tempBoard(t);
  for (const role of ["engineer", "tester", "release-manager"]) {
    for (const to of ["review", "human_review"]) {
      const c = seed(board, { role, column: "in_progress" });
      refused(() => board.moveCard(c.id, to, c.version, { actor: "board" }), "gate_refused");
      assert.equal(board.getCard(c.id).column, "in_progress");
    }
  }
});

test("PR-link gate: a pm card is admitted; a link in the move's own patch satisfies it", (t) => {
  const { board } = tempBoard(t);
  const pm = seed(board, { role: "pm", column: "in_progress" });
  assert.equal(board.moveCard(pm.id, "review", pm.version, { actor: "board" }).column, "review");
  const pm2 = seed(board, { role: "pm" });
  assert.equal(board.moveCard(pm2.id, "human_review", pm2.version, { actor: "board" }).column, "human_review");

  const eng = seed(board, { column: "in_progress" });
  const moved = board.moveCard(eng.id, "review", eng.version, {
    actor: "orchestrator",
    patch: { links: [{ label: "PR #9", url: "https://github.com/x/y/pull/9" }] },
  });
  assert.equal(moved.column, "review");
});

test("PR-link gate: a gated card cannot have its last link patched away", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board, { column: "review", links: [{ label: "PR #1", url: "u" }] });
  refused(() => board.updateCard(c.id, { links: [] }, c.version, { actor: "a" }), "gate_refused");
});

test("entering in_progress sets agent: null", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board, { column: "review", links: [{ label: "PR", url: "u" }], agent: { status: "done" } });
  const back = board.moveCard(c.id, "in_progress", c.version, { actor: "board" });
  assert.equal(back.agent, null);
});

test("answering a question returns the card to in_progress with agent: null", (t) => {
  const { board } = tempBoard(t);
  let c = seed(board, { column: "needs_input" });
  c = board.updateCard(c.id, { question: { text: "A or B?" }, agent: { status: "done" } }, c.version, { actor: "o" });
  const a = board.answerQuestion(c.id, "B", c.version, { actor: "board" });
  assert.equal(a.column, "in_progress");
  assert.equal(a.agent, null);
  assert.deepEqual(a.question, { text: "A or B?", answer: "B" });
});

test("create_card allocates ids server-side, sequentially", (t) => {
  const { board } = tempBoard(t);
  const a = board.createCard({ title: "a", role: "engineer" }, { actor: "pm-session" });
  const b = board.createCard({ title: "b", role: "pm" }, { actor: "pm-session" });
  assert.equal(a.id, "AS-1");
  assert.equal(b.id, "AS-2");
  assert.equal(a.column, "backlog");
  assert.deepEqual(a.activity.map((x) => [x.by, x.msg]), [["pm-session", "Created"]]);
});

test("writes without an actor are refused", (t) => {
  const { board } = tempBoard(t);
  refused(() => board.createCard({ title: "a", role: "engineer" }, { actor: "" }), "invalid");
});

test("update_card refuses a column change (move_card owns the rules)", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board);
  refused(() => board.updateCard(c.id, { column: "review" }, c.version, { actor: "a" }), "invalid");
});

// ---------------------------------------------------------------- activity + body

test("activity is append-only at the storage layer", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board);
  assert.throws(() => board.db.prepare("UPDATE activity SET msg = 'x'").run(), /append-only/);
  assert.throws(() => board.db.prepare("DELETE FROM activity").run(), /append-only/);
  board.appendActivity(c.id, "second", { actor: "o", by: "orchestrator" });
  const acts = board.getCard(c.id).activity;
  assert.deepEqual(acts.slice(-1)[0].by, "orchestrator");
  assert.equal(board.getCard(c.id, { activity_limit: 1 }).activity.length, 1);
  assert.equal(board.getCard(c.id, { activity_limit: 1 }).activity_total, acts.length);
});

test("append_to_body adds a ## heading section", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board);
  const r = board.appendToBody(c.id, "Tester findings — 2026-09-24", "1. broken", { actor: "o", by: "tester" });
  assert.equal(r.body, "b\n\n## Tester findings — 2026-09-24\n\n1. broken");
});

// ---------------------------------------------------------------- list + events

test("list_cards returns summaries, never bodies or activity, and hides archived by default", (t) => {
  const { board } = tempBoard(t);
  seed(board);
  seed(board, { column: "archived" });
  const rows = board.listCards();
  assert.equal(rows.length, 1);
  assert.equal(rows[0].body, undefined);
  assert.equal(rows[0].activity, undefined);
  assert.deepEqual(Object.keys(rows[0]).sort(), ["agent", "column", "id", "links", "priority", "role", "title", "updated", "version"]);
  assert.equal(board.listCards({ include_archived: true }).length, 2);
  assert.equal(board.listCards({ column: "archived" }).length, 1);
  assert.equal(board.listCards({ fields: ["body"] })[0].body, "b");
});

test("events_since does not echo the caller's own writes, and its cursor skips past them", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board);
  const start = board.head();
  board.appendActivity(c.id, "mine", { actor: "orchestrator" });
  board.moveCard(c.id, "in_progress", board.getCard(c.id).version, { actor: "board" });
  board.appendActivity(c.id, "mine again", { actor: "orchestrator" });

  const r = board.eventsSince(start, { ignore_actors: ["orchestrator"] });
  assert.deepEqual(r.events.map((e) => [e.actor, e.kind, e.data.to]), [["board", "moved", "in_progress"]]);
  assert.equal(r.cursor, board.head(), "the cursor advances past filtered events");
  assert.deepEqual(board.eventsSince(r.cursor, { ignore_actors: ["orchestrator"] }).events, []);
});

test("a refused write records no event", (t) => {
  const { board } = tempBoard(t);
  const c = seed(board, { column: "in_progress" });
  board.claimCard(c.id, "E1", { actor: "o" });
  const h = board.head();
  const seen = [];
  board.on("event", (e) => seen.push(e));
  try {
    board.claimCard(c.id, "E2", { actor: "o" });
  } catch {}
  assert.equal(board.head(), h);
  assert.deepEqual(seen, []);
});
