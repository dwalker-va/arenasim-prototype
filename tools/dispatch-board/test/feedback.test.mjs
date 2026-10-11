// Review feedback as batches: a submitted review saves its comments as one
// batch and files no card; the orchestrator reads the batch, files what it
// needs, and records each comment's triage (filed / merged_into / no_action),
// which links the cards back and shows on the review page. Also: every text
// the review page draws is escaped.
import { test } from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { BoardError, Board } from "../dist/board.js";
import { tempBoard, tempDir, spawnDaemon, mcpClient, call, post, sleep } from "./helpers.mjs";

const { reviewPage, startDaemon } = await import("../dist/server.js");
const o = { actor: "orchestrator" };
const u = { actor: "board" };

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

async function until(what, pred, ms = 5000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await pred();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(20);
  }
}

function guideFor(a, b, c, text = {}) {
  return {
    units: [
      {
        id: "druid-root",
        title: text.title ?? "Druid in a root",
        summary: text.summary ?? "A rooted Druid shifts out of the root instead of standing in it.",
        checks: [
          { id: "shift", text: text.check ?? "Watch the Druid shift out of Frost Nova within a second", ...(text.replay ? { replay: text.replay } : {}) },
          { id: "no-loop", text: "It never leaves Travel Form straight back into a shift" },
        ],
        cards: [a, b],
      },
      { id: "flare", title: "Flare readability", summary: "Flare reads as one flash on the ground.", checks: [{ id: "flash", text: "One flash, no trail" }], cards: [c] },
    ],
  };
}

/** A milestone with three cards and a guide over them. */
function withGuide(board, text) {
  board.createMilestone("0.7", o);
  const ids = [1, 2, 3].map((n) => board.createCard({ title: text?.cardTitle ?? `card ${n}`, role: "engineer", milestone: "0.7" }, o).id);
  board.setReviewGuide("0.7", guideFor(...ids, text), board.getMilestone("0.7").milestone.version, o);
  return ids;
}

/** Save drafts {key: text} and submit them all, as the page does. */
function submit(board, drafts) {
  const expected = {};
  for (const [k, text] of Object.entries(drafts)) expected[k] = board.saveReviewDraft("0.7", k, text, 0, u).version;
  return board.submitReview("0.7", expected, u).batch;
}

// ---------------------------------------------------------------- the board

test("batch: guide units and checks take comments, and a batch round-trips through get, list and the milestone payload", (t) => {
  const { board } = tempBoard(t);
  const ids = withGuide(board);
  refused(() => board.saveReviewDraft("0.7", "guide:nope", "x", 0, u), "invalid", /not a review item/);
  refused(() => board.saveReviewDraft("0.7", "guide:druid-root:nope", "x", 0, u), "invalid", /not a review item/);
  const cardsBefore = board.listCards({ include_archived: true });
  const b = submit(board, {
    "guide:druid-root": "The shift reads late",
    "guide:druid-root:shift": "It took two seconds, not one",
    "guide:flare:flash": "Two flashes",
    milestone: "Otherwise good",
  });
  assert.deepEqual(board.listCards({ include_archived: true }), cardsBefore, "a submission filed a card");
  assert.deepEqual(
    b.comments.map((c) => [c.id, c.key, c.target, c.text, c.triage]),
    [
      ["c1", "guide:druid-root", { kind: "guide_unit", unit: "druid-root", title: "Druid in a root" }, "The shift reads late", null],
      ["c2", "guide:druid-root:shift", { kind: "guide_check", unit: "druid-root", check: "shift", title: "Druid in a root", text: "Watch the Druid shift out of Frost Nova within a second" }, "It took two seconds, not one", null],
      ["c3", "guide:flare:flash", { kind: "guide_check", unit: "flare", check: "flash", title: "Flare readability", text: "One flash, no trail" }, "Two flashes", null],
      ["c4", "milestone", { kind: "milestone" }, "Otherwise good", null],
    ],
  );
  assert.deepEqual(board.getFeedbackBatch("FB-1"), b);
  // The snapshot is the words the user saw: a later guide rewrite does not change the batch.
  board.setReviewGuide("0.7", guideFor(...ids, { title: "Renamed" }), board.getMilestone("0.7").milestone.version, o);
  assert.deepEqual(board.getFeedbackBatch("FB-1"), b);
  assert.deepEqual(board.getMilestone("0.7").feedback.batches, [b]);

  const summary = { id: "FB-1", milestone: "0.7", t: b.t, by: "board", comments: 4, pending: 4, version: 1 };
  assert.deepEqual(board.listFeedbackBatches(), [summary]);
  assert.deepEqual(board.listFeedbackBatches({ milestone: "0.7", pending: true }), [summary]);
  assert.deepEqual(board.listFeedbackBatches({ milestone: "0.8" }), []);
  refused(() => board.getFeedbackBatch("FB-9"), "not_found");
  refused(() => board.getFeedbackBatch("AS-1"), "not_found");
});

