// The daemon end to end: a real spawned process, real MCP clients over
// Streamable HTTP, the UI's JSON API, and the `wait` CLI a Monitor runs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { request } from "node:http";
import { existsSync } from "node:fs";
import { lockPath } from "../dist/paths.js";
import { spawnDaemon, mcpClient, call, post, tempDir, CLI } from "./helpers.mjs";

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

test("MCP: the PR gate refuses an engineer card without its own pr and admits a pm card", async (t) => {
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
    patch: { pr: { url: "https://github.com/o/r/pull/5" } },
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

test("web UI API: a delete without expected_version is refused and deletes nothing", async (t) => {
  const d = await spawnDaemon(t);
  const c = (await post(d.base, "/api/cards", { title: "keep me", role: "engineer" })).body;
  for (const body of [{}, { expected_version: null }, { expected_version: String(c.version) }]) {
    const r = await post(d.base, `/api/cards/${c.id}/delete`, body);
    assert.equal(r.status, 422, JSON.stringify(r.body));
  }
  assert.equal((await fetch(`${d.base}/api/cards/${c.id}`)).status, 200);
  assert.equal((await post(d.base, `/api/cards/${c.id}/delete`, { expected_version: c.version })).status, 200);
});

test("claims: update_card / move_card / the UI cannot take or silently drop a claim", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  let c = (await call(client, "create_card", { title: "c", role: "engineer", actor: "o" })).value;
  c = (await call(client, "move_card", { id: c.id, column: "in_progress", expected_version: c.version, actor: "o" })).value;
  c = (await call(client, "claim_card", { id: c.id, name: "Engineer-A", actor: "orchestrator" })).value;

  // MCP: a patch cannot set working (the schema says so before the board does).
  const steal = await client.callTool({
    name: "update_card",
    arguments: { id: c.id, patch: { agent: { status: "working", name: "Engineer-B" } }, expected_version: c.version, actor: "o" },
  });
  assert.equal(steal.isError, true);
  // The UI API: no agent in a patch at all — not working, not null.
  for (const agent of [{ status: "working", name: "Engineer-B" }, null]) {
    const u = await post(d.base, `/api/cards/${c.id}/update`, { patch: { agent }, expected_version: c.version });
    assert.equal(u.status, 422, JSON.stringify(u.body));
    const m = await post(d.base, `/api/cards/${c.id}/move`, { column: "backlog", patch: { agent }, expected_version: c.version });
    assert.equal(m.status, 422, JSON.stringify(m.body));
  }
  const now = (await call(client, "get_card", { id: c.id })).value;
  assert.deepEqual([now.version, now.agent.name, now.agent.status], [c.version, "Engineer-A", "working"]);

  // The explicit gesture: versioned, logged, and it wakes the orchestrator.
  assert.equal((await post(d.base, `/api/cards/${c.id}/release`, {})).status, 422, "a versionless release");
  const rel = await post(d.base, `/api/cards/${c.id}/release`, { expected_version: c.version });
  assert.equal(rel.status, 200, JSON.stringify(rel.body));
  assert.equal(rel.body.agent, null);
  assert.deepEqual(rel.body.activity.slice(-2).map((a) => a.msg), ["Released from the board by the user", "Claim released (was Engineer-A)"]);
  const ev = await call(client, "events_since", { cursor: 0, ignore_actors: ["orchestrator", "o"] });
  assert.deepEqual(ev.value.events.map((e) => [e.actor, e.kind]), [["board", "claim_released"]]);
});

test("MCP: move_card with append is the one-write REJECT", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  let c = (await call(client, "create_card", { title: "c", body: "spec", role: "engineer", column: "review", pr: { url: "https://github.com/o/r/pull/1" }, actor: "o" })).value;
  c = (await call(client, "claim_card", { id: c.id, name: "AS-1-test", actor: "orchestrator" })).value;
  const r = await call(client, "move_card", {
    id: c.id,
    column: "in_progress",
    expected_version: c.version,
    append: { heading: "Tester findings — 2026-09-25", text: "1. broken" },
    actor: "orchestrator",
    by: "tester",
  });
  assert.ok(r.ok, JSON.stringify(r.value));
  assert.deepEqual([r.value.version, r.value.column, r.value.agent], [c.version + 1, "in_progress", null]);
  assert.match(r.value.body, /## Tester findings — 2026-09-25\n\n1\. broken$/);
});

test("wake-up: a cursor past head is printed and resumed from head, never waited on silently", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const c = (await call(client, "create_card", { title: "c", role: "engineer", actor: "pm" })).value;

  const once = await runWait(["--since", "999999", "--port", String(d.port)]).done;
  assert.equal(once.code, 3, once.stderr);
  assert.equal(once.lines[0].error, "cursor_ahead");
  assert.equal(once.lines[0].head, 1);

  const w = runWait(["--follow", "--since", "999999", "--port", String(d.port)]);
  t.after(() => w.child.kill());
  for (let i = 0; i < 50 && !w.stdoutSoFar(); i++) await sleep(100);
  await call(client, "append_activity", { id: c.id, msg: "after the reset", actor: "pm" });
  for (let i = 0; i < 50 && w.stdoutSoFar().split("\n").filter(Boolean).length < 2; i++) await sleep(100);
  const lines = w.stdoutSoFar().split("\n").filter(Boolean).map((l) => JSON.parse(l));
  assert.equal(lines[0].error, "cursor_ahead");
  assert.deepEqual([lines[1].kind, lines[1].cursor], ["activity", 2], "follow mode resumes from head and delivers the next event");

  const mcp = await client.callTool({ name: "events_since", arguments: { cursor: 999999 } });
  assert.equal(mcp.isError, true);
  assert.equal(JSON.parse(mcp.content[0].text).error, "cursor_ahead");
});

