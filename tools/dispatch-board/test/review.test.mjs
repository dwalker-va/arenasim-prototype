// The milestone flow over the real daemon: the orchestrator's MCP tools, the
// event feed `wait` reads, and the two pages (the board's milestone filter
// and drawer fields, and the review page) driven in jsdom against it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { spawnDaemon, mcpClient, call, post, runWait, sleep } from "./helpers.mjs";

const { uiPage, reviewPage } = await import("../dist/server.js");
const PR = (n) => ({ url: `https://github.com/o/r/pull/${n}` });
const orch = { actor: "orchestrator" };

async function until(what, pred, ms = 5000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await pred();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(20);
  }
}

/** A work card taken to review under a Tester claim, then merged — all through MCP, as the orchestrator does it. */
async function mergedCard(client, fields, n) {
  let c = (await call(client, "create_card", { title: `card ${n}`, role: "engineer", ...fields, ...orch })).value;
  c = (await call(client, "move_card", { id: c.id, column: "in_progress", expected_version: c.version, ...orch })).value;
  c = (await call(client, "claim_card", { id: c.id, name: `Engineer-${c.id}`, ...orch })).value;
  c = (await call(client, "move_card", { id: c.id, column: "review", expected_version: c.version, patch: { pr: PR(n), agent: { ...c.agent, status: "done" } }, ...orch })).value;
  c = (await call(client, "claim_card", { id: c.id, name: `${c.id}-test`, ...orch })).value;
  const m = await call(client, "mark_merged", { id: c.id, pr: PR(n), merge_sha: String(n).repeat(40).slice(0, 40), expected_version: c.version, ...orch });
  assert.ok(m.ok, JSON.stringify(m.value));
  return m.value;
}

async function milestoneWithCards(d) {
  const client = await mcpClient(d.t, d.base);
  assert.ok((await call(client, "create_milestone", { name: "0.7", ...orch })).ok);
  const a = await mergedCard(client, { milestone: "0.7", area: "combat", summary: "Warriors start at 0 rage", human_testing: "- Start a Warrior match\n- Watch the rage bar", sweep: { status: "deferred-to-milestone" } }, 1);
  const b = await mergedCard(client, { milestone: "0.7", area: "visuals", summary: "Traps glow", human_testing: "Look at a trap" }, 2);
  assert.ok((await call(client, "record_ruling", { id: a.id, text: "No retune", numbers: { n: 12 }, ...orch, by: "user" })).ok);
  return { client, a, b };
}

test("MCP: the orchestrator's milestone flow, create to close, with refusals as tool errors", async (t) => {
  const d = await spawnDaemon(t);
  d.t = t;
  const { client, a, b } = await milestoneWithCards(d);
  assert.deepEqual([a.column, a.milestone, a.iteration, a.merge_sha.length], ["merged", "0.7", 1, 40]);

  const list = (await call(client, "list_milestones", {})).value;
  assert.deepEqual(list.map((m) => [m.name, m.status, m.cards]), [["0.7", "open", { merged: 2 }]]);
  let m = (await call(client, "get_milestone", { name: "0.7" })).value;
  assert.deepEqual(m.what_changed.map((g) => g.area), ["combat", "visuals"]);
  assert.deepEqual(m.decisions.map((x) => [x.card, x.text, x.numbers]), [[a.id, "No retune", { n: 12 }]]);
  assert.deepEqual(m.balance.deferred.map((x) => x.id), [a.id]);

  const stale = await call(client, "update_milestone", { name: "0.7", patch: { status: "in_review" }, expected_version: 0, ...orch });
  assert.equal(stale.ok, false);
  assert.equal(stale.value.error, "stale_version");
  assert.equal(stale.value.current.version, 1, "the refusal carries the milestone as it is now");

  let ms = (await call(client, "update_milestone", { name: "0.7", patch: { status: "in_review", review_sha: "1".repeat(40) }, expected_version: 1, ...orch })).value;
  ms = (await call(client, "set_milestone_sweep", { name: "0.7", summary: "no class moved more than 3pt", link: "docs/design/balance/0.7.md", expected_version: ms.version, ...orch })).value;
  const summaries = (await call(client, "list_cards", { milestone: "0.7" })).value;
  assert.deepEqual(summaries.map((c) => [c.id, c.milestone, c.iteration, c.area]), [[a.id, "0.7", 1, "combat"], [b.id, "0.7", 1, "visuals"]]);

  // A stale close is a tool error that changes nothing.
  const head = (await (await fetch(`${d.base}/api/health`)).json()).head;
  const staleClose = await call(client, "close_milestone", { name: "0.7", expected_version: ms.version - 1, ...orch });
  assert.equal(staleClose.ok, false);
  assert.equal(staleClose.value.error, "stale_version");
  assert.deepEqual((await call(client, "list_cards", { milestone: "0.7" })).value.map((c) => c.column), ["merged", "merged"], "no card moved merged -> done");
  assert.equal((await call(client, "get_milestone", { name: "0.7" })).value.milestone.status, "in_review", "the milestone was not released");
  assert.deepEqual((await call(client, "events_since", { cursor: head })).value.events, [], "no moved or milestone_closed event");

  const closed = await call(client, "close_milestone", { name: "0.7", expected_version: ms.version, tag: "v0.7.0", ...orch });
  assert.ok(closed.ok, JSON.stringify(closed.value));
  assert.deepEqual(closed.value.cards.map((c) => [c.id, c.summary]), [[a.id, "Warriors start at 0 rage"], [b.id, "Traps glow"]]);
  assert.deepEqual((await call(client, "list_cards", { column: "done" })).value.map((c) => c.id), [a.id, b.id]);
  const late = await call(client, "create_card", { title: "too late", role: "engineer", milestone: "0.7", ...orch });
  assert.equal(late.value.error, "invalid");
});