test("triage: each outcome is recorded under the batch's version and links its card back; a stale write is refused and changes nothing", (t) => {
  const { board } = tempBoard(t);
  withGuide(board);
  const b = submit(board, { "guide:druid-root": "Shift reads late", "guide:druid-root:shift": "Took two seconds", "guide:flare:flash": "Two flashes", milestone: "Fine" });
  const x = board.createCard({ title: "Druid shift timing", role: "engineer", milestone: "0.7", iteration: 2 }, o);
  const head = board.head();
  const after = board.triageFeedback(
    b.id,
    [
      { comment: "c1", outcome: "filed", card: x.id },
      { comment: "c2", outcome: "merged_into", card: x.id, reason: "same delay" },
      { comment: "c3", outcome: "no_action", reason: "Already fixed by AS-213" },
    ],
    b.version,
    o,
  );
  assert.equal(after.version, 2);
  assert.deepEqual(
    after.comments.map((c) => c.triage && [c.triage.outcome, c.triage.card ?? null, c.triage.reason ?? null, c.triage.by]),
    [["filed", x.id, null, "orchestrator"], ["merged_into", x.id, "same delay", "orchestrator"], ["no_action", null, "Already fixed by AS-213", "orchestrator"], null],
  );
  assert.deepEqual(board.getFeedbackBatch(b.id), after, "the triage is stored");
  assert.deepEqual(board.getMilestone("0.7").feedback.batches[0].comments.map((c) => c.triage?.outcome ?? "pending"), ["filed", "merged_into", "no_action", "pending"]);
  assert.deepEqual(board.listFeedbackBatches({ pending: true }).map((s) => [s.id, s.pending]), [["FB-1", 1]]);
  let card = board.getCard(x.id);
  assert.deepEqual(card.from_feedback, [
    { batch: "FB-1", comment: "c1", milestone: "0.7", outcome: "filed" },
    { batch: "FB-1", comment: "c2", milestone: "0.7", outcome: "merged_into" },
  ]);
  assert.deepEqual(card.activity.slice(-2).map((a) => a.msg), ["Filed from milestone 0.7 review feedback FB-1/c1", "Merged with milestone 0.7 review feedback FB-1/c2"]);
  assert.deepEqual(board.eventsSince(head).events.map((e) => [e.kind, e.data]), [["feedback_triaged", { milestone: "0.7", batch: "FB-1", comments: ["c1", "c2", "c3"], cards: [x.id] }]]);

  // A write naming the version it read before the triage is refused, with the batch as it is now.
  const err = refused(() => board.triageFeedback(b.id, [{ comment: "c4", outcome: "no_action", reason: "noted" }], b.version, o), "stale_version", /version 2, not 1/);
  assert.equal(err.current.version, 2);
  assert.deepEqual(board.getFeedbackBatch(b.id), after, "a stale write changed the batch");
  assert.equal(board.getCard(x.id).version, card.version);

  // Re-triage moves the link: c2 is now no action, so the card no longer names it.
  const re = board.triageFeedback(b.id, [{ comment: "c2", outcome: "no_action", reason: "answered in the guide" }, { comment: "c4", outcome: "no_action", reason: "approval" }], 2, o);
  assert.deepEqual(re.comments.map((c) => c.triage.outcome), ["filed", "no_action", "no_action", "no_action"]);
  card = board.getCard(x.id);
  assert.deepEqual(card.from_feedback, [{ batch: "FB-1", comment: "c1", milestone: "0.7", outcome: "filed" }]);
  assert.equal(card.activity.at(-1).msg, "No longer linked to milestone 0.7 review feedback FB-1/c2");
  assert.deepEqual(board.listFeedbackBatches({ pending: true }), []);
  // A card that predates batches keeps its own `source`; nothing here touches it.
  assert.equal(card.source, undefined);
});

