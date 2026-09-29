// The cutover: a schema 1 (AS-153) database brought to schema 2 in place.
// Every assertion here compares the raw database before and after, so "the
// migration changed nothing else" is checked row by row rather than trusted:
// each human_review card becomes merged (+1 version, +1 activity line), one
// `migrated` event is appended, the schema goes to 2 — and every other card,
// activity row, event and meta value is byte-for-byte what it was. The last
// test runs the same checks over a copy of the REAL exported board.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import Database from "better-sqlite3";
import { Board, BoardError, HUMAN_REVIEW_MIGRATION_NOTE } from "../dist/board.js";
import { tempDir, asSchema2, realBoardExport, CLI } from "./helpers.mjs";
import { legacyDb, dumpDb } from "./legacy.mjs";

const act = (t, by, msg) => ({ t, by, msg });
const base = { body: "", role: "engineer", priority: "P2", links: [], worktree: null, question: null, agent: null, created: "2026-09-20T00:00:00", updated: "2026-09-20T00:00:00" };
const PR = (n) => ({ number: n, url: `https://github.com/o/r/pull/${n}` });

/** An AS-153 board mid-flight: two cards awaiting the user's merge, the rest elsewhere. */
function legacyState() {
  return {
    schema: 1,
    nextId: 7,
    cards: [
      { id: "AS-1", title: "shipped", ...base, column: "archived", pr: PR(1), released: "v0.5.0", activity: [act("2026-09-20T00:00:00", "board", "Created")] },
      { id: "AS-2", title: "approved, awaiting merge", ...base, column: "human_review", pr: PR(2), agent: { status: "done", started: "s", finished: "f", name: "AS-2-test" }, activity: [act("2026-09-20T00:00:00", "board", "Created"), act("2026-09-21T00:00:00", "tester", "APPROVE")] },
      { id: "AS-3", title: "merged, awaiting release", ...base, column: "done", pr: PR(3), activity: [] },
      { id: "AS-4", title: "second approved card", ...base, column: "human_review", pr: PR(4), activity: [act("2026-09-22T00:00:00", "tester", "APPROVE")] },
      { id: "AS-5", title: "in review", ...base, column: "review", pr: PR(5), activity: [] },
      { id: "AS-6", title: "legacy shape", column: "backlog", role: "pm", agent: "a bare string", body: "x", created: "c", updated: "u", activity: [] },
    ],
  };
}
const HISTORY = [
  { t: "2026-09-21T00:00:00", actor: "orchestrator", kind: "moved", card: "AS-2", data: { from: "review", to: "human_review" } },
  { t: "2026-09-22T00:00:00", actor: "board", kind: "edited", card: "AS-5", data: { fields: ["title"] } },
];

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
}

/**
 * The whole before/after comparison. `moved` are the ids that must have gone
 * human_review -> merged; everything else must be untouched.
 */
function assertMigrated(before, after, moved) {
  const tables = after.master.filter((r) => r.type === "table").map((r) => r.name);
  assert.ok(tables.includes("milestones") && tables.includes("review_items"), "the milestone tables exist");
  assert.deepEqual(
    after.master.filter((r) => before.master.some((b) => b.name === r.name)),
    before.master,
    "every schema 1 table, index and trigger is unchanged",
  );
  assert.deepEqual(after.meta, { ...before.meta, schema: "2" }, "only the schema version changed in meta (board_id kept, so cursors stay valid)");

  assert.equal(after.cards.length, before.cards.length);
  const movedNow = [];
  after.cards.forEach((row, i) => {
    const was = before.cards[i];
    assert.equal(row.id, was.id);
    if (!moved.includes(row.id)) {
      assert.deepEqual(row, was, `${row.id} must be byte-for-byte untouched`);
      return;
    }
    movedNow.push(row.id);
    const d0 = JSON.parse(was.doc), d1 = JSON.parse(row.doc);
    assert.equal(d0.column, "human_review");
    assert.equal(row.version, was.version + 1);
    assert.equal(d1.column, "merged");
    assert.equal(row.doc, JSON.stringify({ ...d0, column: "merged", updated: d1.updated }), `${row.id}: only column and updated changed, key order kept`);
    assert.equal(d1.milestone, undefined, "a migrated card is on no milestone");
    assert.equal(d1.merge_sha, undefined, "no merge is invented: mark_merged records it");
  });
  assert.deepEqual(movedNow, moved);

  const extra = after.activity.filter((a) => !before.activity.some((b) => b.card_id === a.card_id && b.seq === a.seq));
  assert.deepEqual(after.activity.filter((a) => !extra.includes(a)), before.activity, "every existing activity row is unchanged");
  assert.deepEqual(extra.map((a) => [a.card_id, a.by, a.msg]).sort(), moved.map((id) => [id, "migrate", HUMAN_REVIEW_MIGRATION_NOTE]).sort());
  for (const a of extra) {
    const last = Math.max(0, ...before.activity.filter((b) => b.card_id === a.card_id).map((b) => b.seq));
    assert.equal(a.seq, last + 1, "appended after the card's existing activity");
  }

  assert.deepEqual(after.events.slice(0, before.events.length), before.events, "every existing event is unchanged, cursors included");
  const added = after.events.slice(before.events.length);
  assert.deepEqual(added.map((e) => [e.actor, e.kind, e.card_id, JSON.parse(e.data)]), [["migrate", "migrated", null, { from: 1, to: 2, human_review_to_merged: moved }]]);
}

