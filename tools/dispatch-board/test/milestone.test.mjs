// Milestones: the entity, the Merged column, the structured card fields the
// review page is built from, the review's ticks/comments/submission, and the
// schema 2 export. test/migration.test.mjs covers getting an AS-153 board
// here; test/review.test.mjs covers the same flow over the daemon and pages.
import { test } from "node:test";
import assert from "node:assert/strict";
import { BoardError, checklistSteps } from "../dist/board.js";
import { tempBoard } from "./helpers.mjs";

const PR = (n) => ({ url: `https://github.com/o/r/pull/${n}` });
const SHA = (c) => c.repeat(40);

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

const o = { actor: "orchestrator" };

/** A card taken through the pipeline to review, Tester claim working: what mark_merged meets. */
function inReview(board, extra = {}, n = 1) {
  let c = board.createCard({ title: `card ${n}`, role: "engineer", ...extra }, o);
  c = board.moveCard(c.id, "in_progress", c.version, o);
  c = board.claimCard(c.id, `Engineer-${c.id}`, o);
  c = board.moveCard(c.id, "review", c.version, { ...o, patch: { pr: PR(n), agent: { ...c.agent, status: "done" } } });
  return board.claimCard(c.id, `${c.id}-test`, o);
}

function merged(board, extra = {}, n = 1, sha = SHA("a")) {
  const c = inReview(board, extra, n);
  return board.markMerged(c.id, { pr: PR(n), merge_sha: sha }, c.version, o);
}

function events(board, since = 0) {
  return board.eventsSince(since).events;
}

// ---------------------------------------------------------------- the entity

test("milestones: create, list and get; a name is unique and URL-safe", (t) => {
  const { board } = tempBoard(t);
  const m = board.createMilestone("0.7", o);
  assert.deepEqual([m.name, m.status, m.version, m.first_id, m.sweep, m.submissions], ["0.7", "open", 1, 1, null, []]);
  refused(() => board.createMilestone("0.7", o), "invalid", /already exists/);
  for (const bad of ["", "0 7", "/x", "-x", "x".repeat(33)]) refused(() => board.createMilestone(bad, o), "invalid");
  board.createMilestone("0.8", o);
  assert.deepEqual(board.listMilestones().map((x) => [x.name, x.status]), [["0.7", "open"], ["0.8", "open"]]);
  assert.equal(board.getMilestone("0.7").milestone.name, "0.7");
  refused(() => board.getMilestone("0.9"), "not_found");
  assert.deepEqual(events(board).map((e) => [e.kind, e.card, e.data.milestone]), [["milestone_created", null, "0.7"], ["milestone_created", null, "0.8"]]);
});

test("attach: a card joins an existing, unreleased milestone at iteration 1; list_cards filters by milestone and iteration", (t) => {
  const { board } = tempBoard(t);
  board.createMilestone("0.7", o);
  refused(() => board.createCard({ title: "x", role: "engineer", milestone: "0.9" }, o), "invalid", /no milestone 0\.9/);
  const a = board.createCard({ title: "a", role: "engineer", milestone: "0.7" }, o);
  assert.deepEqual([a.milestone, a.iteration], ["0.7", 1]);
  const b = board.createCard({ title: "b", role: "engineer", milestone: "0.7", iteration: 2 }, o);
  const none = board.createCard({ title: "c", role: "engineer" }, o);
  assert.equal("milestone" in none, false, "a card filed without milestone fields keeps the pre-milestone shape");

  // A card that gains a milestone by patch is iteration 1 too.
  const moved = board.updateCard(none.id, { milestone: "0.7" }, none.version, o);
  assert.equal(moved.iteration, 1);
  const off = board.updateCard(moved.id, { milestone: null }, moved.version, o);
  assert.equal(off.milestone, null);

  assert.deepEqual(board.listCards({ milestone: "0.7" }).map((c) => c.id), [a.id, b.id]);
  assert.deepEqual(board.listCards({ milestone: "0.7", iteration: 2 }).map((c) => c.id), [b.id]);
  assert.deepEqual(board.listCards({ milestone: null }).map((c) => c.id), [none.id]);
  const [s] = board.listCards({ milestone: "0.7", iteration: 1 });
  assert.deepEqual([s.milestone, s.iteration], ["0.7", 1], "summaries carry milestone and iteration");
  refused(() => board.updateCard(a.id, { iteration: 0 }, a.version, o), "invalid");
  refused(() => board.updateCard(a.id, { area: "sound" }, a.version, o), "invalid");
  refused(() => board.updateCard(a.id, { sweep: { status: "later" } }, a.version, o), "invalid");
});