test("triage: refuses what it cannot record, writing nothing", (t) => {
  const { board } = tempBoard(t);
  withGuide(board);
  const b = submit(board, { milestone: "one", "guide:flare": "two" });
  const x = board.createCard({ title: "x", role: "engineer" }, o).id;
  const bad = [
    [[], /non-empty array/],
    [[{ comment: "c9", outcome: "no_action", reason: "r" }], /no comment c9/],
    [[{ comment: "c1", outcome: "filed" }], /needs the card/],
    [[{ comment: "c1", outcome: "merged_into", card: "AS-999" }], /no card AS-999/],
    [[{ comment: "c1", outcome: "no_action" }], /reason/],
    [[{ comment: "c1", outcome: "no_action", reason: "  " }], /reason/],
    [[{ comment: "c1", outcome: "no_action", reason: "r", card: x }], /names no card/],
    [[{ comment: "c1", outcome: "dropped", reason: "r" }], /outcome must be one of/],
    [[{ comment: "c1", outcome: "filed", card: x }, { comment: "c1", outcome: "no_action", reason: "r" }], /twice/],
    [[{ comment: "c1", outcome: "filed", card: x, note: "x" }], /unknown field/],
  ];
  for (const [entries, match] of bad) refused(() => board.triageFeedback(b.id, entries, b.version, o), "invalid", match);
  refused(() => board.triageFeedback("FB-7", [{ comment: "c1", outcome: "no_action", reason: "r" }], 1, o), "not_found");
  refused(() => board.triageFeedback(b.id, [{ comment: "c1", outcome: "no_action", reason: "r" }], undefined, o), "invalid", /expected_version/);
  assert.deepEqual(board.getFeedbackBatch(b.id), b);
  assert.equal(board.getCard(x).from_feedback, undefined);
});

// ---------------------------------------------------------------- MCP

test("MCP: the orchestrator lists pending batches, reads one, records its triage; a stale triage is a tool error", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  assert.ok((await call(client, "create_milestone", { name: "0.7", ...o })).ok);
  const ids = [];
  for (const n of [1, 2, 3]) ids.push((await call(client, "create_card", { title: `card ${n}`, role: "engineer", milestone: "0.7", ...o })).value.id);
  const v = (await call(client, "get_milestone", { name: "0.7" })).value.milestone.version;
  assert.ok((await call(client, "set_review_guide", { name: "0.7", guide: guideFor(...ids), expected_version: v, ...o })).ok);
  for (const [key, text] of [["guide:flare:flash", "Two flashes"], ["guide:flare", "Flare is hard to see"]]) {
    assert.equal((await post(d.base, "/api/milestones/0.7/feedback", { key, text, expected_version: 0 })).status, 200);
  }
  const sub = await post(d.base, "/api/milestones/0.7/submit", { expected: { "guide:flare:flash": 1, "guide:flare": 1 } });
  assert.equal(sub.status, 200, JSON.stringify(sub.body));
  assert.equal(sub.body.cards, undefined, "the submit response names no filed cards");

  const pending = (await call(client, "list_feedback_batches", { pending: true })).value;
  assert.deepEqual(pending.map((s) => [s.id, s.comments, s.pending, s.version]), [["FB-1", 2, 2, 1]]);
  const batch = (await call(client, "get_feedback_batch", { id: "FB-1" })).value;
  assert.deepEqual(batch, sub.body.batch);
  // Two comments, one theme: one card answers both.
  const card = (await call(client, "create_card", { title: "Flare visibility", role: "engineer", milestone: "0.7", iteration: 2, ...o })).value;
  const tri = await call(client, "triage_feedback", { id: "FB-1", triage: [{ comment: "c1", outcome: "filed", card: card.id }, { comment: "c2", outcome: "merged_into", card: card.id }], expected_version: 1, ...o });
  assert.ok(tri.ok, JSON.stringify(tri.value));
  assert.deepEqual(tri.value.comments.map((c) => [c.triage.outcome, c.triage.card]), [["filed", card.id], ["merged_into", card.id]]);
  assert.deepEqual((await call(client, "get_card", { id: card.id })).value.from_feedback.map((f) => f.comment), ["c1", "c2"]);
  assert.deepEqual((await call(client, "list_feedback_batches", { pending: true })).value, []);

  const stale = await call(client, "triage_feedback", { id: "FB-1", triage: [{ comment: "c1", outcome: "no_action", reason: "x" }], expected_version: 1, ...o });
  assert.equal(stale.ok, false);
  assert.equal(stale.value.error, "stale_version");
  assert.equal(stale.value.current.version, 2);
  const evs = (await call(client, "events_since", { cursor: 0 })).value.events.filter((e) => /review_submitted|feedback_triaged/.test(e.kind));
  assert.deepEqual(evs.map((e) => [e.kind, e.data.batch]), [["review_submitted", "FB-1"], ["feedback_triaged", "FB-1"]]);
});

