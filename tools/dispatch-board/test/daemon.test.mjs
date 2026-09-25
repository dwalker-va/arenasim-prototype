// The daemon end to end: a real spawned process, real MCP clients over
// Streamable HTTP, the UI's JSON API, and the `wait` CLI a Monitor runs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { request } from "node:http";
import { spawnDaemon, mcpClient, call, post, CLI } from "./helpers.mjs";

/** Run `cli.js wait ...` and resolve with its stdout lines and exit code. */
function runWait(args) {
  const child = spawn(process.execPath, [CLI, "wait", ...args], { stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (d) => (stdout += d));
  child.stderr.on("data", (d) => (stderr += d));
  const done = new Promise((resolve) =>
    child.on("exit", (code) => resolve({ code, lines: stdout.split("\n").filter(Boolean).map((l) => JSON.parse(l)), stderr })),
  );
  return { child, done, stdoutSoFar: () => stdout };
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test("MCP: tools are served and list_cards returns summaries only", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const names = (await client.listTools()).tools.map((x) => x.name).sort();
  assert.deepEqual(names, [
    "answer_question", "append_activity", "append_to_body", "claim_card", "create_card", "events_since",
    "finish_claim", "get_card", "list_cards", "move_card", "release_claim", "update_card",
  ]);
  const created = await call(client, "create_card", { title: "via mcp", body: "long spec", role: "engineer", actor: "pm-test" });
  assert.ok(created.ok);
  assert.equal(created.value.id, "AS-1");
  const list = await call(client, "list_cards", {});
  assert.equal(list.value.length, 1);
  assert.equal(list.value[0].body, undefined);
  assert.equal(list.value[0].activity, undefined);
});

test("MCP: a stale version is refused as a tool error carrying the current card", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const c = (await call(client, "create_card", { title: "v", role: "engineer", actor: "o" })).value;
  assert.ok((await call(client, "update_card", { id: c.id, patch: { priority: "P1" }, expected_version: c.version, actor: "o" })).ok);
  const stale = await call(client, "update_card", { id: c.id, patch: { priority: "P3" }, expected_version: c.version, actor: "o" });
  assert.equal(stale.ok, false);
  assert.equal(stale.value.error, "stale_version");
  assert.equal(stale.value.card.priority, "P1");
});

test("MCP: concurrent claims from separate clients resolve to exactly one winner", async (t) => {
  const d = await spawnDaemon(t);
  const setup = await mcpClient(t, d.base);
  let c = (await call(setup, "create_card", { title: "race", role: "engineer", actor: "o" })).value;
  c = (await call(setup, "move_card", { id: c.id, column: "in_progress", expected_version: c.version, actor: "board" })).value;
  const clients = await Promise.all(Array.from({ length: 10 }, () => mcpClient(t, d.base)));
  const results = await Promise.all(clients.map((cl, i) => call(cl, "claim_card", { id: c.id, name: `Engineer-${i}`, actor: "o" })));
  const winners = results.filter((r) => r.ok);
  assert.equal(winners.length, 1, JSON.stringify(results.map((r) => r.value.error ?? "won")));
  assert.ok(results.filter((r) => !r.ok).every((r) => r.value.error === "claim_refused"));
});

test("MCP: the PR-link gate refuses a linkless engineer card and admits a pm card", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const eng = (await call(client, "create_card", { title: "e", role: "engineer", actor: "o" })).value;
  const refused = await call(client, "move_card", { id: eng.id, column: "human_review", expected_version: eng.version, actor: "o" });
  assert.equal(refused.value.error, "gate_refused");
  const pm = (await call(client, "create_card", { title: "p", role: "pm", actor: "o" })).value;
  assert.ok((await call(client, "move_card", { id: pm.id, column: "review", expected_version: pm.version, actor: "o" })).ok);
});

test("wake-up: a UI drag wakes a waiter; the orchestrator's own writes do not", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const c = (await call(client, "create_card", { title: "wake me", role: "engineer", actor: "orchestrator" })).value;
  const head = (await (await fetch(`${d.base}/api/health`)).json()).head;

  const w = runWait(["--since", String(head), "--ignore-actor", "orchestrator", "--port", String(d.port)]);
  // The orchestrator's own write must NOT wake it...
  await call(client, "append_activity", { id: c.id, msg: "orchestrator's own write", actor: "orchestrator" });
  await sleep(400);
  assert.equal(w.stdoutSoFar(), "", "the waiter woke on its own session's write");
  assert.equal(w.child.exitCode, null, "the waiter is still blocked");
  // ...a user gesture in the web UI must.
  const current = (await (await fetch(`${d.base}/api/cards/${c.id}`)).json()).version;
  const drag = await post(d.base, `/api/cards/${c.id}/move`, { column: "in_progress", expected_version: current, activity: "Dragged: backlog → in_progress" });
  assert.equal(drag.status, 200, JSON.stringify(drag.body));

  const r = await w.done;
  assert.equal(r.code, 0, r.stderr);
  assert.equal(r.lines.length, 1);
  assert.equal(r.lines[0].actor, "board");
  assert.equal(r.lines[0].kind, "moved");
  assert.equal(r.lines[0].card, c.id);
  assert.deepEqual([r.lines[0].data.from, r.lines[0].data.to], ["backlog", "in_progress"]);
  assert.ok(r.lines[0].cursor > head);
});

