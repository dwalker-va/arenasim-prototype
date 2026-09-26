// A card's OWN pull request (`pr`) and the worktree its branch is checked out
// in (`worktree`). `links` are references — a prerequisite PR, the PR where a
// finding was made — so the review gate and the Tester spawn read `pr`, never
// `links`. `worktree` is informational: recorded, shown, never checked.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { BoardError, derivePr, extractStateFromHtml } from "../dist/board.js";
import { tempBoard, tempDir, realBoardPage, spawnDaemon, mcpClient, call, CLI } from "./helpers.mjs";

const PR = (n) => ({ url: `https://github.com/o/r/pull/${n}` });
const act = (t, by, msg) => ({ t, by, msg });

function refused(fn, code) {
  let err;
  try {
    fn();
  } catch (e) {
    err = e;
  }
  assert.ok(err instanceof BoardError, `expected a BoardError(${code}), got ${err === undefined ? "success" : err}`);
  assert.equal(err.code, code, err.message);
  return err;
}

function inProgress(board, extra = {}) {
  let c = board.createCard({ title: "t", body: "b", role: "engineer", ...extra }, { actor: "o" });
  return board.moveCard(c.id, "in_progress", c.version, { actor: "o" });
}

// ---------------------------------------------------------------- the gate

test("PR gate: reference links alone never admit a card to review or human_review", (t) => {
  const { board } = tempBoard(t);
  const refs = [{ label: "PR #154 (prerequisite)", url: "https://github.com/o/r/pull/154" }];
  for (const to of ["review", "human_review"]) {
    const c = inProgress(board, { links: refs });
    refused(() => board.moveCard(c.id, to, c.version, { actor: "o" }), "gate_refused");
    refused(() => board.moveCard(c.id, to, c.version, { actor: "o", patch: { links: [...refs, ...refs] } }), "gate_refused");
    const moved = board.moveCard(c.id, to, c.version, { actor: "o", patch: { pr: PR(7) } });
    assert.deepEqual(moved.pr, { number: 7, url: "https://github.com/o/r/pull/7" });
    assert.deepEqual(moved.links, refs, "the card's own PR is not a reference link");
  }
  refused(() => board.createCard({ title: "x", role: "engineer", column: "review", links: refs }, { actor: "o" }), "gate_refused");
  const pm = board.createCard({ title: "p", role: "pm" }, { actor: "o" });
  assert.equal(board.moveCard(pm.id, "review", pm.version, { actor: "o" }).column, "review", "pm cards stay exempt");
});

test("PR gate: pr must be a pull-request URL, and a gated card cannot lose it", (t) => {
  const { board } = tempBoard(t);
  const c = inProgress(board);
  for (const pr of [{ url: "" }, { url: "https://github.com/o/r/issues/3" }, { url: "not a url/pull/3" }, "https://github.com/o/r/pull/3", { url: "https://github.com/o/r/pull/3", number: 4 }]) {
    refused(() => board.updateCard(c.id, { pr }, c.version, { actor: "o" }), "invalid");
  }
  const r = board.moveCard(c.id, "review", c.version, { actor: "o", patch: { pr: PR(3) } });
  refused(() => board.updateCard(r.id, { pr: null }, r.version, { actor: "o" }), "gate_refused");
  assert.deepEqual(board.getCard(r.id).pr, { number: 3, url: "https://github.com/o/r/pull/3" });
});

test("READY_FOR_REVIEW: pr, worktree, the claim close-out and the move are ONE write", (t) => {
  const { board } = tempBoard(t);
  let c = inProgress(board);
  c = board.claimCard(c.id, "Engineer-AS-1", { actor: "orchestrator", worktree: "/abs/wt/agent-1" });
  assert.equal(c.worktree, "/abs/wt/agent-1");
  const r = board.moveCard(c.id, "review", c.version, {
    actor: "orchestrator",
    by: "engineer",
    patch: { pr: PR(12), worktree: "/abs/wt/agent-1", agent: { ...c.agent, status: "done", finished: "f" } },
  });
  assert.equal(r.version, c.version + 1);
  assert.deepEqual([r.column, r.pr.number, r.worktree, r.agent.status], ["review", 12, "/abs/wt/agent-1", "done"]);
  const [summary] = board.listCards({ column: "review" });
  assert.deepEqual(summary.pr, { number: 12, url: "https://github.com/o/r/pull/12" });
  assert.equal(summary.worktree, "/abs/wt/agent-1");
});