// ---------------------------------------------------------------- the review page

/** An in-process daemon over `db`; the page drives it in jsdom, `nudge()` fires the SSE message by hand. */
async function openPage(t, db, ready) {
  const daemon = await startDaemon({ dbPath: db, port: 0, log: () => {} });
  const base = `http://127.0.0.1:${daemon.port}`;
  // The page's own requests may still be landing when a test ends (it loads
  // on open and on connect): let them settle before the window and daemon go.
  const inflight = new Set();
  t.after(async () => {
    while (inflight.size) await Promise.allSettled([...inflight]);
    await sleep(50);
    dom.window.close();
    await daemon.close();
  });
  let es;
  const dom = new JSDOM(reviewPage(), {
    url: `${base}/milestones/0.7`,
    runScripts: "dangerously",
    beforeParse(window) {
      window.fetch = (p, opts) => {
        const r = fetch(new URL(p, base), opts);
        const tracked = r.then((res) => res.clone().text()).finally(() => inflight.delete(tracked));
        inflight.add(tracked);
        return r;
      };
      window.EventSource = class {
        constructor() {
          es = this;
        }
      };
    },
  });
  const doc = dom.window.document;
  es.onopen();
  await until("the page to load", () => ready(doc));
  const fire = (el, type) => el.dispatchEvent(new dom.window.Event(type, { bubbles: true }));
  return { board: daemon.board, dom, doc, fire, nudge: () => es.onmessage({}), status: () => doc.querySelector(".hdr .stat").textContent };
}

/** A temp DB set up by `fn(board)` and closed, for a daemon to open. */
function preparedDb(t, fn) {
  const tmp = tempDir();
  t.after(tmp.cleanup);
  const board = new Board(tmp.db);
  try {
    fn(board);
  } finally {
    board.close();
  }
  return tmp.db;
}

test("review page: an untriaged comment shows as pending, a triaged one its outcome", async (t) => {
  let x;
  const db = preparedDb(t, (board) => {
    withGuide(board);
    submit(board, { "guide:druid-root:shift": "Took two seconds", "guide:flare:flash": "Two flashes", milestone: "Fine otherwise" });
    x = board.createCard({ title: "Druid shift timing", role: "engineer", milestone: "0.7", iteration: 2 }, o).id;
  });
  const pg = await openPage(t, db, (doc) => doc.querySelector('#batches .batch[data-batch="FB-1"]'));
  const row = (c) => pg.doc.querySelector(`#batches .batch[data-batch="FB-1"] .bcomment[data-comment="${c}"]`);
  for (const c of ["c1", "c2", "c3"]) assert.match(row(c).querySelector(".triage.pending").textContent, /^Pending/, `${c} is not shown as pending`);
  assert.match(row("c1").querySelector(".tgt").textContent, /Druid in a root — Watch the Druid shift/);
  assert.equal(row("c1").querySelector(".said").textContent, "Took two seconds");
  assert.match(pg.doc.querySelector('#batches .batch[data-batch="FB-1"] .top').textContent, /3 comments, 3 pending/);

  pg.board.triageFeedback("FB-1", [{ comment: "c1", outcome: "filed", card: x }, { comment: "c2", outcome: "no_action", reason: "Fixed by AS-213" }], 1, o);
  pg.nudge();
  await until("the triage to render", () => row("c1").querySelector(".triage.filed"));
  assert.equal(row("c1").querySelector(".triage").textContent, `Filed as ${x}`);
  assert.equal(row("c1").querySelector(".triage a.cid").textContent, x);
  assert.equal(row("c2").querySelector(".triage.no_action").textContent, "No action — Fixed by AS-213");
  assert.ok(row("c3").querySelector(".triage.pending"), "the comment left untriaged stopped showing as pending");
  assert.match(pg.doc.querySelector('#batches .batch[data-batch="FB-1"] .top').textContent, /3 comments, 1 pending/);
});