test("attach: a released milestone takes no new cards, by create, patch or move", (t) => {
  const { board } = tempBoard(t);
  board.createMilestone("0.6", o);
  const m = board.closeMilestone("0.6", 1, o).milestone;
  assert.equal(m.status, "released");
  refused(() => board.createCard({ title: "x", role: "engineer", milestone: "0.6" }, o), "invalid", /released/);
  const c = board.createCard({ title: "y", role: "engineer" }, o);
  refused(() => board.updateCard(c.id, { milestone: "0.6" }, c.version, o), "invalid", /released/);
  refused(() => board.moveCard(c.id, "in_progress", c.version, { ...o, patch: { milestone: "0.6" } }), "invalid", /released/);
  assert.equal(board.getCard(c.id).milestone, undefined);
});

// ---------------------------------------------------------------- rulings

test("rulings: record_ruling appends dated entries with their numbers; a patch can never rewrite them", (t) => {
  const { board } = tempBoard(t);
  board.createMilestone("0.7", o);
  const c = board.createCard({ title: "trap", role: "engineer", milestone: "0.7" }, o);
  const head = board.head();
  const r1 = board.recordRuling(c.id, { text: "Traps target dispellers", numbers: { "win rate": "+36pt", z: 5.2 } }, { actor: "orchestrator", by: "user" });
  const r2 = board.recordRuling(c.id, { text: "No retune" }, { ...o, expected_version: r1.version });
  assert.deepEqual(r2.rulings.map((r) => [r.text, r.by, r.numbers]), [
    ["Traps target dispellers", "user", { "win rate": "+36pt", z: 5.2 }],
    ["No retune", "orchestrator", undefined],
  ]);
  assert.match(r2.rulings[0].t, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d$/);
  assert.equal(r2.activity.at(-1).msg, "Ruling recorded: No retune");
  assert.deepEqual(events(board, head).map((e) => [e.kind, e.card, e.data.milestone]), [["ruling", c.id, "0.7"], ["ruling", c.id, "0.7"]]);
  refused(() => board.recordRuling(c.id, { text: "stale" }, { ...o, expected_version: r1.version }), "stale_version");
  refused(() => board.recordRuling(c.id, { text: " " }, o), "invalid");
  refused(() => board.recordRuling(c.id, { text: "x", numbers: { z: Infinity } }, o), "invalid");
  refused(() => board.updateCard(c.id, { rulings: [] }, r2.version, o), "invalid", /not patchable/);
  refused(() => board.updateCard(c.id, { merge_sha: SHA("b") }, r2.version, o), "invalid", /not patchable/);
  assert.equal(board.getCard(c.id).rulings.length, 2);
});

// ---------------------------------------------------------------- mark_merged

test("mark_merged: review -> merged with pr, merge sha and the Tester claim closed, in one write", (t) => {
  const { board } = tempBoard(t);
  board.createMilestone("0.7", o);
  const c = inReview(board, { milestone: "0.7" }, 12);
  assert.equal(c.agent.status, "working");
  const head = board.head();
  const m = board.markMerged(c.id, { pr: PR(12), merge_sha: "ABCDEF1234567" }, c.version, { ...o, by: "tester", activity: "APPROVE: no findings" });
  assert.deepEqual([m.column, m.merge_sha, m.pr.number, m.agent.status, m.version], ["merged", "abcdef1234567", 12, "done", c.version + 1]);
  assert.ok(m.agent.finished && m.merged_at);
  assert.deepEqual(m.activity.slice(-3).map((a) => a.msg), ["APPROVE: no findings", "Merged: PR #12 at abcdef1234", `Claim finished (${c.id}-test)`]);
  const [ev] = events(board, head);
  assert.deepEqual([ev.kind, ev.card, ev.data], ["merged", c.id, { from: "review", to: "merged", pr: 12, merge_sha: "abcdef1234567", milestone: "0.7" }]);
});