test("wake-up: the new event kinds reach `wait` in the existing line shape; ticks and drafts do not wake it, a submission does", async (t) => {
  const d = await spawnDaemon(t);
  d.t = t;
  const w = runWait(["--follow", "--since", "0", "--port", String(d.port)]);
  t.after(() => w.child.kill());
  const { client, a } = await milestoneWithCards(d);
  const kinds = () => w.stdoutSoFar().split("\n").filter(Boolean).map((l) => JSON.parse(l));
  await until("the milestone events", () => kinds().some((e) => e.kind === "ruling"));
  const seen = kinds();
  for (const e of seen) assert.deepEqual(Object.keys(e), ["cursor", "t", "actor", "kind", "card", "data"], "every line keeps the shape existing consumers parse");
  assert.deepEqual(seen.filter((e) => ["milestone_created", "merged", "ruling"].includes(e.kind)).map((e) => [e.kind, e.card]), [
    ["milestone_created", null],
    ["merged", a.id],
    ["merged", seen.find((e) => e.kind === "merged" && e.card !== a.id).card],
    ["ruling", a.id],
  ]);

  // The orchestrator's monitor ignores its own writes; the user's review gestures:
  const head = (await (await fetch(`${d.base}/api/health`)).json()).head;
  const one = runWait(["--since", String(head), "--ignore-actor", "orchestrator", "--port", String(d.port)]);
  const p = await (await fetch(`${d.base}/api/milestones/0.7`)).json();
  const key = p.checklist.groups[0].cards[0].steps[0].key;
  assert.equal((await post(d.base, "/api/milestones/0.7/check", { key, checked: true, expected_version: 0 })).status, 200);
  assert.equal((await post(d.base, "/api/milestones/0.7/feedback", { key: `card:${a.id}`, text: "rage bar flickers", expected_version: 0 })).status, 200);
  await sleep(400);
  assert.equal(one.stdoutSoFar(), "", "a tick or a draft woke the orchestrator");
  assert.equal(one.child.exitCode, null);
  const sub = await post(d.base, "/api/milestones/0.7/submit", { expected: { [`card:${a.id}`]: 1 } });
  assert.equal(sub.status, 200, JSON.stringify(sub.body));
  const r = await one.done;
  // The new cards and the submission commit in one write, so they wake the waiter as one batch.
  assert.deepEqual(r.lines.map((l) => [l.actor, l.kind]), [["board", "created"], ["board", "review_submitted"]]);
  const all = (await call(client, "events_since", { cursor: head, ignore_actors: ["orchestrator"] })).value.events;
  assert.deepEqual(all.map((e) => [e.kind, e.data.milestone]), [["created", "0.7"], ["review_submitted", "0.7"]]);
  assert.deepEqual(all[1].data.cards, [sub.body.cards[0].id]);
});