test("review page: guide units and checks take comments, and the guide's own Submit sends them as a batch, filing no card", async (t) => {
  const db = preparedDb(t, (board) => withGuide(board));
  const pg = await openPage(t, db, (doc) => doc.querySelector("#guide .unit"));
  const { doc } = pg;
  const type = (key, text) => {
    doc.querySelector(`#guide [data-open="${key}"]`).click();
    const ta = [...doc.querySelectorAll("#guide textarea[data-key]")].find((x) => x.dataset.key === key);
    ta.value = text;
    pg.fire(ta, "input");
    pg.fire(ta, "blur");
  };
  type("guide:druid-root", "The unit as a whole");
  await until("the unit comment to save", () => pg.board.getMilestone("0.7").feedback.drafts["guide:druid-root"]);
  type("guide:flare:flash", "Two flashes, not one");
  await until("the check comment to save", () => pg.board.getMilestone("0.7").feedback.drafts["guide:flare:flash"]);
  const cards = pg.board.listCards({ include_archived: true });
  const submitBtn = doc.querySelector("#feedback #submit");
  assert.ok(!doc.getElementById("detail").contains(submitBtn), "Submit is inside the collapsed detail");
  submitBtn.click();
  assert.match(doc.getElementById("submit").textContent, /Confirm: send 2 comments/);
  doc.getElementById("submit").click();
  await until("the submission", () => /submitted — FB-1, 2 comments sent for triage/.test(pg.status()));
  assert.deepEqual(pg.board.listCards({ include_archived: true }), cards, "submitting filed a card");
  assert.deepEqual(
    pg.board.getFeedbackBatch("FB-1").comments.map((c) => [c.target.kind, c.text]),
    [["guide_unit", "The unit as a whole"], ["guide_check", "Two flashes, not one"]],
  );
});

test("review page: a milestone reviewed before batches still shows its rounds, their auto-filed cards and each card's source", async (t) => {
  // The shape 0.7's first two rounds left on the live board: submissions on
  // the milestone, one auto-filed card per comment, each with its `source`.
  const card = (id, extra) => ({ id, title: `card ${id}`, body: "", column: "backlog", role: "engineer", priority: "P2", links: [], pr: null, worktree: null, question: null, agent: null, created: "2026-10-01T00:00:00", updated: "2026-10-01T00:00:00", activity: [], milestone: "0.7", iteration: 1, ...extra });
  const db = preparedDb(t, (board) =>
    board.importState(
      {
        schema: 2,
        nextId: 4,
        cards: [card("AS-1"), card("AS-2", { iteration: 2, title: "Review feedback on AS-1: too bright", source: { milestone: "0.7", key: "card:AS-1", card: "AS-1" } }), card("AS-3", { iteration: 2, source: { milestone: "0.7", key: "milestone", card: null } })],
        milestones: [{ name: "0.7", status: "in_review", created: "2026-10-01T00:00:00", updated: "2026-10-02T00:00:00", first_id: 1, sweep: null, submissions: [{ t: "2026-10-02T00:00:00", by: "board", cards: ["AS-2", "AS-3"] }, { t: "2026-10-03T00:00:00", by: "board", cards: [] }], review: [] }],
      },
      { actor: "import" },
    ),
  );
  const pg = await openPage(t, db, (doc) => doc.querySelector("#legacysubs"));
  const items = [...pg.doc.querySelectorAll("#legacysubs li")].map((li) => li.textContent);
  assert.equal(items.length, 2);
  assert.match(items[0], /no feedback/);
  assert.match(items[1], /filed AS-2, AS-3 \(one card per comment, before triage\)/);
  assert.equal(pg.board.getMilestone("0.7").feedback.batches.length, 0);
  assert.deepEqual(pg.board.getCard("AS-2").source, { milestone: "0.7", key: "card:AS-1", card: "AS-1" }, "an old card's source is still readable");
  assert.deepEqual(pg.board.getMilestone("0.7").in_flight.find((c) => c.id === "AS-2").source.card, "AS-1");
});