test("wake-up: a re-created board behind the same port is printed, not delivered as news", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  for (let i = 0; i < 3; i++) await call(client, "create_card", { title: `old ${i}`, role: "engineer", actor: "pm" });
  const w = runWait(["--follow", "--since", "3", "--port", String(d.port)]);
  t.after(() => w.child.kill());
  await sleep(300);
  await d.stop();

  // A different database on the same port, which already has MORE events than
  // the waiter's cursor: without the board check, events 4.. would be read as new.
  const tmp = tempDir();
  t.after(() => tmp.cleanup());
  const second = spawn(process.execPath, [CLI, "serve", "--db", tmp.db, "--port", String(d.port)], { stdio: ["ignore", "ignore", "pipe"] });
  t.after(() => second.kill());
  await new Promise((res) => second.stderr.on("data", (x) => /serving/.test(String(x)) && res()));
  const c2 = await mcpClient(t, d.base);
  for (let i = 0; i < 5; i++) await call(c2, "create_card", { title: `new ${i}`, role: "engineer", actor: "pm" });

  for (let i = 0; i < 80 && !/board_replaced/.test(w.stdoutSoFar()); i++) await sleep(100);
  const lines = w.stdoutSoFar().split("\n").filter(Boolean).map((l) => JSON.parse(l));
  const kinds = lines.map((l) => l.error ?? (l.reconnected ? "reconnected" : l.kind));
  assert.ok(kinds.includes("board_replaced"), JSON.stringify(lines));
  assert.ok(!lines.some((l) => l.kind === "created"), `a replaced board's events were delivered: ${JSON.stringify(lines)}`);
  assert.equal(lines.find((l) => l.error === "board_replaced").head, 5);
});

test("wake-up: --follow across a restart of the SAME board resumes exactly once, with no resync", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  const c = (await call(client, "create_card", { title: "c", role: "engineer", actor: "pm" })).value;
  const w = runWait(["--follow", "--since", "1", "--port", String(d.port)]);
  t.after(() => w.child.kill());
  await sleep(300);
  await d.stop();
  const again = spawn(process.execPath, [CLI, "serve", "--db", d.db, "--port", String(d.port)], { stdio: ["ignore", "ignore", "pipe"] });
  t.after(() => new Promise((res) => (again.exitCode !== null ? res() : (again.once("exit", res), again.kill()))));
  await new Promise((res) => again.stderr.on("data", (x) => /serving/.test(String(x)) && res()));
  const c2 = await mcpClient(t, d.base);
  await call(c2, "append_activity", { id: c.id, msg: "after restart", actor: "pm" });
  for (let i = 0; i < 80 && !/"activity"/.test(w.stdoutSoFar()); i++) await sleep(100);
  const lines = w.stdoutSoFar().split("\n").filter(Boolean).map((l) => JSON.parse(l));
  assert.deepEqual(
    lines.map((l) => l.error ?? (l.reconnected ? "reconnected" : `${l.kind}@${l.cursor}`)),
    ["daemon_unreachable", "reconnected", "activity@2"],
  );
});