// ---------------------------------------------------------------- worktree

test("worktree: set at claim, re-set by a later round's claim, absolute paths only, never checked on disk", (t) => {
  const { board } = tempBoard(t);
  let c = inProgress(board);
  assert.equal(c.worktree, null, "a new card has no worktree");
  c = board.claimCard(c.id, "Engineer-1", { actor: "o", worktree: "/no/such/dir/round-1" });
  assert.equal(c.worktree, "/no/such/dir/round-1");
  c = board.moveCard(c.id, "in_progress", c.version, { actor: "o" }); // a REJECT: claim reset, tree kept
  assert.equal(c.worktree, "/no/such/dir/round-1");
  c = board.claimCard(c.id, "Engineer-2", { actor: "o", worktree: "/no/such/dir/round-2" });
  assert.equal(c.worktree, "/no/such/dir/round-2");
  c = board.claimCard(board.releaseClaim(c.id, { actor: "o" }).id, "Engineer-3", { actor: "o" });
  assert.equal(c.worktree, "/no/such/dir/round-2", "a claim that names no tree leaves the recorded one");
  for (const bad of ["relative/path", "", "/a\nb", 7]) {
    refused(() => board.updateCard(c.id, { worktree: bad }, c.version, { actor: "o" }), "invalid");
    refused(() => board.claimCard(c.id, "X", { actor: "o", worktree: bad }), "invalid");
  }
});

// ---------------------------------------------------------------- migration

const handoff = (n) => act("2026-09-01T10:00:00", "engineer", `READY_FOR_REVIEW — PR #${n}: did the thing`);

test("migration: pr is derived from the Engineer's own hand-off, never from reference links", () => {
  const cards = [
    { id: "AS-1", links: [{ label: "PR #5", url: "https://github.com/o/r/pull/5" }], activity: [handoff(5)] },
    // a later round names the same PR (and a commit): still one PR
    { id: "AS-2", links: [], activity: [handoff(6), act("t", "engineer", "READY_FOR_REVIEW (round 2) — commit abc on PR #6")] },
    // the orchestrator's record of the hand-off counts too
    { id: "AS-3", links: [], activity: [act("t", "orchestrator", "ENGINEER DONE -> `review`. PR #8 at `abc`")] },
    // a reference PR in links, no hand-off: nothing to derive
    { id: "AS-4", links: [{ label: "PR #154 (prerequisite)", url: "https://github.com/o/r/pull/154" }], activity: [] },
    // two different PRs handed off: ambiguous, so null and reported
    { id: "AS-5", links: [], activity: [handoff(9), handoff(10)] },
    // a PR named only by somebody else's message is not a hand-off
    { id: "AS-6", links: [], activity: [act("t", "tester", "READY — PR #11 looks fine"), act("t", "orchestrator", "Moved: PR #12 merged")] },
  ];
  const r = derivePr(cards);
  assert.deepEqual(r.pr, {
    "AS-1": { number: 5, url: "https://github.com/o/r/pull/5" },
    "AS-2": { number: 6, url: "https://github.com/o/r/pull/6" },
    "AS-3": { number: 8, url: "https://github.com/o/r/pull/8" },
    "AS-4": null,
    "AS-5": null,
    "AS-6": null,
  });
  assert.deepEqual(r.ambiguous, [{ id: "AS-5", candidates: [9, 10] }]);
  assert.deepEqual(r.constructed, ["AS-2", "AS-3"], "urls built from the board's single repository base are reported");
});

