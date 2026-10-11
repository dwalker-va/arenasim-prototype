// The review page's ▶ replay button (AS-237): the daemon launches a guide
// check's STORED replay config in the game client, looked up by milestone and
// check key, and nothing a request carries. Every launch here goes through an
// injected spawn or a stub shell script — never the game.
import { test } from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { chmodSync, mkdirSync, readFileSync, utimesSync, writeFileSync, appendFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { JSDOM } from "jsdom";
import { BoardError } from "../dist/board.js";
import { ReplayLauncher, resolveGameBinary, GAME_BIN_ENV } from "../dist/replay.js";
import { tempBoard, tempDir, spawnDaemon, mcpClient, call, post, sleep } from "./helpers.mjs";

const { startDaemon, reviewPage } = await import("../dist/server.js");
const o = { actor: "orchestrator" };
const REPLAY = { team1: ["Druid", "Priest"], team2: ["Warrior", "Rogue"], map: "PillaredArena", random_seed: 42 };
const KEY = "guide:druid-root:shift";

function guideFor(ids) {
  return {
    units: [
      {
        id: "druid-root",
        title: "Druid in a root",
        summary: "A rooted Druid shifts out.",
        checks: [
          { id: "shift", text: "Watch the Druid shift out of Frost Nova", replay: REPLAY },
          { id: "no-loop", text: "It never loops" },
        ],
        cards: ids,
      },
      {
        id: "flare",
        title: "Flare",
        summary: "One flash.",
        checks: [{ id: "flash", text: "One flash", replay: { team1: ["Hunter"], team2: ["Rogue"] } }],
        cards: ids,
      },
    ],
  };
}

function seedGuide(board) {
  board.createMilestone("0.7", o);
  const id = board.createCard({ title: "c", role: "engineer", milestone: "0.7" }, o).id;
  board.setReviewGuide("0.7", guideFor([id]), board.getMilestone("0.7").milestone.version, o);
}

/** An executable stub file standing in for the game binary (never run by the fake spawn). */
function stubBinary(dir, script = "#!/bin/sh\nexit 0\n") {
  const p = join(dir, "bin", "arenasim");
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(p, script);
  chmodSync(p, 0o755);
  return p;
}

/**
 * A fake spawn: records every call and returns a process the test drives.
 * `behave(proc, call)` runs right after the spawn (e.g. to exit early).
 */
function fakeSpawn(behave = () => {}) {
  const calls = [];
  const fn = (cmd, args, opts) => {
    const proc = new EventEmitter();
    proc.pid = 4242;
    proc.unref = () => (proc.unrefed = true);
    const c = { argv: [cmd, ...args], opts, proc, config: JSON.parse(readFileSync(args[1], "utf8")) };
    calls.push(c);
    queueMicrotask(() => behave(proc, c));
    return proc;
  };
  return { fn, calls };
}

/** An in-process daemon over a temp DB with the guide seeded and the spawn injected. */
async function daemon(t, replay = {}) {
  const tmp = tempDir();
  const { Board } = await import("../dist/board.js");
  const b = new Board(tmp.db);
  seedGuide(b);
  b.close();
  const binary = replay.binary === undefined ? stubBinary(tmp.dir) : replay.binary;
  const d = await startDaemon({ dbPath: tmp.db, port: 0, log: () => {}, replay: { earlyExitMs: 60, ...replay, binary } });
  t.after(async () => {
    await d.close();
    tmp.cleanup();
  });
  return { base: `http://127.0.0.1:${d.port}`, dir: tmp.dir, binary };
}

const launch = (base, body, name = "0.7") => post(base, `/api/milestones/${encodeURIComponent(name)}/replay`, body);

// ---- the lookup

test("replay lookup: the stored config, by milestone and guide check key; unknown or replay-less checks are refused", (t) => {
  const { board } = tempBoard(t);
  seedGuide(board);
  assert.deepEqual(board.guideReplay("0.7", KEY), REPLAY);
  const refusedAs = (fn, code) => assert.throws(fn, (e) => e instanceof BoardError && e.code === code);
  refusedAs(() => board.guideReplay("0.8", KEY), "not_found");
  refusedAs(() => board.guideReplay("0.7", "guide:druid-root:nope"), "not_found");
  refusedAs(() => board.guideReplay("0.7", "check:AS-1:0"), "not_found");
  refusedAs(() => board.guideReplay("0.7", "guide:druid-root:no-loop"), "invalid");
  refusedAs(() => board.guideReplay("0.7", undefined), "invalid");
});

// ---- the endpoint

test("replay endpoint: an unknown milestone or check key is refused, and nothing is launched", async (t) => {
  const spawn = fakeSpawn();
  const { base } = await daemon(t, { spawn: spawn.fn });
  assert.equal((await launch(base, { key: KEY }, "0.8")).status, 404);
  assert.equal((await launch(base, { key: "guide:druid-root:nope" })).status, 404);
  assert.equal((await launch(base, { key: "guide:druid-root:no-loop" })).status, 422, "a check with no replay config");
  assert.equal((await launch(base, {})).status, 422, "no key");
  assert.equal(spawn.calls.length, 0);
});

test("replay endpoint: a body carrying its own config or command is refused; nothing is launched", async (t) => {
  const spawn = fakeSpawn();
  const { base } = await daemon(t, { spawn: spawn.fn });
  for (const extra of [{ config: { team1: ["Mage"], team2: ["Mage"] } }, { command: "rm -rf /" }, { argv: ["--headless"] }, { binary: "/bin/sh" }]) {
    const r = await launch(base, { key: KEY, ...extra });
    assert.equal(r.status, 400, JSON.stringify(extra));
    assert.match(r.body.message, /config comes from the stored guide/);
  }
  assert.equal(spawn.calls.length, 0);
});

test("replay endpoint: argv is exactly [binary, --replay, <file>], and the file holds the stored config", async (t) => {
  const spawn = fakeSpawn();
  const { base, dir, binary } = await daemon(t, { spawn: spawn.fn });
  const r = await launch(base, { key: KEY });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.status, "launched");
  assert.equal(r.body.message, "launched");
  assert.equal(r.body.pid, 4242);
  assert.equal(spawn.calls.length, 1);
  const [c] = spawn.calls;
  assert.deepEqual(c.argv, [binary, "--replay", r.body.file]);
  assert.equal(dirname(r.body.file), join(dir, "replays"), "the file is under the board's data directory");
  assert.deepEqual(JSON.parse(readFileSync(r.body.file, "utf8")), REPLAY);
  assert.deepEqual(c.config, REPLAY, "the file held the stored config when the game started");
  assert.equal(c.proc.unrefed, true, "the daemon lets the game go");
  // The other check launches its own config.
  const r2 = await launch(base, { key: "guide:flare:flash" });
  assert.equal(r2.body.status, "launched");
  assert.deepEqual(spawn.calls[1].config, { team1: ["Hunter"], team2: ["Rogue"] });
  assert.notEqual(r2.body.file, r.body.file);
});

