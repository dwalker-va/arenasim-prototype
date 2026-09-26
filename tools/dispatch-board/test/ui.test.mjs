// The web UI's drawer against a real daemon, in jsdom: the page's own script,
// its own fetches, and a foreign session writing through MCP underneath it.
// The rule under test: a drawer edit is based on the version the drawer was
// opened (or rebased) at, and Save sends only the fields the user changed —
// so a concurrent write is either kept or the save is refused, never undone.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { JSDOM } from "jsdom";
import { spawnDaemon, mcpClient, call, PKG } from "./helpers.mjs";

const HTML = readFileSync(join(PKG, "ui", "board.html"), "utf8");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function until(what, pred, ms = 5000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = pred();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(20);
  }
}

/**
 * Open the page over the daemon. The SSE nudge is a stub the test fires by
 * hand (`refresh()`), so WHEN the page learns of a foreign write is decided
 * by the test, not by a race.
 */
async function openPage(t, base) {
  let es;
  const dom = new JSDOM(HTML, {
    url: `${base}/`,
    runScripts: "dangerously",
    beforeParse(window) {
      window.fetch = (path, opts) => fetch(new URL(path, base), opts);
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
  await until("the board to load", () => doc.querySelector(".card"));
  const page = {
    doc,
    /** The daemon's nudge after a write: reload the board (and the open card). */
    async refresh(expectVersion) {
      es.onmessage({});
      await until(`the drawer to show v${expectVersion}`, () => doc.querySelector(".drawer .cid")?.textContent.includes(`v${expectVersion}`));
    },
    async open(id, version) {
      doc.querySelector(`.card[data-id="${id}"]`).click();
      await until("the drawer", () => doc.getElementById("d-save"));
      await until(`the drawer to show v${version}`, () => doc.querySelector(".drawer .cid")?.textContent.includes(`v${version}`));
    },
    set(field, value) {
      const el = doc.getElementById(field);
      el.value = value;
      el.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
      el.dispatchEvent(new dom.window.Event("change", { bubbles: true }));
    },
    value: (field) => doc.getElementById(field).value,
    status: () => doc.querySelector(".hdr .stat").textContent,
    async save() {
      const before = page.status();
      doc.getElementById("d-save").click();
      await until("the save to settle", () => page.status() !== before && page.status() !== "saving…");
    },
  };
  return page;
}

/** A review card with a PR link, as the Tester sees it. */
async function reviewCard(client) {
  const c = await call(client, "create_card", {
    title: "drawer card",
    body: "original spec",
    role: "engineer",
    column: "review",
    links: [{ label: "PR #1", url: "https://github.com/o/r/pull/1" }],
    actor: "pm-test",
  });
  assert.ok(c.ok, JSON.stringify(c.value));
  return c.value;
}

/** The orchestrator's REJECT-shaped write: findings into the body, and a move. */
async function foreignWrite(client, c) {
  const a = await call(client, "append_to_body", { id: c.id, heading: "Tester findings", text: "1. broken", actor: "orchestrator" });
  assert.ok(a.ok, JSON.stringify(a.value));
  const m = await call(client, "move_card", { id: c.id, column: "human_review", expected_version: a.value.version, actor: "orchestrator" });
  assert.ok(m.ok, JSON.stringify(m.value));
  return m.value;
}

const getCard = async (client, id) => (await call(client, "get_card", { id })).value;

test("drawer: a foreign write seen before editing survives a one-field save", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const c = await reviewCard(client);
  const page = await openPage(t, d.base);
  await page.open(c.id, c.version);

  const after = await foreignWrite(client, c);
  await page.refresh(after.version);
  // Untouched fields show what the card is NOW, not what the drawer opened on.
  assert.match(page.value("d-body"), /Tester findings/);
  assert.equal(page.value("d-col"), "human_review");

  page.set("d-pri", "P1");
  await page.save();

  const now = await getCard(client, c.id);
  assert.equal(now.priority, "P1", page.status());
  assert.match(now.body, /Tester findings/, "the save reverted the foreign body append");
  assert.equal(now.column, "human_review", "the save reverted the foreign move");
  assert.equal(now.activity.at(-1).msg, "Edited priority", "the save sent more than the one changed field");
});

test("drawer: an edit that a foreign write overtakes is refused, not written over it", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const c = await reviewCard(client);
  const page = await openPage(t, d.base);
  await page.open(c.id, c.version);

  // The user is mid-edit on the spec when the findings land.
  page.set("d-body", "original spec, edited by the user");
  page.set("d-pri", "P1");
  const after = await foreignWrite(client, c);
  await page.refresh(after.version);

  // The typed edits are kept; the fields the user did not touch are current.
  assert.equal(page.value("d-body"), "original spec, edited by the user");
  assert.equal(page.value("d-pri"), "P1");
  assert.equal(page.value("d-col"), "human_review");
  assert.ok(page.doc.getElementById("d-rebase"), "no changed-elsewhere notice on a dirty drawer");

  await page.save();
  let now = await getCard(client, c.id);
  assert.match(now.body, /Tester findings/, "the stale drawer overwrote the foreign body append");
  assert.equal(now.priority, "P2");
  assert.equal(now.version, after.version, "a refused save must change nothing");
  assert.equal(page.value("d-body"), "original spec, edited by the user", "the refusal threw the user's edit away");

  // Rebasing is the user's explicit act; after it, Save applies the edits to the new version.
  page.doc.getElementById("d-rebase").click();
  await page.save();
  now = await getCard(client, c.id);
  assert.equal(now.body, "original spec, edited by the user");
  assert.equal(now.priority, "P1");
  assert.equal(now.column, "human_review", "the rebased save must still leave the foreign move alone");
});

test("drawer: discarding stale edits shows the card as it is now; a later save sends only new edits", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const c = await reviewCard(client);
  const page = await openPage(t, d.base);
  await page.open(c.id, c.version);
  page.set("d-body", "my draft");
  const after = await foreignWrite(client, c);
  await page.refresh(after.version);

  page.doc.getElementById("d-discard").click();
  assert.match(page.value("d-body"), /Tester findings/);
  assert.equal(page.doc.getElementById("d-rebase"), null, "the notice outlived the discard");
  page.set("d-pri", "P3");
  await page.save();
  const now = await getCard(client, c.id);
  assert.deepEqual([now.priority, now.column], ["P3", "human_review"]);
  assert.match(now.body, /Tester findings/);
});

