// The milestone review guide: themed review units the orchestrator writes
// with set_review_guide, ticks that carry over across guide updates (unless
// the unit changed or the check is new), and the review page leading with it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { BoardError, guideCheckKey } from "../dist/board.js";
import { tempBoard, spawnDaemon, mcpClient, call, post, sleep } from "./helpers.mjs";

const { reviewPage } = await import("../dist/server.js");
const o = { actor: "orchestrator" };

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

const REPLAY = { team1: ["Druid", "Priest"], team2: ["Warrior", "Rogue"], map: "PillaredArena", random_seed: 42 };

/** A two-unit guide over cards a, b and c: the shape the orchestrator writes. */
function guideFor(a, b, c) {
  return {
    units: [
      {
        id: "druid-root",
        title: "Druid in a root",
        summary: "A rooted Druid shifts out of the root instead of standing in it, so melee trains no longer pin it to a pillar.",
        checks: [
          { id: "shift", text: "Watch the Druid shift out of Frost Nova within a second", replay: REPLAY },
          { id: "no-loop", text: "It never leaves Travel Form straight back into a shift" },
        ],
        cards: [a, b],
      },
      {
        id: "flare",
        title: "Flare readability",
        summary: "Flare reads as one flash on the ground, not a second projectile.",
        checks: [{ id: "flash", text: "One flash, no trail" }],
        cards: [c],
      },
    ],
  };
}

function milestoneWithCards(board) {
  board.createMilestone("0.7", o);
  const ids = [1, 2, 3].map((n) => board.createCard({ title: `card ${n}`, role: "engineer", milestone: "0.7" }, o).id);
  return ids;
}

const version = (board) => board.getMilestone("0.7").milestone.version;
const ticks = (board) => board.getMilestone("0.7").guide.ticks;
/** Backdate a tick, as if the user made it in an earlier round. */
function backdate(board, key, t) {
  board.db.prepare("UPDATE review_items SET checked_at = ? WHERE milestone = '0.7' AND key = ?").run(t, key);
}

test("guide: set_review_guide stores the units as given, and get_milestone returns them once, with ticks beside them", (t) => {
  const { board } = tempBoard(t);
  const [a, b, c] = milestoneWithCards(board);
  assert.equal(board.getMilestone("0.7").guide, null, "a milestone starts with no guide");
  const g = guideFor(a, b, c);
  const m = board.setReviewGuide("0.7", g, version(board), o);
  assert.deepEqual(m.review_guide.units, g.units);
  const p = board.getMilestone("0.7");
  assert.deepEqual(p.guide.units, g.units, "the units come back exactly as written, replay config as structured JSON");
  assert.equal(p.milestone.review_guide, undefined, "the guide is returned once, not again inside milestone");
  assert.deepEqual(Object.keys(p.guide.ticks), ["guide:druid-root:shift", "guide:druid-root:no-loop", "guide:flare:flash"]);
  assert.deepEqual(p.guide.ticks["guide:flare:flash"], { checked: false, checked_at: null, checked_by: null, version: 0 });
  // What get_milestone returns is what set_review_guide takes back.
  board.setReviewGuide("0.7", { units: p.guide.units }, p.milestone.version, o);
  assert.deepEqual(board.getMilestone("0.7").guide.units, g.units);
});

test("guide: a stale version is refused with the milestone as it is now, and nothing is written", (t) => {
  const { board } = tempBoard(t);
  const [a, b, c] = milestoneWithCards(board);
  const v = version(board);
  board.setReviewGuide("0.7", guideFor(a, b, c), v, o);
  const err = refused(() => board.setReviewGuide("0.7", { units: [] }, v, o), "stale_version");
  assert.equal(err.current.version, v + 1);
  assert.equal(board.getMilestone("0.7").guide.units.length, 2, "the stale write replaced the guide");
});