// The hostile strings: each would become markup if any one draw skipped esc().
const IMG = '<img src=x onerror="window.__pwned=1">';
const SCRIPT = "<script>window.__pwned=2</script>";
const BREAK = '"><b id=pwn>pwn</b>';
const CLOSE = "</textarea><i id=pwn2>pwn</i>";
const HOSTILE = `${IMG} ${SCRIPT} ${BREAK} ${CLOSE}`;

test("review page: every text a person or agent typed renders as text, never as markup", async (t) => {
  let x;
  const db = preparedDb(t, (board) => {
    withGuide(board, { title: `Title ${HOSTILE}`, summary: `Summary ${HOSTILE}`, check: `Check ${HOSTILE}`, cardTitle: `Card ${HOSTILE}`, replay: { team1: [BREAK], team2: ["Rogue"] } });
    submit(board, { "guide:druid-root": `Unit comment ${HOSTILE}`, "guide:druid-root:shift": `Check comment ${HOSTILE}`, milestone: `Milestone comment ${HOSTILE}` });
    x = board.createCard({ title: `Filed ${HOSTILE}`, role: "engineer", milestone: "0.7" }, o).id;
    board.triageFeedback("FB-1", [{ comment: "c1", outcome: "filed", card: x }, { comment: "c2", outcome: "no_action", reason: `Reason ${HOSTILE}` }], 1, o);
    board.saveReviewDraft("0.7", "milestone", `Draft ${HOSTILE}`, 2, u);
  });
  const pg = await openPage(t, db, (doc) => doc.querySelector("#batches .bcomment"));
  const { doc, dom } = pg;
  const app = doc.getElementById("app");
  assert.equal(app.querySelectorAll("img").length, 0, "an <img> was injected");
  assert.equal(app.querySelectorAll("script").length, 0, "a <script> was injected");
  assert.equal(doc.getElementById("pwn"), null, "an attribute break injected markup");
  assert.equal(doc.getElementById("pwn2"), null, "a closing tag broke out of a textarea");
  assert.equal(app.querySelectorAll("b").length, 0, "an attribute break injected markup");
  assert.equal(dom.window.__pwned, undefined);

  // ...and each reads back exactly as typed.
  const unit = doc.querySelector('#guide .unit[data-unit="druid-root"]');
  assert.equal(unit.querySelector(".ttl").textContent, `Title ${HOSTILE}`);
  assert.equal(unit.querySelector(".sum").textContent, `Summary ${HOSTILE}`);
  assert.equal(unit.querySelector('[data-guide-check="guide:druid-root:shift"] .said').textContent, `Check ${HOSTILE}`);
  const said = [...doc.querySelectorAll("#batches .bcomment .said")].map((s) => s.textContent);
  assert.deepEqual(said, [`Unit comment ${HOSTILE}`, `Check comment ${HOSTILE}`, `Milestone comment ${HOSTILE}`]);
  assert.match(doc.querySelector('#batches .bcomment[data-comment="c1"] .tgt').textContent, new RegExp(`Unit: Title ${HOSTILE.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`));
  assert.equal(doc.querySelector('#batches .bcomment[data-comment="c2"] .triage .why').textContent, `Reason ${HOSTILE}`);
  const draft = [...doc.querySelectorAll("textarea[data-key]")].find((ta) => ta.dataset.key === "milestone");
  assert.equal(draft.value, `Draft ${HOSTILE}`);
  assert.match(doc.querySelector("#changed").textContent, /Card <img src=x/);
});