test("drawer: a foreign write the page has not seen yet refuses the save", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const c = await reviewCard(client);
  const page = await openPage(t, d.base);
  await page.open(c.id, c.version);

  await foreignWrite(client, c); // no refresh: the page still shows the opened version
  page.set("d-pri", "P1");
  await page.save();

  const now = await getCard(client, c.id);
  assert.equal(now.priority, "P2");
  assert.match(now.body, /Tester findings/);
  assert.equal(now.column, "human_review");
});

test("drawer: a working claim is released only by the explicit, logged gesture", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  let c = (await call(client, "create_card", { title: "claimed", role: "engineer", actor: "o" })).value;
  c = (await call(client, "move_card", { id: c.id, column: "in_progress", expected_version: c.version, actor: "o" })).value;
  c = (await call(client, "claim_card", { id: c.id, name: "Engineer-AS-1", actor: "orchestrator" })).value;
  const page = await openPage(t, d.base);
  await page.open(c.id, c.version);

  page.set("d-title", "renamed");
  await page.save();
  assert.equal((await getCard(client, c.id)).agent.status, "working", "an ordinary save touched the claim");

  c = await getCard(client, c.id);
  await page.open(c.id, c.version);
  page.doc.getElementById("d-release").click(); // first click asks
  assert.equal((await getCard(client, c.id)).agent.status, "working");
  page.doc.getElementById("d-release").click();
  await until("the release", () => /released/.test(page.status()));
  const now = await getCard(client, c.id);
  assert.equal(now.agent, null);
  assert.match(now.activity.at(-1).msg, /released from the board by the user \(was Engineer-AS-1\)/);
});