test("guide: malformed units are refused", (t) => {
  const { board } = tempBoard(t);
  const [a, b, c] = milestoneWithCards(board);
  const other = board.createCard({ title: "elsewhere", role: "engineer" }, o).id;
  const v = version(board);
  const unit = (over) => ({ units: [{ ...guideFor(a, b, c).units[0], ...over }] });
  refused(() => board.setReviewGuide("0.7", unit({ checks: [] }), v, o), "invalid", /1-3 checks/);
  refused(() => board.setReviewGuide("0.7", unit({ checks: [1, 2, 3, 4].map((n) => ({ id: `c${n}`, text: "x" })) }), v, o), "invalid", /1-3 checks/);
  refused(() => board.setReviewGuide("0.7", unit({ checks: [{ id: "x", text: "a" }, { id: "x", text: "b" }] }), v, o), "invalid", /duplicate check id/);
  refused(() => board.setReviewGuide("0.7", { units: [guideFor(a, b, c).units[0], guideFor(a, b, c).units[0]] }, v, o), "invalid", /duplicate unit id/);
  refused(() => board.setReviewGuide("0.7", unit({ id: "has:colon" }), v, o), "invalid", /\.id must be/);
  refused(() => board.setReviewGuide("0.7", unit({ cards: [other] }), v, o), "invalid", /not a card on this milestone/);
  refused(() => board.setReviewGuide("0.7", unit({ cards: [] }), v, o), "invalid", /at least one card/);
  refused(() => board.setReviewGuide("0.7", unit({ title: "two\nlines" }), v, o), "invalid", /one line/);
  refused(() => board.setReviewGuide("0.7", unit({ checks: [{ id: "x", text: "a", replay: "--headless x.json" }] }), v, o), "invalid", /JSON object/);
  refused(() => board.setReviewGuide("0.7", unit({ checks: [{ id: "x", text: "a", replay: { team1: ["Mage"] } }] }), v, o), "invalid", /team2/);
  refused(() => board.setReviewGuide("0.7", unit({ changed_since: "2099-01-01T00:00:00" }), v, o), "invalid", /in the future/);
  refused(() => board.setReviewGuide("0.7", unit({ changed_since: "2026-10-01" }), v, o), "invalid", /board timestamp/);
  refused(() => board.setReviewGuide("0.7", unit({ notes: "x" }), v, o), "invalid", /unknown field/);
  refused(() => board.setReviewGuide("0.7", { units: [], intro: "x" }, v, o), "invalid", /unknown field/);
  assert.equal(version(board), v, "a refused guide wrote nothing");
});

test("guide: ticks persist across an update that does not mark the unit changed, and reset when it does", (t) => {
  const { board } = tempBoard(t);
  const [a, b, c] = milestoneWithCards(board);
  const g = guideFor(a, b, c);
  board.setReviewGuide("0.7", g, version(board), o);
  const shift = guideCheckKey("druid-root", "shift");
  const flash = guideCheckKey("flare", "flash");
  board.setReviewCheck("0.7", shift, true, 0, { actor: "board" });
  board.setReviewCheck("0.7", flash, true, 0, { actor: "board" });
  backdate(board, shift, "2026-01-01T00:00:00");
  backdate(board, flash, "2026-01-01T00:00:00");

  // A reworded summary and check, no changed_since: both ticks carry over untouched.
  const reworded = structuredClone(g);
  reworded.units[0].summary += " (reworded)";
  reworded.units[0].checks[0].text = "Watch the Druid shift out of the root";
  board.setReviewGuide("0.7", reworded, version(board), o);
  let tk = ticks(board);
  assert.deepEqual([tk[shift].checked, tk[shift].version, tk[flash].checked], [true, 1, true], "an unchanged unit kept its tick");

  // The orchestrator marks the root unit changed (after the tick): its ticks reset, the flare unit's stay.
  const changed = structuredClone(reworded);
  changed.units[0].changed_since = "2026-02-01T00:00:00";
  board.setReviewGuide("0.7", changed, version(board), o);
  tk = ticks(board);
  assert.deepEqual([tk[shift].checked, tk[shift].checked_at, tk[shift].version], [false, null, 2], "a changed unit's tick was cleared, under a new version");
  assert.equal(tk[flash].checked, true, "an unchanged unit lost its tick");
  assert.equal(board.getMilestone("0.7").guide.units[0].changed_since, "2026-02-01T00:00:00");

  // A page still holding the pre-change version cannot tick over the reset.
  refused(() => board.setReviewCheck("0.7", shift, true, 1, { actor: "board" }), "stale_version");
  // Re-ticked in the new round, then the guide re-written with the same changed_since: the new tick stays.
  board.setReviewCheck("0.7", shift, true, 2, { actor: "board" });
  board.setReviewGuide("0.7", { units: board.getMilestone("0.7").guide.units }, version(board), o);
  assert.equal(ticks(board)[shift].checked, true, "re-sending the stored changed_since cleared a tick made after it");
});