test("wake-up: --follow streams every event as its own line, and events_since skips own writes", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const w = runWait(["--follow", "--since", "0", "--ignore-actor", "orchestrator", "--port", String(d.port)]);
  t.after(() => w.child.kill());
  const a = (await call(client, "create_card", { title: "from pm", role: "engineer", actor: "pm-session" })).value;
  await call(client, "append_activity", { id: a.id, msg: "mine", actor: "orchestrator" });
  await post(d.base, `/api/cards/${a.id}/update`, { patch: { priority: "P1" }, expected_version: a.version + 1 });
  for (let i = 0; i < 50 && w.stdoutSoFar().split("\n").filter(Boolean).length < 2; i++) await sleep(100);
  const lines = w.stdoutSoFar().split("\n").filter(Boolean).map((l) => JSON.parse(l));
  assert.deepEqual(lines.map((l) => [l.actor, l.kind]), [["pm-session", "created"], ["board", "edited"]]);

  const ev = await call(client, "events_since", { cursor: 0, ignore_actors: ["orchestrator"] });
  assert.deepEqual(ev.value.events.map((e) => e.actor), ["pm-session", "board"]);
  assert.equal(ev.value.cursor, ev.value.head);
});

test("wake-up: an unreachable daemon is printed, never silent", async (t) => {
  const d = await spawnDaemon(t);
  await d.stop();
  const r = await runWait(["--since", "0", "--port", String(d.port)]).done;
  assert.equal(r.code, 2);
  assert.equal(r.lines[0].error, "daemon_unreachable");
});

test("web UI: served at /, and the UI API enforces the same gate and versions", async (t) => {
  const d = await spawnDaemon(t);
  const page = await fetch(`${d.base}/`);
  assert.equal(page.status, 200);
  assert.match(await page.text(), /ArenaSim/);

  const created = await post(d.base, "/api/cards", { title: "ui card", role: "engineer", priority: "P2", body: "" });
  assert.equal(created.status, 200);
  const gate = await post(d.base, `/api/cards/${created.body.id}/move`, { column: "review", expected_version: created.body.version });
  assert.equal(gate.status, 422);
  assert.equal(gate.body.error, "gate_refused");
  const withLink = await post(d.base, `/api/cards/${created.body.id}/move`, {
    column: "review",
    expected_version: created.body.version,
    patch: { links: [{ label: "PR #5", url: "https://github.com/o/r/pull/5" }] },
  });
  assert.equal(withLink.status, 200);
  const stale = await post(d.base, `/api/cards/${created.body.id}/update`, { patch: { title: "x" }, expected_version: created.body.version });
  assert.equal(stale.status, 409);
  assert.equal(stale.body.card.column, "review");

  const board = await (await fetch(`${d.base}/api/board`)).json();
  assert.equal(board.cards[0].body, undefined, "the board view carries no bodies");
});

test("security: foreign Host headers and cross-origin requests are refused", async (t) => {
  const d = await spawnDaemon(t);
  const status = (headers) =>
    new Promise((resolve, reject) => {
      const req = request({ host: "127.0.0.1", port: d.port, path: "/api/board", headers }, (res) => {
        res.resume();
        resolve(res.statusCode);
      });
      req.on("error", reject);
      req.end();
    });
  assert.equal(await status({ host: "evil.example:80" }), 403);
  assert.equal(await status({ host: `127.0.0.1:${d.port}`, origin: "https://evil.example" }), 403);
  assert.equal(await status({ host: `localhost:${d.port}` }), 200);
});

test("one daemon per db: a second daemon on the same db refuses to start", async (t) => {
  const d = await spawnDaemon(t);
  const second = spawn(process.execPath, [CLI, "serve", "--db", d.db, "--port", "0"], { stdio: ["ignore", "ignore", "pipe"] });
  let err = "";
  second.stderr.on("data", (x) => (err += x));
  const code = await new Promise((r) => second.on("exit", r));
  assert.notEqual(code, 0);
  assert.match(err, /already owns/);
});