test("migration: import adds pr and worktree; a state that already carries them keeps them; export is exact otherwise", (t) => {
  const { board } = tempBoard(t);
  const base = { title: "t", body: "", column: "archived", role: "engineer", priority: "P2", question: null, agent: null, created: "c", updated: "u" };
  const state = {
    schema: 1,
    nextId: 4,
    cards: [
      { id: "AS-1", ...base, links: [{ label: "PR #5", url: "https://github.com/o/r/pull/5" }], activity: [handoff(5)] },
      { id: "AS-2", ...base, links: [], activity: [] },
      { id: "AS-3", ...base, links: [], activity: [], pr: { number: 2, url: "https://github.com/o/r/pull/2" }, worktree: "/kept" },
    ],
  };
  const r = board.importState(structuredClone(state), { actor: "import" });
  assert.deepEqual(r.migration, { pr_derived: 1, pr_null: 1, pr_kept: 1, ambiguous: [], constructed_url: [] });
  const out = board.exportState();
  const expected = structuredClone(state);
  expected.cards[0].pr = { number: 5, url: "https://github.com/o/r/pull/5" };
  expected.cards[0].worktree = null;
  expected.cards[1].pr = null;
  expected.cards[1].worktree = null;
  assert.equal(JSON.stringify(out), JSON.stringify(expected));

  // Re-importing that export changes nothing: the migration is idempotent.
  const { board: b2 } = tempBoard(t);
  b2.importState(structuredClone(out), { actor: "import" });
  assert.equal(JSON.stringify(b2.exportState()), JSON.stringify(out));
});

test("migration over the REAL saved board: every card gets pr and worktree, and nothing else changes", (t) => {
  const page = realBoardPage();
  if (!page) {
    t.skip("the saved artifact page is not on this machine");
    return;
  }
  const input = extractStateFromHtml(readFileSync(page, "utf8"));
  const tmp = tempDir();
  t.after(tmp.cleanup);
  const imp = spawnSync(process.execPath, [CLI, "import", page, "--db", tmp.db], { encoding: "utf8" });
  assert.equal(imp.status, 0, imp.stderr);
  const report = JSON.parse(imp.stdout).imported.migration;
  assert.equal(report.pr_derived + report.pr_null + report.pr_kept, input.cards.length);
  t.diagnostic(`real board: ${JSON.stringify(report)}`);

  const exp = spawnSync(process.execPath, [CLI, "export", "--db", tmp.db], { encoding: "utf8", maxBuffer: 1 << 28 });
  const output = JSON.parse(exp.stdout);
  const stripped = { ...output, cards: output.cards.map(({ pr, worktree, ...rest }) => rest) };
  assert.equal(JSON.stringify(stripped), JSON.stringify(input), "migration changed something besides adding pr/worktree");
  for (const c of output.cards) {
    assert.equal(c.worktree, null);
    if (c.pr !== null) assert.match(c.pr.url, new RegExp(`/pull/${c.pr.number}$`));
  }
});

// ---------------------------------------------------------------- over MCP and the UI API

test("MCP: list_cards summaries carry pr and worktree; claim_card records a worktree", async (t) => {
  const d = await spawnDaemon(t);
  const client = await mcpClient(t, d.base);
  let c = (await call(client, "create_card", { title: "c", role: "engineer", actor: "o" })).value;
  c = (await call(client, "move_card", { id: c.id, column: "in_progress", expected_version: c.version, actor: "o" })).value;
  c = (await call(client, "claim_card", { id: c.id, name: "Engineer-AS-1", worktree: "/abs/tree", actor: "orchestrator" })).value;
  const refs = await call(client, "move_card", { id: c.id, column: "review", expected_version: c.version, patch: { links: [PR(3)].map((p) => ({ label: "ref", url: p.url })) }, actor: "o" });
  assert.equal(refs.value.error, "gate_refused");
  const r = await call(client, "move_card", { id: c.id, column: "review", expected_version: c.version, patch: { pr: PR(4), agent: { ...c.agent, status: "done" } }, actor: "o" });
  assert.ok(r.ok, JSON.stringify(r.value));
  const [s] = (await call(client, "list_cards", { column: "review" })).value;
  assert.deepEqual([s.pr, s.worktree], [{ number: 4, url: "https://github.com/o/r/pull/4" }, "/abs/tree"]);
});