test("guide: a new check starts unticked, and a dropped check's tick does not come back with it", (t) => {
  const { board } = tempBoard(t);
  const [a, b, c] = milestoneWithCards(board);
  const g = guideFor(a, b, c);
  board.setReviewGuide("0.7", g, version(board), o);
  const flash = guideCheckKey("flare", "flash");
  board.setReviewCheck("0.7", flash, true, 0, { actor: "board" });

  const more = structuredClone(g);
  more.units[1].checks.push({ id: "colour", text: "The flash is not team-coloured" });
  board.setReviewGuide("0.7", more, version(board), o);
  assert.deepEqual([ticks(board)[flash].checked, ticks(board)[guideCheckKey("flare", "colour")].checked], [true, false]);

  const dropped = structuredClone(more);
  dropped.units[1].checks = [{ id: "colour", text: "The flash is not team-coloured" }];
  board.setReviewGuide("0.7", dropped, version(board), o);
  refused(() => board.setReviewCheck("0.7", flash, true, 2, { actor: "board" }), "invalid", /not a current checklist step/);
  board.setReviewGuide("0.7", more, version(board), o);
  assert.equal(ticks(board)[flash].checked, false, "a check dropped and re-added came back ticked");
});

test("guide: refused on a released milestone; exported and imported with the milestone", (t) => {
  const { board } = tempBoard(t);
  const [a, b, c] = milestoneWithCards(board);
  board.setReviewGuide("0.7", guideFor(a, b, c), version(board), o);
  board.setReviewCheck("0.7", guideCheckKey("flare", "flash"), true, 0, { actor: "board" });
  const state = board.exportState();
  const { board: copy } = tempBoard(t);
  copy.importState(state, { actor: "import" });
  assert.deepEqual(copy.getMilestone("0.7").guide.units, guideFor(a, b, c).units);
  assert.equal(copy.getMilestone("0.7").guide.ticks[guideCheckKey("flare", "flash")].checked, true);

  for (const id of [a, b, c]) board.updateCard(id, { milestone: null }, board.getCard(id).version, o);
  board.closeMilestone("0.7", version(board), o);
  refused(() => board.setReviewGuide("0.7", { units: [] }, version(board), o), "invalid", /released/);
});

// ---------------------------------------------------------------- over the daemon

async function until(what, pred, ms = 5000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await pred();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(20);
  }
}

async function daemonMilestone(t) {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  assert.ok((await call(client, "create_milestone", { name: "0.7", ...o })).ok);
  const ids = [];
  for (const n of [1, 2, 3]) {
    ids.push((await call(client, "create_card", { title: `card ${n}`, role: "engineer", milestone: "0.7", area: "combat", summary: `summary ${n}`, ...o })).value.id);
  }
  return { d, client, ids };
}

test("MCP: set_review_guide round-trips through get_milestone; a stale version is a tool error", async (t) => {
  const { client, ids } = await daemonMilestone(t);
  const g = guideFor(...ids);
  let p = (await call(client, "get_milestone", { name: "0.7" })).value;
  assert.equal(p.guide, null);
  const set = await call(client, "set_review_guide", { name: "0.7", guide: g, expected_version: p.milestone.version, ...o });
  assert.ok(set.ok, JSON.stringify(set.value));
  p = (await call(client, "get_milestone", { name: "0.7" })).value;
  assert.deepEqual(p.guide.units, g.units);
  assert.equal(p.guide.by, "orchestrator");

  const stale = await call(client, "set_review_guide", { name: "0.7", guide: { units: [] }, expected_version: p.milestone.version - 1, ...o });
  assert.equal(stale.ok, false);
  assert.equal(stale.value.error, "stale_version");
  assert.equal(stale.value.current.version, p.milestone.version);
  assert.deepEqual((await call(client, "get_milestone", { name: "0.7" })).value.guide.units, g.units, "a stale write changed the guide");

  // Passing the units straight back is a valid write, and it is a board event.
  const again = await call(client, "set_review_guide", { name: "0.7", guide: { units: p.guide.units }, expected_version: p.milestone.version, ...o });
  assert.ok(again.ok, JSON.stringify(again.value));
  const evs = (await call(client, "events_since", { cursor: 0 })).value.events.filter((e) => e.kind === "milestone_guide");
  assert.deepEqual(evs.map((e) => [e.data.milestone, e.data.units]), [["0.7", 2], ["0.7", 2]]);
});

/** Open the review page over the daemon; `nudge()` fires the SSE message by hand. */
async function openReview(t, base, ready) {
  let es;
  const dom = new JSDOM(reviewPage(), {
    url: `${base}/milestones/0.7`,
    runScripts: "dangerously",
    beforeParse(window) {
      window.fetch = (p, opts) => fetch(new URL(p, base), opts);
      window.EventSource = class {
        constructor() {
          es = this;
        }
      };
    },
  });
  t.after(() => dom.window.close());
  const doc = dom.window.document;
  es.onopen();
  await until("the page to load", () => ready(doc));
  const fire = (el, type) => el.dispatchEvent(new dom.window.Event(type, { bubbles: true }));
  return { doc, fire, nudge: () => es.onmessage({}) };
}

