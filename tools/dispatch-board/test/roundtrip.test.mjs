// import -> export equality. Two fixtures:
//  - a SYNTHETIC state (always runs) carrying every irregular shape the real
//    board has: legacy string agents, a missing priority and question, a
//    string question, P4, released tags, varied key order;
//  - the REAL saved artifact page, when this machine has it (it is gitignored
//    because it holds the whole live board, so it is read in place, never
//    copied into the repo).
// Equality is ORDERED: JSON.stringify of both sides must match, so key order
// and card order survive too, not just deep equality. Import migrates a card
// with no `pr` / `worktree` by appending them (test/prwork.test.mjs covers
// the derivation), so the expected export is the input plus those two keys.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { BoardError, extractStateFromHtml } from "../dist/board.js";
import { tempBoard, tempDir, realBoardPage, CLI } from "./helpers.mjs";

const act = (t, by, msg) => ({ t, by, msg });

/** The input as an export returns it: every card gains pr/worktree if it lacked them. */
function migrated(state, pr = {}) {
  return {
    ...state,
    cards: state.cards.map((c) => ({ ...c, ...("pr" in c ? {} : { pr: pr[c.id] ?? null }), ...("worktree" in c ? {} : { worktree: null }) })),
  };
}

export const SYNTHETIC = {
  schema: 1,
  nextId: 12,
  cards: [
    {
      id: "AS-1", title: "plain", body: "spec", column: "archived", role: "engineer", priority: "P2",
      links: [{ label: "PR #1", url: "https://github.com/o/r/pull/1" }], question: null,
      agent: { status: "done", started: "2026-09-01T10:00:00", finished: "2026-09-01T11:00:00" },
      activity: [act("2026-09-01T09:00:00", "board", "Created"), act("2026-09-01T10:00:00", "orchestrator", "Spawned")],
      created: "2026-09-01T09:00:00", updated: "2026-09-02T09:00:00", released: "v0.2.0",
    },
    {
      // legacy: agent as a bare string; created/updated before activity
      id: "AS-3", title: "legacy agent", body: "", column: "archived", role: "engineer", priority: "P3",
      links: [], question: null, agent: "AS-3-engineer", created: "2026-09-02T09:00:00", updated: "2026-09-02T09:00:00",
      activity: [act("2026-09-02T09:00:00", "claude", "Filed")],
    },
    {
      // legacy: no priority, no question; unusual key order
      id: "AS-2", title: "no priority", column: "archived", role: "engineer", agent: null, body: "x",
      activity: [], created: "2026-09-02T09:00:00", updated: "2026-09-02T09:00:00", links: [], released: "v0.3.0",
    },
    {
      id: "AS-7", title: "string question", body: "b", column: "done", role: "pm", priority: "P4",
      links: [], question: "free-text question", agent: { status: "working", started: "2026-09-03T00:00:00", name: "orchestrator(pm)" },
      activity: [act("2026-09-03T00:00:00", "Tester-AS-7", "odd author")], created: "2026-09-03T00:00:00", updated: "2026-09-03T00:00:00",
    },
    {
      id: "AS-11", title: "answered — with unicode → and \"quotes\" and </script>", body: "multi\nline\n\n## Tester findings\n\n1. x",
      column: "in_progress", role: "release-manager", priority: "P1", links: [],
      question: { text: "A or B?", answer: "B" }, agent: null,
      activity: [act("2026-09-04T00:00:00", "user", "answered")], created: "2026-09-04T00:00:00", updated: "2026-09-04T00:00:00",
    },
  ],
};

test("round trip: synthetic state with every legacy shape exports exactly as imported", (t) => {
  const { board } = tempBoard(t);
  const r = board.importState(structuredClone(SYNTHETIC), { actor: "import" });
  assert.deepEqual([r.cards, r.activity, r.nextId], [5, 5, 12]);
  assert.equal(JSON.stringify(board.exportState()), JSON.stringify(migrated(SYNTHETIC)));
});

test("import refuses a non-empty database", (t) => {
  const { board } = tempBoard(t);
  board.createCard({ title: "already here", role: "engineer" }, { actor: "x" });
  assert.throws(() => board.importState(structuredClone(SYNTHETIC), { actor: "import" }), (e) => e instanceof BoardError && e.code === "not_empty");
  assert.equal(board.listCards({ include_archived: true }).length, 1, "nothing was loaded");
});

test("import validates before writing: a bad state leaves the db empty", (t) => {
  const { board } = tempBoard(t);
  const bad = structuredClone(SYNTHETIC);
  bad.cards[3].activity.push({ t: "x", msg: "no by" });
  assert.throws(() => board.importState(bad, { actor: "import" }), /exactly \{t, by, msg\}/);
  const lowNext = { ...structuredClone(SYNTHETIC), nextId: 11 };
  assert.throws(() => board.importState(lowNext, { actor: "import" }), /nextId/);
  assert.ok(board.isEmpty());
});

test("after import, new cards continue from nextId and ids stay stable", (t) => {
  const { board } = tempBoard(t);
  board.importState(structuredClone(SYNTHETIC), { actor: "import" });
  const c = board.createCard({ title: "next", role: "engineer" }, { actor: "pm" });
  assert.equal(c.id, "AS-12");
  assert.equal(board.exportState().nextId, 13);
  assert.equal(board.getCard("AS-3").agent, "AS-3-engineer");
});

test("round trip: the REAL saved board page, via the CLI import/export", (t) => {
  const page = realBoardPage();
  if (!page) {
    t.skip("the saved artifact page (<main checkout>/.claude/pm-outbox/dispatch-board-artifact.html, or $DISPATCH_BOARD_FIXTURE) is not on this machine; the synthetic round trip above still ran");
    return;
  }
  const input = extractStateFromHtml(readFileSync(page, "utf8"));
  const tmp = tempDir();
  t.after(tmp.cleanup);
  const imp = spawnSync(process.execPath, [CLI, "import", page, "--db", tmp.db], { encoding: "utf8" });
  assert.equal(imp.status, 0, imp.stderr);
  const out = join(tmp.dir, "export.json");
  const exp = spawnSync(process.execPath, [CLI, "export", "--db", tmp.db, "--out", out], { encoding: "utf8" });
  assert.equal(exp.status, 0, exp.stderr);
  const output = JSON.parse(readFileSync(out, "utf8"));
  assert.equal(output.cards.length, input.cards.length);
  const report = JSON.parse(imp.stdout).imported.migration;
  const derived = Object.fromEntries(output.cards.map((c) => [c.id, c.pr]));
  assert.equal(Object.values(derived).filter(Boolean).length, report.pr_derived);
  assert.equal(JSON.stringify(output), JSON.stringify(migrated(input, derived)), "import -> export must equal the input plus pr/worktree, key order included");

  const again = spawnSync(process.execPath, [CLI, "import", page, "--db", tmp.db], { encoding: "utf8" });
  assert.notEqual(again.status, 0, "a second import into the now non-empty db is refused");
  assert.match(again.stderr, /EMPTY/);
});