test("replay endpoint: a second launch of the same check within 2s is ignored; another check, or later, is not", async (t) => {
  let now = 1_000_000;
  const spawn = fakeSpawn();
  const { base } = await daemon(t, { spawn: spawn.fn, now: () => now });
  // A double click: the second arrives while the first is still being watched.
  const both = await Promise.all([launch(base, { key: KEY }), launch(base, { key: KEY })]);
  assert.deepEqual(both.map((r) => r.status).sort(), [200, 429]);
  assert.match(both.find((r) => r.status === 429).body.message, /already starting/);
  assert.equal(spawn.calls.length, 1);
  // The window runs from the "launched" answer, so a click right after it is ignored too.
  now += 1500;
  const again = await launch(base, { key: KEY });
  assert.equal(again.status, 429);
  assert.equal(again.body.error, "rate_limited");
  assert.equal(spawn.calls.length, 1, "the second click launched nothing");
  assert.equal((await launch(base, { key: "guide:flare:flash" })).body.status, "launched", "a different check is its own limit");
  now += 600;
  assert.equal((await launch(base, { key: KEY })).body.status, "launched", "2s after the first, the check launches again");
  assert.equal(spawn.calls.length, 3);
});

test("replay endpoint: a launch that died early frees its check for an immediate retry", async (t) => {
  const spawn = fakeSpawn((proc, c) => {
    if (c === spawn.calls[0]) proc.emit("exit", 1, null);
  });
  const { base } = await daemon(t, { spawn: spawn.fn, now: () => 5_000_000 });
  assert.equal((await launch(base, { key: KEY })).body.status, "exited");
  assert.equal((await launch(base, { key: KEY })).body.status, "launched");
  assert.equal(spawn.calls.length, 2);
});