test("web UI API: the review page, theme and milestone endpoints are served; the same guards hold", async (t) => {
  const d = await spawnDaemon(t);
  d.t = t;
  const { a } = await milestoneWithCards(d);
  const page = await fetch(`${d.base}/milestones/0.7`);
  assert.equal(page.status, 200);
  assert.match(page.headers.get("content-type"), /text\/html/);
  assert.match(await page.text(), /<title>Milestone Review<\/title>/);
  const css = await fetch(`${d.base}/theme.css`);
  assert.match(css.headers.get("content-type"), /text\/css/);
  assert.match(await css.text(), /--c-merged/);
  const board = await (await fetch(`${d.base}/api/board`)).json();
  assert.deepEqual(board.milestones.map((m) => m.name), ["0.7"]);
  assert.equal(board.cards.find((c) => c.id === a.id).merge_sha.length, 40, "the board's summaries carry the merge sha");

  assert.equal((await post(d.base, "/api/milestones", { name: "0.8" })).status, 200);
  assert.equal((await post(d.base, "/api/milestones", { name: "0.8" })).body.error, "invalid");
  assert.equal((await fetch(`${d.base}/api/milestones/0.9`)).status, 404);
  const tick = await post(d.base, "/api/milestones/0.7/check", { key: "check:AS-1:0000000000", checked: true, expected_version: 0 });
  assert.equal(tick.body.error, "invalid");
  const noVersion = await post(d.base, "/api/milestones/0.7/feedback", { key: "milestone", text: "x" });
  assert.equal(noVersion.body.error, "invalid");
  const ruling = await post(d.base, `/api/cards/${a.id}/ruling`, { text: "Recorded from the drawer" });
  assert.equal(ruling.status, 200);
  assert.deepEqual(ruling.body.rulings.map((r) => [r.text, r.by]), [["No retune", "user"], ["Recorded from the drawer", "board"]]);
});

// ---------------------------------------------------------------- the pages, in jsdom