test("review page: leads with the guide's units, in order, before a collapsed Full detail that keeps every section", async (t) => {
  const { d, client, ids } = await daemonMilestone(t);
  const v = (await call(client, "get_milestone", { name: "0.7" })).value.milestone.version;
  assert.ok((await call(client, "set_review_guide", { name: "0.7", guide: guideFor(...ids), expected_version: v, ...o })).ok);
  const pg = await openReview(t, d.base, (doc) => doc.querySelector("#guide .unit"));
  const { doc } = pg;

  // The guide comes first, the detail after it, collapsed.
  const guide = doc.getElementById("guide");
  const detail = doc.getElementById("detail");
  assert.ok(detail, "no Full detail disclosure");
  assert.equal(detail.tagName, "DETAILS");
  assert.ok(guide.compareDocumentPosition(detail) & 4, "the guide is not before the detail");
  assert.equal(detail.open, false, "the detail starts open");
  assert.equal(doc.getElementById("noguide"), null);
  assert.deepEqual([...doc.querySelectorAll("#guide .unit .ttl")].map((x) => x.textContent), ["Druid in a root", "Flare readability"]);
  assert.deepEqual([...doc.querySelectorAll('#guide .unit[data-unit="druid-root"] .cards a')].map((x) => x.textContent), ids.slice(0, 2));
  assert.equal(doc.querySelectorAll("#guide input[data-check]").length, 3);
  assert.deepEqual([...doc.querySelectorAll("#guide button[data-replay]")].map((b) => b.dataset.replay), ["guide:druid-root:shift"], "one ▶ per check with a replay");
  // Every unit and every check takes a comment, beside its ▶.
  assert.deepEqual([...doc.querySelectorAll("#guide .ucomment [data-open]")].map((b) => b.dataset.open), ["guide:druid-root", "guide:flare"], "a unit without its comment box");
  assert.deepEqual(
    [...doc.querySelectorAll("#guide .ccomment [data-open]")].map((b) => b.dataset.open),
    ["guide:druid-root:shift", "guide:druid-root:no-loop", "guide:flare:flash"],
    "a check without its comment box",
  );
  // Nothing is removed: every per-card section is inside the detail; Feedback
  // and its Submit sit beside the guide, between it and the detail.
  assert.deepEqual([...detail.querySelectorAll("section > h2")].map((h) => h.textContent), ["What changed", "What to check", "Your decisions", "Balance", "Known gaps and follow-ups"]);
  assert.equal(guide.querySelector("section"), null);
  const fb = doc.getElementById("feedback");
  assert.ok(fb && !detail.contains(fb), "Feedback is folded inside the detail");
  assert.ok(guide.compareDocumentPosition(fb) & 4 && fb.compareDocumentPosition(detail) & 4, "Feedback is not between the guide and the detail");
  assert.ok(fb.querySelector("#submit"), "no Submit beside the guide");

  // A tick on a check persists through the same versioned route as a checklist step.
  const box = doc.querySelector('#guide input[data-check="guide:flare:flash"]');
  box.checked = true;
  pg.fire(box, "change");
  await until("the tick", () => doc.getElementById("guideprogress").textContent === "1 / 3 checked");
  assert.equal((await call(client, "get_milestone", { name: "0.7" })).value.guide.ticks["guide:flare:flash"].checked, true);

  // Opened once, the detail stays open across a redraw.
  detail.open = true;
  pg.fire(detail, "toggle");
  pg.nudge();
  await sleep(250);
  assert.equal(doc.getElementById("detail").open, true, "a redraw closed the detail");
});

test("review page: with no guide yet, the detail is the page, with a note saying so", async (t) => {
  const { d } = await daemonMilestone(t);
  const pg = await openReview(t, d.base, (doc) => doc.querySelector("#changed"));
  assert.match(pg.doc.getElementById("noguide").textContent, /No review guide has been written/);
  assert.equal(pg.doc.getElementById("guide"), null);
  assert.equal(pg.doc.getElementById("detail"), null, "the detail is folded away with nothing above it");
  assert.equal(pg.doc.querySelectorAll("section > h2").length, 6);
  assert.equal((await post(d.base, "/api/milestones/0.7/check", { key: "guide:flare:flash", checked: true, expected_version: 0 })).body.error, "invalid", "a tick on a guide that does not exist");
});