test("replay endpoint: a missing binary gives the build-first error, launches nothing, and is not rate-limited", async (t) => {
  const spawn = fakeSpawn();
  const { base, dir } = await daemon(t, { spawn: spawn.fn, binary: "/no/such/dir/arenasim" });
  for (let i = 0; i < 2; i++) {
    const r = await launch(base, { key: KEY });
    assert.equal(r.status, 503);
    assert.equal(r.body.error, "binary_not_found");
    assert.match(r.body.message, /game binary not found at \/no\/such\/dir\/arenasim — build first: cargo build --release/);
  }
  assert.equal(spawn.calls.length, 0);
  assert.equal(existsSync(join(dir, "replays")), false, "no replay file is written for a launch that cannot happen");
});

test("replay endpoint: a game that exits inside the window reports its code and stderr's tail", async (t) => {
  const spawn = fakeSpawn((proc, c) => {
    appendFileSync(c.opts.stderrPath, "loading...\nError loading replay config: unknown class Druidd\n");
    proc.emit("exit", 1, null);
  });
  const { base } = await daemon(t, { spawn: spawn.fn, earlyExitMs: 2000 });
  const r = await launch(base, { key: KEY });
  assert.equal(r.status, 200);
  assert.equal(r.body.status, "exited");
  assert.equal(r.body.message, "the game exited with code 1");
  assert.equal(r.body.code, 1);
  assert.match(r.body.stderr_tail, /Error loading replay config: unknown class Druidd$/);
});

// ---- the real spawn, against a stub script (never the game)

test("replay launcher: the real detached spawn runs argv exactly, and an early exit's stderr comes back", async (t) => {
  const tmp = tempDir();
  t.after(tmp.cleanup);
  const out = join(tmp.dir, "argv.txt");
  const bin = stubBinary(tmp.dir, `#!/bin/sh\nprintf '%s\\n' "$0" "$@" > '${out}'\necho "Error loading replay config: boom" >&2\nexit 3\n`);
  const l = new ReplayLauncher({ binary: bin, dataDir: tmp.dir, cwd: tmp.dir, earlyExitMs: 5000 });
  const r = await l.launch("0.7", KEY, REPLAY);
  assert.equal(r.status, "exited");
  assert.equal(r.message, "the game exited with code 3");
  assert.equal(r.stderr_tail, "Error loading replay config: boom");
  assert.deepEqual(readFileSync(out, "utf8").trimEnd().split("\n"), [bin, "--replay", r.file]);
});

test("replay launcher: the real spawn of a process that outlives the window is launched", async (t) => {
  const tmp = tempDir();
  t.after(tmp.cleanup);
  const bin = stubBinary(tmp.dir, "#!/bin/sh\nexec sleep 5\n");
  const l = new ReplayLauncher({ binary: bin, dataDir: tmp.dir, cwd: tmp.dir, earlyExitMs: 150 });
  const r = await l.launch("0.7", KEY, REPLAY);
  assert.equal(r.status, "launched");
  assert.ok(Number.isInteger(r.pid) && r.pid > 0);
  process.kill(r.pid, "SIGTERM");
});