/** Open a page over the daemon; `nudge()` fires the SSE message by hand, so WHEN the page refreshes is the test's call. */
async function openDom(t, base, html, path, ready) {
  let es;
  const dom = new JSDOM(html, {
    url: `${base}${path}`,
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
  return { dom, doc, fire, nudge: () => es.onmessage({}), status: () => doc.querySelector(".hdr .stat").textContent };
}

test("review page: ticks persist, a comment saves as it is typed, and submitting files an iteration-2 card", async (t) => {
  const d = await spawnDaemon(t);
  d.t = t;
  const { client, a, b } = await milestoneWithCards(d);
  const pg = await openDom(t, d.base, reviewPage(), "/milestones/0.7", (doc) => doc.querySelector("#check input[type=checkbox]"));
  const { doc } = pg;

  // Every section is drawn from the payload.
  assert.deepEqual([...doc.querySelectorAll("section > h2")].map((h) => h.textContent), ["What changed", "What to check", "Your decisions", "Balance", "Known gaps and follow-ups", "Feedback"]);
  assert.match(doc.querySelector("#changed").textContent, /Warriors start at 0 rage[\s\S]*Traps glow/);
  assert.match(doc.querySelector("#check .applies").textContent, /main at or after 2222222222 \(the newest merge, AS-2\)/);
  assert.match(doc.querySelector("#decisions").textContent, /No retune/);
  assert.match(doc.querySelector("#balance").textContent, /Deferred to this sweep/);
  assert.equal(doc.getElementById("progress").textContent, "0 / 3 checked");

  // Tick the first step: persisted on the server, and the page redraws from it.
  const box = doc.querySelector("#check input[type=checkbox]");
  box.checked = true;
  pg.fire(box, "change");
  await until("the tick to land", () => doc.getElementById("progress").textContent === "1 / 3 checked");
  let p = (await call(client, "get_milestone", { name: "0.7" })).value;
  assert.equal(p.checklist.groups[0].cards[0].steps[0].checked, true);
  // ...and unticked again, which needs the version the first tick produced.
  const again = doc.querySelector("#check input[type=checkbox]");
  again.checked = false;
  pg.fire(again, "change");
  await until("the untick to land", () => doc.getElementById("progress").textContent === "0 / 3 checked");

  // Comment on card A: open the box, type, and it saves without a button.
  doc.querySelector(`[data-open="card:${a.id}"]`).click();
  const ta = [...doc.querySelectorAll("textarea[data-key]")].find((x) => x.dataset.key === `card:${a.id}`);
  ta.value = "The rage bar flickers at 0";
  pg.fire(ta, "input");
  pg.fire(ta, "blur");
  await until("the draft to save", async () => (await call(client, "get_milestone", { name: "0.7" })).value.feedback.drafts[`card:${a.id}`]);
  // A whole-milestone comment too.
  const whole = doc.querySelector('[data-open="milestone"]');
  whole.click();
  const wt = [...doc.querySelectorAll("textarea[data-key]")].find((x) => x.dataset.key === "milestone");
  wt.value = "Ship it after the rage fix";
  pg.fire(wt, "input");

  // Submit: the first click asks, the second files — flushing the unsaved comment first.
  doc.getElementById("submit").click();
  assert.match(doc.getElementById("submit").textContent, /Confirm: file 2 cards/);
  doc.getElementById("submit").click();
  await until("the submission", () => /submitted — filed/.test(pg.status()));
  const filed = (await call(client, "list_cards", { milestone: "0.7", iteration: 2 })).value;
  assert.equal(filed.length, 2);
  const full = await Promise.all(filed.map(async (c) => (await call(client, "get_card", { id: c.id })).value));
  assert.deepEqual(full.map((c) => c.source.key).sort(), [`card:${a.id}`, "milestone"]);
  assert.match(full.find((c) => c.source.key === `card:${a.id}`).body, /The rage bar flickers at 0/);
  p = (await call(client, "get_milestone", { name: "0.7" })).value;
  assert.deepEqual(p.feedback.drafts, {});
  assert.deepEqual(p.feedback.submissions[0].cards.sort(), filed.map((c) => c.id).sort());
  await until("the page to list the new cards in flight", () => /Still in flight/.test(doc.querySelector("#changed").textContent));
  assert.ok(b);
});

test("review page: a comment changed in another window is not overwritten — the page asks", async (t) => {
  const d = await spawnDaemon(t);
  d.t = t;
  await milestoneWithCards(d);
  const pg = await openDom(t, d.base, reviewPage(), "/milestones/0.7", (doc) => doc.querySelector('[data-open="milestone"]'));
  const { doc } = pg;
  doc.querySelector('[data-open="milestone"]').click();
  // Another window saves first.
  assert.equal((await post(d.base, "/api/milestones/0.7/feedback", { key: "milestone", text: "theirs", expected_version: 0 })).status, 200);
  const ta = [...doc.querySelectorAll("textarea[data-key]")].find((x) => x.dataset.key === "milestone");
  ta.value = "mine";
  pg.fire(ta, "input");
  pg.fire(ta, "blur");
  await until("the conflict notice", () => doc.querySelector(".conflict"));
  assert.match(doc.querySelector(".conflict").textContent, /theirs/);
  let p = await (await fetch(`${d.base}/api/milestones/0.7`)).json();
  assert.equal(p.feedback.drafts.milestone.text, "theirs", "the other window's comment was not overwritten");
  doc.querySelector("[data-keep]").click();
  await until("mine to be saved over it, on purpose", async () => (await (await fetch(`${d.base}/api/milestones/0.7`)).json()).feedback.drafts.milestone.text === "mine");
  p = await (await fetch(`${d.base}/api/milestones/0.7`)).json();
  assert.equal(p.feedback.drafts.milestone.version, 2);
});

test("board page: the milestone filter, the Merged column, a card's milestone fields and a new milestone", async (t) => {
  const d = await spawnDaemon(t);
  d.t = t;
  const { client, a } = await milestoneWithCards(d);
  const loose = (await call(client, "create_card", { title: "no milestone", role: "engineer", ...orch })).value;
  const pg = await openDom(t, d.base, uiPage(), "/?m=0.7", (doc) => doc.querySelector(".card"));
  const { doc } = pg;
  const ids = () => [...doc.querySelectorAll(".card")].map((c) => c.dataset.id);
  assert.deepEqual(ids(), [a.id, "AS-2"], "?m=0.7 shows that milestone's cards only");
  assert.deepEqual([...doc.querySelectorAll(".colhead .nm")].map((n) => n.textContent), ["Backlog", "Needs Input", "In Progress", "Review", "Merged", "Done"]);
  assert.equal(doc.querySelector('.col[data-col="merged"] .cards').children.length, 2);
  assert.match(doc.querySelector(`.card[data-id="${a.id}"]`).textContent, /0\.7[\s\S]*merged 1111111/);
  assert.equal(doc.getElementById("reviewlink").getAttribute("href"), "/milestones/0.7");
  const sel = doc.getElementById("msel");
  sel.value = "-";
  pg.fire(sel, "change");
  assert.deepEqual(ids(), [loose.id]);

  // The drawer edits the milestone fields like any other: only what changed is sent.
  sel.value = "";
  pg.fire(sel, "change");
  doc.querySelector(`.card[data-id="${loose.id}"]`).click();
  await until("the drawer", () => doc.getElementById("d-ms"));
  assert.equal(doc.getElementById("d-ms").value, "");
  doc.getElementById("d-ms").value = "0.7";
  doc.getElementById("d-area").value = "tooling";
  doc.getElementById("d-ht").value = "- Open the board\n- Tick a box";
  doc.getElementById("d-save").click();
  await until("the save", async () => (await call(client, "get_card", { id: loose.id })).value.area === "tooling");
  const saved = (await call(client, "get_card", { id: loose.id })).value;
  assert.deepEqual([saved.milestone, saved.iteration, saved.area, saved.human_testing, saved.title], ["0.7", 1, "tooling", "- Open the board\n- Tick a box", "no milestone"]);
  assert.match(saved.activity.at(-1).msg, /^Edited milestone, area, human testing$/);

  // New milestone from the header.
  doc.getElementById("newms").click();
  doc.getElementById("nm-name").value = "0.8";
  doc.getElementById("nm-create").click();
  await until("the milestone", async () => (await call(client, "list_milestones", {})).value.some((m) => m.name === "0.8"));
  await until("the filter to follow it", () => doc.getElementById("msel").value === "0.8");
});