/** Spawn a daemon and SIGTERM it synchronously, in the same tick it announces itself. */
function termOnServing() {
  const tmp = tempDir();
  const child = spawn(process.execPath, [CLI, "serve", "--db", tmp.db, "--port", "0"], { stdio: ["ignore", "ignore", "pipe"] });
  let err = "";
  child.stderr.on("data", (x) => {
    err += x;
    if (/serving/.test(err) && child.signalCode === null && !child.killed) child.kill("SIGTERM");
  });
  return new Promise((res) =>
    child.once("exit", (code, signal) => {
      const lock = existsSync(lockPath(tmp.db));
      tmp.cleanup();
      res({ code, signal, lock });
    }),
  );
}

test("shutdown: a SIGTERM the instant the daemon says it is serving is handled gracefully", async () => {
  // The daemon installs its handlers before it announces itself, so no
  // SIGTERM after "serving" can take the default action (dying without
  // removing its lock). Many trials: the window this closes is a race.
  const trials = [];
  for (let batch = 0; batch < 4; batch++) trials.push(...(await Promise.all(Array.from({ length: 10 }, termOnServing))));
  const bad = trials.filter((r) => r.code !== 0 || r.signal !== null || r.lock);
  assert.deepEqual(bad, [], `${bad.length}/${trials.length} daemons died by signal or kept their lock`);
});

test("shutdown: the test helper's stop() settles on a daemon that died by signal, and twice", async (t) => {
  const d = await spawnDaemon(t);
  await new Promise((res) => {
    d.child.once("exit", res);
    d.child.kill("SIGKILL");
  });
  await d.stop();
  await d.stop();
});

test("wake-up: a FRESH waiter armed with --board catches a board replaced while it was not running", async (t) => {
  const old = await spawnDaemon(t);
  const client = await mcpClient(t, old.base);
  for (let i = 0; i < 3; i++) await call(client, "create_card", { title: `old ${i}`, role: "engineer", actor: "pm" });
  const headRun = spawn(process.execPath, [CLI, "head", "--port", String(old.port)], { stdio: ["ignore", "pipe", "ignore"] });
  let headOut = "";
  headRun.stdout.on("data", (x) => (headOut += x));
  await new Promise((r) => headRun.on("exit", r));
  const armed = JSON.parse(headOut);
  assert.equal(armed.cursor, 3);
  assert.match(armed.board, /^[0-9a-f-]{36}$/);

  // The re-created board already has MORE events than the saved cursor.
  const fresh = await spawnDaemon(t);
  const c2 = await mcpClient(t, fresh.base);
  for (let i = 0; i < 5; i++) await call(c2, "create_card", { title: `new ${i}`, role: "engineer", actor: "pm" });

  const r = await runWait(["--since", String(armed.cursor), "--board", armed.board, "--port", String(fresh.port)]).done;
  assert.equal(r.code, 3, r.stderr);
  assert.deepEqual(r.lines.map((l) => l.error ?? l.kind), ["board_replaced"], "a replaced board's events were delivered as news");
  assert.equal(r.lines[0].head, 5);

  // Armed with the board it came from, the same cursor delivers normally.
  await call(client, "create_card", { title: "old 3", role: "engineer", actor: "pm" });
  const ok = await runWait(["--since", String(armed.cursor), "--board", armed.board, "--port", String(old.port)]).done;
  assert.deepEqual(ok.lines.map((l) => [l.kind, l.cursor]), [["created", 4]]);
});