test("mark_merged: only from review (or a merged card with no merge recorded), only the card's own PR, only a real sha", (t) => {
  const { board } = tempBoard(t);
  const c = inReview(board, {}, 3);
  refused(() => board.markMerged(c.id, { pr: PR(4), merge_sha: SHA("a") }, c.version, o), "invalid", /own PR is #3/);
  for (const sha of ["", "abc", "g".repeat(40), null]) refused(() => board.markMerged(c.id, { pr: PR(3), merge_sha: sha }, c.version, o), "invalid");
  refused(() => board.markMerged(c.id, { pr: null, merge_sha: SHA("a") }, c.version, o), "invalid");
  refused(() => board.markMerged(c.id, { pr: PR(3), merge_sha: SHA("a") }, c.version - 1, o), "stale_version");
  const m = board.markMerged(c.id, { pr: PR(3), merge_sha: SHA("a") }, c.version, o);
  refused(() => board.markMerged(m.id, { pr: PR(3), merge_sha: SHA("b") }, m.version, o), "invalid", /already recorded/);
  let early = board.createCard({ title: "x", role: "engineer" }, o);
  early = board.moveCard(early.id, "in_progress", early.version, o);
  refused(() => board.markMerged(early.id, { pr: PR(9), merge_sha: SHA("a") }, early.version, o), "invalid", /in in_progress/);
  assert.equal(board.getCard(early.id).column, "in_progress");
});

test("mark_merged: a merged card with no merge recorded (a migrated human_review card) gets it recorded", (t) => {
  const { board } = tempBoard(t);
  let c = board.createCard({ title: "legacy approved", role: "engineer" }, o);
  c = board.moveCard(c.id, "merged", c.version, { ...o, patch: { pr: PR(8) } });
  assert.equal(c.merge_sha, undefined);
  const m = board.markMerged(c.id, { pr: PR(8), merge_sha: SHA("c") }, c.version, o);
  assert.deepEqual([m.column, m.merge_sha], ["merged", SHA("c")]);
});

// ---------------------------------------------------------------- milestone writes

test("update_milestone and set_milestone_sweep are versioned; released is reached only by closing, and is final", (t) => {
  const { board } = tempBoard(t);
  let m = board.createMilestone("0.7", o);
  m = board.updateMilestone("0.7", { status: "in_review", review_sha: "ABCDEF1" }, m.version, o);
  assert.deepEqual([m.status, m.review_sha, m.version], ["in_review", "abcdef1", 2]);
  refused(() => board.updateMilestone("0.7", { tag: "v0.7.0" }, 1, o), "stale_version");
  refused(() => board.updateMilestone("0.7", { status: "released" }, m.version, o), "invalid", /close_milestone/);
  refused(() => board.updateMilestone("0.7", { name: "0.8" }, m.version, o), "invalid", /not patchable/);
  refused(() => board.updateMilestone("0.7", { release_url: "ftp://x" }, m.version, o), "invalid");
  m = board.setMilestoneSweep("0.7", { summary: "n=100 per cell: Hunter +4pt", link: "docs/design/balance/0.7.csv" }, m.version, o);
  assert.deepEqual([m.sweep.summary, m.sweep.link, m.sweep.by], ["n=100 per cell: Hunter +4pt", "docs/design/balance/0.7.csv", "orchestrator"]);
  refused(() => board.setMilestoneSweep("0.7", { summary: "again" }, m.version - 1, o), "stale_version");
  const closed = board.closeMilestone("0.7", m.version, { ...o, tag: "v0.7.0" }).milestone;
  assert.deepEqual([closed.status, closed.tag], ["released", "v0.7.0"]);
  refused(() => board.updateMilestone("0.7", { status: "open" }, closed.version, o), "invalid", /final/);
  refused(() => board.closeMilestone("0.7", closed.version, o), "invalid", /already released/);
  // The tag and URL may still be recorded after the release.
  assert.equal(board.updateMilestone("0.7", { release_url: "https://github.com/o/r/releases/tag/v0.7.0" }, closed.version, o).release_url, "https://github.com/o/r/releases/tag/v0.7.0");
});

test("close_milestone: refused while a card is unfinished; then every merged card moves to done in one write", (t) => {
  const { board } = tempBoard(t);
  let m = board.createMilestone("0.7", o);
  const a = merged(board, { milestone: "0.7", summary: "A", area: "combat" }, 1);
  const b = merged(board, { milestone: "0.7", summary: "B", area: "ui" }, 2);
  const late = board.createCard({ title: "iteration 2", role: "engineer", milestone: "0.7", iteration: 2 }, o);
  const other = merged(board, {}, 3); // on no milestone: untouched
  const err = refused(() => board.closeMilestone("0.7", m.version, o), "invalid", new RegExp(`${late.id} \\(backlog\\)`));
  assert.equal(err.current.version, m.version);
  assert.deepEqual([a.id, b.id].map((id) => board.getCard(id).column), ["merged", "merged"], "a refused close moves nothing");

  board.updateCard(late.id, { milestone: null }, late.version, o);
  const head = board.head();
  const r = board.closeMilestone("0.7", m.version, o);
  assert.equal(r.milestone.status, "released");
  assert.ok(r.milestone.released_at);
  assert.deepEqual(r.cards.map((c) => [c.id, c.summary, c.area, c.pr.number, c.merge_sha]), [
    [a.id, "A", "combat", 1, SHA("a")],
    [b.id, "B", "ui", 2, SHA("a")],
  ]);
  for (const c of [a, b]) {
    const now = board.getCard(c.id);
    assert.deepEqual([now.column, now.version], ["done", c.version + 1]);
    assert.equal(now.activity.at(-1).msg, "Milestone 0.7 closed: merged → done");
  }
  assert.equal(board.getCard(other.id).column, "merged");
  assert.deepEqual(events(board, head).map((e) => [e.kind, e.card]), [["moved", a.id], ["moved", b.id], ["milestone_closed", null]]);
  assert.deepEqual(events(board, head).at(-1).data, { milestone: "0.7", cards: [a.id, b.id] });

  // Follow-ups are the cards filed while it was open — not after it closed.
  const afterClose = board.createCard({ title: "filed after the release", role: "engineer" }, o);
  const followups = board.getMilestone("0.7").gaps.followups.map((c) => c.id);
  assert.deepEqual(followups, [late.id, other.id]);
  assert.ok(!followups.includes(afterClose.id));
});

// ---------------------------------------------------------------- the review payload

test("checklist steps: one per line, the PR lead-in and list markers stripped, keyed by their own text", () => {
  const steps = checklistSteps("AS-7", "**Human testing:** Watch a trap spring on the healer.\n\n- Check the ice reads cleanly.\n2. Nothing else.\n- Check the ice reads cleanly.");
  assert.deepEqual(steps.map((s) => s.text), ["Watch a trap spring on the healer.", "Check the ice reads cleanly.", "Nothing else."]);
  assert.ok(steps.every((s) => /^AS-7:[0-9a-f]{10}$/.test(s.key)));
  const again = checklistSteps("AS-7", "* Check the ice reads cleanly.");
  assert.equal(again[0].key, steps[1].key, "the same step keeps its key wherever it moves");
  assert.notEqual(checklistSteps("AS-7", "Check the ice reads well.")[0].key, steps[1].key, "a reworded step is a new step");
  assert.deepEqual(checklistSteps("AS-7", null), []);
});

test("review payload: assembled from the cards' structured fields, section by section", (t) => {
  const { board } = tempBoard(t);
  const before = board.createCard({ title: "filed before the milestone", role: "engineer" }, o);
  board.createMilestone("0.7", o);
  const vis = merged(board, { milestone: "0.7", area: "visuals", summary: "Traps glow", human_testing: "- Watch the glow\n- Check dark mode", sweep: { status: "none" } }, 1, SHA("1"));
  const cmb = merged(board, { milestone: "0.7", area: "combat", summary: "Warriors start at 0 rage", human_testing: "Nothing needs human testing.", sweep: { status: "deferred-to-milestone", summary: "rage start shifts 1v1" } }, 2, SHA("2"));
  const bare = merged(board, { milestone: "0.7", gaps: "Mage untouched" }, 3, SHA("3"));
  board.recordRuling(cmb.id, { text: "Start at 0, no retune", numbers: { n: 12 } }, { ...o, by: "user" });
  board.recordRuling(vis.id, { text: "Glow stays" }, { ...o, by: "user" });
  const flight = board.createCard({ title: "iteration 2 fix", role: "engineer", milestone: "0.7", iteration: 2 }, o);
  const follow = board.createCard({ title: "a follow-up found meanwhile", role: "engineer" }, o);

  const p = board.getMilestone("0.7");
  assert.deepEqual(p.what_changed.map((g) => [g.area, g.cards.map((c) => c.id)]), [["combat", [cmb.id]], ["visuals", [vis.id]], [null, [bare.id]]], "areas in the fixed order, unassigned last");
  assert.equal(p.what_changed[0].cards[0].summary, "Warriors start at 0 rage");
  assert.deepEqual(p.in_flight.map((c) => c.id), [flight.id]);

  const steps = p.checklist.groups.flatMap((g) => g.cards.flatMap((c) => c.steps.map((s) => [c.id, s.text, s.checked, s.version])));
  assert.deepEqual(steps, [
    [cmb.id, "Nothing needs human testing.", false, 0],
    [vis.id, "Watch the glow", false, 0],
    [vis.id, "Check dark mode", false, 0],
  ]);
  assert.deepEqual(p.checklist.without_steps.map((c) => c.id), [bare.id], "a merged card with no steps is named, not silently absent");
  assert.deepEqual(p.checklist.applies_to.newest_merge.card, bare.id);
  assert.equal(p.checklist.applies_to.review_sha, null);

  // Recorded in one second here, so card order decides (vis was filed first).
  assert.deepEqual(p.decisions.map((d) => [d.card, d.text, d.by, d.numbers]), [
    [vis.id, "Glow stays", "user", undefined],
    [cmb.id, "Start at 0, no retune", "user", { n: 12 }],
  ]);
  assert.ok(p.decisions.every((d, i) => i === 0 || p.decisions[i - 1].t <= d.t), "oldest first");
  assert.deepEqual(p.balance.deferred, [{ id: cmb.id, title: cmb.title, note: "rage start shifts 1v1" }]);
  assert.deepEqual(p.balance.unstated.map((c) => c.id), [bare.id]);
  assert.equal(p.balance.sweep, null);
  assert.deepEqual(p.gaps.stated.map((g) => [g.id, g.gaps]), [[bare.id, "Mage untouched"]]);
  assert.deepEqual(p.gaps.followups.map((c) => c.id), [follow.id], "cards filed while it was open, not before, not its own");
  assert.ok(!p.gaps.followups.some((c) => c.id === before.id));
});

// ---------------------------------------------------------------- ticks, comments, submission

function reviewFixture(board) {
  board.createMilestone("0.7", o);
  const a = merged(board, { milestone: "0.7", area: "combat", summary: "A", human_testing: "- Step one\n- Step two" }, 1);
  const b = merged(board, { milestone: "0.7", area: "ui", summary: "B", human_testing: "Look at it", iteration: 2 }, 2);
  const p = board.getMilestone("0.7");
  const keys = p.checklist.groups.flatMap((g) => g.cards.flatMap((c) => c.steps.map((s) => s.key)));
  return { a, b, keys };
}

test("ticks: versioned per step, persisted, never a board event; a reworded step comes back unticked", (t) => {
  const { board } = tempBoard(t);
  const { a, keys } = reviewFixture(board);
  const nudges = [];
  board.on("nudge", (n) => nudges.push(n));
  const head = board.head();
  const it = board.setReviewCheck("0.7", keys[0], true, 0, { actor: "board" });
  assert.deepEqual([it.version, it.checked_by], [1, "board"]);
  refused(() => board.setReviewCheck("0.7", keys[0], false, 0, { actor: "board" }), "stale_version");
  board.setReviewCheck("0.7", keys[1], true, 0, { actor: "board" });
  board.setReviewCheck("0.7", keys[1], false, 1, { actor: "board" });
  const steps = board.getMilestone("0.7").checklist.groups.flatMap((g) => g.cards.flatMap((c) => c.steps));
  assert.deepEqual(steps.map((s) => [s.checked, s.version]), [[true, 1], [false, 2], [false, 0]]);
  assert.equal(board.head(), head, "a tick is not a board event: the orchestrator's wait never wakes for one");
  assert.equal(nudges.length, 3, "but open pages are nudged");

  refused(() => board.setReviewCheck("0.7", "check:AS-1:0000000000", true, 0, { actor: "board" }), "invalid", /not a current checklist step/);
  refused(() => board.setReviewCheck("0.7", "milestone", true, 0, { actor: "board" }), "invalid");
  board.updateCard(a.id, { human_testing: "- Step one, reworded\n- Step two" }, board.getCard(a.id).version, o);
  const after = board.getMilestone("0.7").checklist.groups[0].cards[0].steps;
  assert.deepEqual(after.map((s) => [s.text, s.checked]), [["Step one, reworded", false], ["Step two", false]]);
});

test("submit: each comment becomes a card on the milestone at its source's iteration + 1, linked back; drafts clear; one event", (t) => {
  const { board } = tempBoard(t);
  const { a, b, keys } = reviewFixture(board);
  const u = { actor: "board" };
  board.saveReviewDraft("0.7", `card:${a.id}`, "The ice is too bright", 0, u);
  board.saveReviewDraft("0.7", keys[1], "Step two did nothing\nsecond line", 0, u);
  board.saveReviewDraft("0.7", `card:${b.id}`, "Tooltip clips", 0, u);
  board.saveReviewDraft("0.7", "milestone", "Good milestone overall; do the sweep", 0, u);
  board.saveReviewDraft("0.7", "milestone", "Good milestone; do the sweep next", 1, u);
  refused(() => board.saveReviewDraft("0.7", "milestone", "stale", 1, u), "stale_version");
  refused(() => board.saveReviewDraft("0.7", "card:AS-99", "not on it", 0, u), "invalid", /not a review item/);
  const head = board.head();
  const drafts = board.getMilestone("0.7").feedback.drafts;
  assert.equal(Object.keys(drafts).length, 4);
  assert.equal(board.head(), head, "saving a draft is not a board event");

  const expected = Object.fromEntries(Object.entries(drafts).map(([k, d]) => [k, d.version]));
  const r = board.submitReview("0.7", expected, u);
  assert.equal(r.cards.length, 4);
  const bySource = Object.fromEntries(r.cards.map((c) => [c.source.key, c]));
  const onA = bySource[`card:${a.id}`];
  assert.deepEqual([onA.milestone, onA.iteration, onA.column, onA.role, onA.source.card], ["0.7", 2, "backlog", "engineer", a.id]);
  assert.match(onA.title, new RegExp(`^Review feedback on ${a.id}: The ice is too bright$`));
  assert.match(onA.body, /The ice is too bright/);
  assert.deepEqual(onA.links, [{ label: `${a.id} PR #1 (feedback source)`, url: PR(1).url }], "linked to the source's PR as a reference, never as its own pr");
  assert.equal(onA.pr, null);
  const onStep = bySource[keys[1]];
  assert.deepEqual([onStep.iteration, onStep.source.card], [2, a.id]);
  assert.match(onStep.body, /checklist step "Step two"/);
  assert.equal(bySource[`card:${b.id}`].iteration, 3, "one after its source's own iteration");
  assert.equal(bySource.milestone.iteration, 3, "milestone-level feedback: one after the milestone's latest iteration");
  assert.equal(bySource.milestone.source.card, null);
  assert.match(board.getCard(a.id).activity.at(-1).msg, new RegExp(`review feedback filed as ${bySource[keys[1]].id}`));

  const after = board.getMilestone("0.7");
  assert.deepEqual(after.feedback.drafts, {}, "submitted comments are cleared");
  assert.deepEqual(after.feedback.submissions.map((s) => s.cards), [r.cards.map((c) => c.id)]);
  assert.deepEqual(after.in_flight.map((c) => c.id), r.cards.map((c) => c.id));
  const evs = events(board, head);
  assert.deepEqual(evs.map((e) => e.kind), ["created", "created", "created", "created", "review_submitted"]);
  assert.deepEqual(evs[0].data, { column: "backlog", title: r.cards[0].title, milestone: "0.7", iteration: r.cards[0].iteration });
  assert.deepEqual(evs.at(-1).data, { milestone: "0.7", cards: r.cards.map((c) => c.id) });
  assert.equal(evs.at(-1).actor, "board");
});

test("submit: a comment changed or added since the user read it refuses the whole submission, filing nothing", (t) => {
  const { board } = tempBoard(t);
  const { a, keys } = reviewFixture(board);
  const u = { actor: "board" };
  board.saveReviewDraft("0.7", `card:${a.id}`, "one", 0, u);
  const seen = { [`card:${a.id}`]: 1 };
  board.saveReviewDraft("0.7", keys[0], "added in another window", 0, u);
  const before = board.listCards({ include_archived: true }).length;
  const err = refused(() => board.submitReview("0.7", seen, u), "stale_version", new RegExp(keys[0]));
  assert.ok(err.current.drafts[keys[0]], "the refusal carries the drafts as they are now");
  board.saveReviewDraft("0.7", `card:${a.id}`, "one, edited", 1, u);
  refused(() => board.submitReview("0.7", { [`card:${a.id}`]: 1, [keys[0]]: 1 }, u), "stale_version");
  assert.equal(board.listCards({ include_archived: true }).length, before);
  assert.equal(board.submitReview("0.7", { [`card:${a.id}`]: 2, [keys[0]]: 1 }, u).cards.length, 2);
});

test("submit: no comments records a submission with no cards; a released milestone's review is closed", (t) => {
  const { board } = tempBoard(t);
  const { keys } = reviewFixture(board);
  const head = board.head();
  const r = board.submitReview("0.7", {}, { actor: "board" });
  assert.deepEqual(r.cards, []);
  assert.deepEqual(r.milestone.submissions.map((s) => s.cards), [[]]);
  assert.deepEqual(events(board, head).map((e) => [e.kind, e.data]), [["review_submitted", { milestone: "0.7", cards: [] }]]);
  board.closeMilestone("0.7", r.milestone.version, o);
  refused(() => board.submitReview("0.7", {}, { actor: "board" }), "invalid", /released/);
  refused(() => board.setReviewCheck("0.7", keys[0], true, 0, { actor: "board" }), "invalid", /released/);
  refused(() => board.saveReviewDraft("0.7", "milestone", "late", 0, { actor: "board" }), "invalid", /released/);
});

test("submit: a comment whose step was reworded away still files against its card, and can be cleared instead", (t) => {
  const { board } = tempBoard(t);
  const { a, keys } = reviewFixture(board);
  const u = { actor: "board" };
  board.saveReviewDraft("0.7", keys[0], "orphan me", 0, u);
  board.updateCard(a.id, { human_testing: "- Something else entirely" }, board.getCard(a.id).version, o);
  refused(() => board.saveReviewDraft("0.7", keys[0], "edit it", 1, u), "invalid", /not a review item/);
  const r = board.submitReview("0.7", { [keys[0]]: 1 }, u);
  assert.deepEqual([r.cards[0].source.card, r.cards[0].iteration], [a.id, 2]);

  board.saveReviewDraft("0.7", `card:${a.id}`, "moved off later", 0, u);
  board.updateCard(a.id, { milestone: null }, board.getCard(a.id).version, o);
  refused(() => board.submitReview("0.7", { [`card:${a.id}`]: 1 }, u), "invalid", /no longer names an item/);
  board.saveReviewDraft("0.7", `card:${a.id}`, "", 1, u);
  assert.deepEqual(board.submitReview("0.7", {}, u).cards, []);
});

// ---------------------------------------------------------------- export / import

test("schema 2 round trip: milestones, their ticks, drafts and submissions, and every card field survive exactly", (t) => {
  const { board } = tempBoard(t);
  const { a, keys } = reviewFixture(board);
  board.recordRuling(a.id, { text: "keep", numbers: { x: 1 } }, o);
  board.setReviewCheck("0.7", keys[0], true, 0, { actor: "board" });
  board.saveReviewDraft("0.7", keys[1], "draft kept", 0, { actor: "board" });
  board.submitReview("0.7", { [keys[1]]: 1 }, { actor: "board" });
  board.saveReviewDraft("0.7", "milestone", "unsubmitted", 0, { actor: "board" });
  board.createMilestone("0.8", o);
  const out = board.exportState();
  assert.equal(out.schema, 2);
  assert.deepEqual(out.milestones.map((m) => [m.name, m.review.length]), [["0.7", 3], ["0.8", 0]]);

  const { board: b2 } = tempBoard(t);
  const r = b2.importState(structuredClone(out), { actor: "import" });
  assert.deepEqual([r.milestones, r.migration.human_review_to_merged], [2, []]);
  assert.equal(JSON.stringify(b2.exportState()), JSON.stringify(out));
  const p = b2.getMilestone("0.7");
  assert.equal(p.checklist.groups[0].cards[0].steps[0].checked, true);
  assert.equal(p.feedback.drafts.milestone.text, "unsubmitted");
});

test("schema 2 import: refuses a card naming a milestone the state lacks, and a schema 1 human_review card arrives merged", (t) => {
  const { board } = tempBoard(t);
  const card = { id: "AS-1", title: "t", body: "", column: "backlog", role: "engineer", priority: "P2", links: [], pr: null, worktree: null, question: null, agent: null, created: "c", updated: "u", activity: [] };
  refused(() => board.importState({ schema: 2, nextId: 2, cards: [{ ...card, milestone: "0.7" }], milestones: [] }, { actor: "import" }), "invalid", /not in the state's milestones/);
  refused(() => board.importState({ schema: 2, nextId: 2, cards: [{ ...card, column: "human_review" }], milestones: [] }, { actor: "import" }), "invalid", /column must be one of/);
  refused(() => board.importState({ schema: 3, nextId: 2, cards: [] }, { actor: "import" }), "invalid", /unsupported schema/);
  const r = board.importState(
    { schema: 1, nextId: 3, cards: [{ ...card, column: "human_review", pr: PR(5) }, { ...card, id: "AS-2", column: "done" }] },
    { actor: "import" },
  );
  assert.deepEqual(r.migration.human_review_to_merged, ["AS-1"]);
  const c = board.getCard("AS-1");
  assert.deepEqual([c.column, c.merge_sha, c.milestone, c.activity.at(-1).by], ["merged", undefined, undefined, "migrate"]);
  assert.equal(board.getCard("AS-2").column, "done");
  assert.equal(board.markMerged("AS-1", { pr: PR(5), merge_sha: SHA("d") }, c.version, o).merge_sha, SHA("d"));
});