test("replay launcher: a binary older than the checkout's HEAD launches with a rebuild warning", async (t) => {
  const tmp = tempDir();
  t.after(tmp.cleanup);
  const bin = stubBinary(tmp.dir);
  const checkout = join(tmp.dir, "checkout");
  mkdirSync(join(checkout, ".git", "refs", "heads"), { recursive: true });
  writeFileSync(join(checkout, ".git", "HEAD"), "ref: refs/heads/main\n");
  const ref = join(checkout, ".git", "refs", "heads", "main");
  writeFileSync(ref, "0".repeat(40) + "\n");
  const at = (p, s) => utimesSync(p, s, s);
  at(join(checkout, ".git", "HEAD"), 1000);
  at(bin, 2000);
  const { fn } = fakeSpawn();
  const l = () => new ReplayLauncher({ binary: bin, checkout, dataDir: tmp.dir, spawn: fn, earlyExitMs: 20, rateLimitMs: 0 });
  at(ref, 1500);
  let r = await l().launch("0.7", KEY, REPLAY);
  assert.equal(r.stale, false);
  assert.equal(r.message, "launched");
  at(ref, 3000);
  r = await l().launch("0.7", KEY, REPLAY);
  assert.equal(r.status, "launched");
  assert.equal(r.stale, true);
  assert.match(r.message, /older than the checkout's HEAD; rebuild \(cargo build --release\)/);
});

test("replay binary: --game-bin wins over $DISPATCH_BOARD_GAME_BIN, which wins over the main checkout's release build", (t) => {
  const saved = process.env[GAME_BIN_ENV];
  t.after(() => (saved === undefined ? delete process.env[GAME_BIN_ENV] : (process.env[GAME_BIN_ENV] = saved)));
  delete process.env[GAME_BIN_ENV];
  const def = resolveGameBinary();
  assert.match(def.binary, /\/target\/release\/arenasim$/);
  assert.equal(join(def.checkout, "target", "release", "arenasim"), def.binary);
  process.env[GAME_BIN_ENV] = "/env/arenasim";
  assert.deepEqual(resolveGameBinary(), { binary: "/env/arenasim", checkout: null });
  assert.deepEqual(resolveGameBinary("/flag/arenasim"), { binary: "/flag/arenasim", checkout: null });
});

test("replay daemon: serve takes the binary from --game-bin over $DISPATCH_BOARD_GAME_BIN", async (t) => {
  for (const [env, args, want] of [
    [{ [GAME_BIN_ENV]: "/env-only/arenasim" }, [], "/env-only/arenasim"],
    [{ [GAME_BIN_ENV]: "/env-only/arenasim" }, ["--game-bin", "/from-flag/arenasim"], "/from-flag/arenasim"],
  ]) {
    const d = await spawnDaemon(t, env, args);
    const client = await mcpClient(t, d.base);
    assert.ok((await call(client, "create_milestone", { name: "0.7", ...o })).ok);
    const id = (await call(client, "create_card", { title: "c", role: "engineer", milestone: "0.7", ...o })).value.id;
    const v = (await call(client, "get_milestone", { name: "0.7" })).value.milestone.version;
    assert.ok((await call(client, "set_review_guide", { name: "0.7", guide: guideFor([id]), expected_version: v, ...o })).ok);
    const r = await launch(d.base, { key: KEY });
    assert.equal(r.status, 503);
    assert.equal(r.body.message, `game binary not found at ${want} — build first: cargo build --release`);
  }
});

// ---- the page

async function until(what, pred, ms = 5000) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await pred();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(20);
  }
}

async function openReview(t, base) {
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
  es.onopen();
  const doc = dom.window.document;
  await until("the guide", () => doc.querySelector("#guide button[data-replay]"));
  return doc;
}

test("review page: ▶ launches the check's replay and shows launched, or the error with stderr's tail", async (t) => {
  let fail = false;
  const spawn = fakeSpawn((proc, c) => {
    if (!fail) return;
    appendFileSync(c.opts.stderrPath, "Invalid replay config: map Nagrand\n");
    proc.emit("exit", 2, null);
  });
  const { base } = await daemon(t, { spawn: spawn.fn, rateLimitMs: 0 });
  const doc = await openReview(t, base);
  assert.deepEqual([...doc.querySelectorAll("#guide button[data-replay]")].map((b) => b.dataset.replay), [KEY, "guide:flare:flash"]);
  const status = (k) => doc.querySelector(`[data-replay-status="${k}"]`);

  doc.querySelector(`button[data-replay="${KEY}"]`).click();
  await until("launched", () => status(KEY).textContent === "launched");
  assert.equal(status(KEY).classList.contains("err"), false);
  assert.equal(spawn.calls.length, 1);
  assert.deepEqual(spawn.calls[0].config, REPLAY);

  fail = true;
  doc.querySelector('button[data-replay="guide:flare:flash"]').click();
  await until("the exit", () => status("guide:flare:flash").textContent === "the game exited with code 2");
  assert.equal(status("guide:flare:flash").classList.contains("err"), true);
  assert.match(doc.querySelector(".replaytail").textContent, /Invalid replay config: map Nagrand/);
  assert.equal(status(KEY).textContent, "launched", "each check keeps its own status");
});

test("review page: ▶ with no binary says to build first", async (t) => {
  const spawn = fakeSpawn();
  const { base } = await daemon(t, { spawn: spawn.fn, binary: "/no/such/arenasim" });
  const doc = await openReview(t, base);
  doc.querySelector(`button[data-replay="${KEY}"]`).click();
  const st = () => doc.querySelector(`[data-replay-status="${KEY}"]`);
  await until("the error", () => /build first: cargo build --release/.test(st().textContent));
  assert.equal(st().classList.contains("err"), true);
  assert.equal(spawn.calls.length, 0);
});