test("a schema 1 database is refused by this build until migrated — and the refusal writes nothing", (t) => {
  const tmp = tempDir();
  t.after(tmp.cleanup);
  legacyDb(tmp.db, legacyState(), HISTORY);
  const before = dumpDb(tmp.db);
  refused(() => new Board(tmp.db), "needs_migration", /schema 1; this build needs 2.*migrate/);
  assert.deepEqual(dumpDb(tmp.db), before);

  // The daemon refuses to serve it, naming the step, and leaves it alone.
  const serve = spawnSync(process.execPath, [CLI, "serve", "--db", tmp.db, "--port", "0"], { encoding: "utf8", timeout: 15000 });
  assert.notEqual(serve.status, 0);
  assert.match(serve.stderr, /needs_migration: .*dist\/cli\.js migrate/);
  assert.deepEqual(dumpDb(tmp.db), before);

  // A database NEWER than the build (a rollback run against a migrated db) is refused too.
  const newer = tempDir();
  t.after(newer.cleanup);
  new Board(newer.db).close();
  const db = new Database(newer.db);
  db.prepare("UPDATE meta SET value = '3' WHERE key = 'schema'").run();
  db.close();
  refused(() => new Board(newer.db), "needs_migration", /newer than this build/);
  refused(() => new Board(newer.db, { readonly: true }), "needs_migration", /newer than this build/);
});

test("export of an unmigrated database is the schema 1 export it always was (the pre-cutover backup)", (t) => {
  const tmp = tempDir();
  t.after(tmp.cleanup);
  const state = legacyState();
  legacyDb(tmp.db, state);
  const out = join(tmp.dir, "backup.json");
  const r = spawnSync(process.execPath, [CLI, "export", "--db", tmp.db, "--out", out], { encoding: "utf8" });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(JSON.stringify(JSON.parse(readFileSync(out, "utf8"))), JSON.stringify(state));
});

test("migrate: human_review cards become merged cards on no milestone; nothing else in the database changes", async (t) => {
  const tmp = tempDir();
  t.after(tmp.cleanup);
  const id = legacyDb(tmp.db, legacyState(), HISTORY);
  // A deleted human_review card is migrated too, so no row is left in a column this build does not know.
  const raw = new Database(tmp.db);
  raw.prepare("UPDATE cards SET deleted_at = '2026-09-23T00:00:00' WHERE id = 'AS-4'").run();
  raw.close();
  const before = dumpDb(tmp.db);
  const backup = join(tmp.dir, "pre.bak");

  const r = await Board.migrate(tmp.db, { backup });
  assert.deepEqual(r, { from: 1, to: 2, backup, human_review_to_merged: ["AS-2", "AS-4"], cards: 6 });
  assertMigrated(before, dumpDb(tmp.db), ["AS-2", "AS-4"]);

  // The backup IS the pre-migration database: restoring it is the rollback.
  const saved = dumpDb(backup);
  assert.deepEqual(saved, before);
  assert.equal(saved.meta.board_id, id);

  // Idempotent: a second run is a no-op that writes no second backup.
  const after = dumpDb(tmp.db);
  assert.deepEqual(await Board.migrate(tmp.db), { from: 2, to: 2, backup: null, human_review_to_merged: [], cards: 6 });
  assert.deepEqual(dumpDb(tmp.db), after);

  // The migrated board serves: the approved card awaits its merge record, and takes it.
  const board = new Board(tmp.db);
  t.after(() => board.close());
  const c = board.getCard("AS-2");
  assert.deepEqual([c.column, c.agent.status, c.activity.at(-1).msg], ["merged", "done", HUMAN_REVIEW_MIGRATION_NOTE]);
  assert.equal(board.markMerged("AS-2", { pr: { url: PR(2).url }, merge_sha: "f".repeat(40) }, c.version, { actor: "orchestrator" }).merge_sha, "f".repeat(40));
  assert.equal(board.boardId(), id);
  assert.deepEqual(board.exportState().milestones, []);
});

test("migrate CLI: refused while a daemon owns the db or the backup name is taken; otherwise prints its report", (t) => {
  const tmp = tempDir();
  t.after(tmp.cleanup);
  legacyDb(tmp.db, legacyState());
  const before = dumpDb(tmp.db);
  writeFileSync(`${tmp.db}.daemon.json`, JSON.stringify({ pid: process.pid, port: 1, started: "now" }));
  const live = spawnSync(process.execPath, [CLI, "migrate", "--db", tmp.db], { encoding: "utf8" });
  assert.notEqual(live.status, 0);
  assert.match(live.stderr, /stop it before migrating/);
  assert.deepEqual(dumpDb(tmp.db), before);
  writeFileSync(`${tmp.db}.daemon.json`, JSON.stringify({ pid: 999999, port: 1, started: "then" })); // a dead daemon's stale lock

  const taken = join(tmp.dir, "taken.bak");
  writeFileSync(taken, "x");
  const clash = spawnSync(process.execPath, [CLI, "migrate", "--db", tmp.db, "--backup", taken], { encoding: "utf8" });
  assert.notEqual(clash.status, 0);
  assert.match(clash.stderr, /already exists/);
  assert.deepEqual(dumpDb(tmp.db), before);

  const ok = spawnSync(process.execPath, [CLI, "migrate", "--db", tmp.db], { encoding: "utf8" });
  assert.equal(ok.status, 0, ok.stderr);
  const report = JSON.parse(ok.stdout);
  assert.deepEqual(report.migrated.human_review_to_merged, ["AS-2", "AS-4"]);
  assert.match(report.migrated.backup, /\.schema1-\d{4}-\d\d-\d\dT\d{6}\.bak$/);
  assert.ok(existsSync(report.migrated.backup));
  assert.deepEqual(dumpDb(report.migrated.backup), before);
});

test("migrate over the REAL exported board: planted human_review cards move, every other card is untouched", async (t) => {
  const file = realBoardExport();
  if (!file) {
    t.skip("no real board export on this machine (<main checkout>/.dispatch/backups/*.json or $DISPATCH_BOARD_EXPORT_FIXTURE); the synthetic migration above still ran");
    return;
  }
  const state = JSON.parse(readFileSync(file, "utf8"));
  assert.equal(state.schema, 1);
  // The live board rarely has a card waiting in human_review, so plant three
  // on real work cards — otherwise "every human_review card moved" would be
  // vacuously true. Prefer merged-awaiting-release cards: that is the shape a
  // card had while it waited for the user's merge.
  const work = state.cards.filter((c) => c.role !== "pm" && c.pr);
  const pool = [...work.filter((c) => c.column === "done"), ...work.filter((c) => c.column !== "done")];
  const planted = pool.slice(0, 3).map((c) => c.id);
  assert.equal(planted.length, 3, "the real board has fewer than three work cards with a PR");
  for (const c of state.cards) if (planted.includes(c.id)) c.column = "human_review";
  const already = state.cards.filter((c) => c.column === "human_review").map((c) => c.id);
  t.diagnostic(`real board ${file}: ${state.cards.length} cards; human_review after planting: ${already.join(", ")}`);

  const tmp = tempDir();
  t.after(tmp.cleanup);
  legacyDb(tmp.db, state);
  const before = dumpDb(tmp.db);
  const r = await Board.migrate(tmp.db, { backup: join(tmp.dir, "pre.bak") });
  const moved = state.cards.filter((c) => c.column === "human_review").map((c) => c.id);
  assert.deepEqual(r.human_review_to_merged, moved);
  assertMigrated(before, dumpDb(tmp.db), moved);

  // And the migrated board exports as the input, in schema 2 shape — key order included.
  const board = new Board(tmp.db, { readonly: true });
  const out = board.exportState();
  board.close();
  assert.equal(JSON.stringify(out), JSON.stringify(asSchema2(state, out)));
});
