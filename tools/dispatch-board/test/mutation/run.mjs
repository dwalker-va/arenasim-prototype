// Mutation check: remove each guard from the BUILT code and prove the suite
// notices. A guard whose removal leaves the tests green is guarded by nothing.
//
//   npm run build && npm run test:mutation
//
// Each mutant is a copy of dist/, ui/ and test/ under .mutants/<name>/ (inside
// the package, so node_modules still resolves) with one exact source edit
// applied — the edit must match exactly once, so a refactor that moves a guard
// fails this script loudly instead of silently mutating nothing. The mutant's
// suite must FAIL, and must fail in the tests named for that guard.
//
// A kill only counts if the named tests fail BECAUSE of the mutation. So the
// copies see the real repository — the checkout's packaging/ icon and the
// saved board fixture, which a two-deep .mutants/ copy cannot reach by
// relative path — and an UNMUTATED copy in the same layout runs first as the
// control: every test any mutant names must PASS there, or no mutant counts.
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const PKG = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const { packagingIconDir } = await import("../../dist/paths.js");
const { realBoardPage } = await import("../helpers.mjs");

// What the package resolves from its real place, handed to every copy explicitly.
const ENV = {
  ...process.env,
  DISPATCH_BOARD_PACKAGING_DIR: packagingIconDir(),
  ...(realBoardPage() ? { DISPATCH_BOARD_FIXTURE: realBoardPage() } : {}),
};
const SUITES = ["board", "race", "daemon", "roundtrip", "ui", "prwork"];

/** A copy of the package's dist/ui/test under .mutants/<name>/, optionally with one edit applied. */
function copyTo(name) {
  const dir = join(PKG, ".mutants", name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  for (const sub of ["dist", "ui", "test"]) cpSync(join(PKG, sub), join(dir, sub), { recursive: true });
  return dir;
}

function runSuite(dir) {
  const tests = SUITES.map((n) => join(dir, "test", `${n}.test.mjs`));
  const r = spawnSync(process.execPath, ["--test", ...tests], { encoding: "utf8", cwd: dir, env: ENV });
  const lines = [...r.stdout.matchAll(/^(not ok|ok) \d+ - (.*)$/gm)];
  return {
    status: r.status,
    failed: lines.filter((l) => l[1] === "not ok").map((l) => l[2]),
    passed: lines.filter((l) => l[1] === "ok" && !/ # SKIP\b/.test(l[2])).map((l) => l[2]),
  };
}

const MUTANTS = [
  {
    name: "claim-guard",
    file: "dist/board.js",
    find: "const why = claimRefusal(doc);",
    replace: "const why = null;",
    mustFail: [
      "claim guard: a second claim on the same card fails",
      "MCP: concurrent claims from separate clients resolve to exactly one winner",
      "concurrent claims:",
    ],
  },
  {
    name: "version-guard",
    file: "dist/board.js",
    find: "if (r.version !== expected) {",
    replace: "if (false) {",
    mustFail: [
      "version guard: a stale expected_version is refused with the current card",
      "version guard: stale move_card and answer_question are refused too",
      "MCP: a stale version is refused as a tool error carrying the current card",
    ],
  },
  {
    name: "pr-gate",
    file: "dist/board.js",
    find: 'return !GATED_COLUMNS.includes(to) || doc.role === "pm" || hasPr(doc);',
    replace: "return true;",
    mustFail: [
      "PR gate: a non-pm card without its own pr is refused into review and human_review",
      "MCP: the PR gate refuses an engineer card without its own pr and admits a pm card",
      "PR gate: reference links alone never admit a card to review or human_review",
    ],
  },
  {
    name: "own-write-filter",
    file: "dist/board.js",
    find: ".filter((r) => !ignore.has(r.actor))",
    replace: "",
    mustFail: [
      "events_since does not echo the caller's own writes, and its cursor skips past them",
      "wake-up: a UI drag wakes a waiter; the orchestrator's own writes do not",
    ],
  },
  {
    name: "delete-version-check",
    file: "dist/board.js",
    find: "checkVersion(expectedVersion);\n        this.write(() => {",
    replace: "this.write(() => {",
    mustFail: [
      "version guard: every versioned write refuses a missing or non-integer expected_version",
      "web UI API: a delete without expected_version is refused and deletes nothing",
    ],
  },
  {
    name: "working-only-via-claim",
    file: "dist/board.js",
    find: 'if (v !== null && !(isObj(v) && v.status === "done")) {',
    replace: 'if (v !== null && !(isObj(v) && (v.status === "working" || v.status === "done"))) {',
    mustFail: ["claim rule: agent.status working comes only from claim_card, never a patch"],
  },
  {
    name: "drawer-base-version",
    file: "ui/board.html",
    find: "var id = c.id, expected = base.version, req;",
    replace: "var id = c.id, expected = c.version, req;",
    mustFail: ["drawer: an edit that a foreign write overtakes is refused, not written over it"],
  },
  {
    name: "drawer-restores-only-edits",
    file: "ui/board.html",
    find: 'var keep = k !== "drawer" || keepDrawer.indexOf(f) >= 0;',
    replace: "var keep = true;",
    mustFail: ["drawer: a foreign write seen before editing survives a one-field save"],
  },
  {
    // The round-2 rule: edits diffed against the held base, not the rendered value.
    name: "drawer-dirty-tracking",
    file: "ui/board.html",
    find: "if(v !== undefined && rendered[f] !== undefined && v !== rendered[f]) out[k] = v;",
    replace: "if(v !== undefined && base && v !== base[k]) out[k] = v;",
    mustFail: [
      "drawer: a field the user never touched follows EVERY refresh, and is never sent",
      "drawer: a spec with a leading newline is not an edit the user made",
      "drawer: a CRLF spec is not an edit the user made",
    ],
  },
  {
    // Round 3: Keep adopts the new version but leaves edits measured against the old one.
    name: "keep-rebaselines-edits",
    file: "ui/board.html",
    find: 'Object.keys(edited).forEach(function(k){ var el = tmp.querySelector("#" + DRAWER[k]); if(el) rendered[DRAWER[k]] = el.value; });',
    replace: "",
    mustFail: [
      "drawer: after Keep, setting priority back to its pre-conflict value is sent",
      "drawer: after Keep, setting column back to its pre-conflict value is sent",
    ],
  },
  {
    // Round 5: a card's own PR comes from its hand-off record, never from any activity naming a PR.
    name: "migration-handoff-only",
    file: "dist/board.js",
    find: 'if ((a.by === "engineer" && /^READY/.test(a.msg)) || (a.by === "orchestrator" && /^(ENGINEER DONE|READY FOR REVIEW)/.test(a.msg))) {',
    replace: "if (true) {",
    mustFail: ["migration: pr is derived from the Engineer's own hand-off, never from reference links"],
  },
  {
    // Round 6: a move record counts only when its PR is the move's own subject, not any PR it mentions.
    name: "moved-record-subject-only",
    file: "dist/board.js",
    find: "const m = /^Moved: in_progress (?:->|→) review\\. PR #(\\d+)\\b/.exec(a.msg);",
    replace: "const m = /^Moved: in_progress (?:->|→) review[\\s\\S]*?PR #(\\d+)\\b/.exec(a.msg);",
    mustFail: ["migration: the orchestrator's move-to-review record counts only when the PR is its subject"],
  },
  {
    name: "attach-dialog-sets-pr",
    file: "ui/board.html",
    find: "var patch = Object.assign({}, p.patch, {pr: {url: u}});",
    replace: 'var patch = Object.assign({}, p.patch, {links: (c.links || []).concat([{label: "PR", url: u}])});',
    mustFail: ["attach dialog: a card with only reference links is gated, and the dialog sets its own pr"],
  },
  {
    name: "claim-records-worktree",
    file: "dist/board.js",
    find: "if (worktree !== undefined)\n                doc.worktree = worktree;",
    replace: "",
    mustFail: [
      "READY_FOR_REVIEW: pr, worktree, the claim close-out and the move are ONE write",
      "worktree: set at claim, re-set by a later round's claim, absolute paths only, never checked on disk",
    ],
  },
  {
    // The tab icon must be the game's own packaging file, not a stand-in.
    name: "tab-icon-source",
    file: "dist/server.js",
    find: '"/favicon.svg": ["icon.svg", "image/svg+xml"],',
    replace: '"/favicon.svg": [join("icon", "icon_16.png"), "image/svg+xml"],',
    mustFail: ["tab icon: the daemon serves the game's own packaging icon, and the page links it SVG-first"],
  },
  {
    name: "signal-handler-ordering",
    file: "dist/cli.js",
    find: 'process.on("SIGINT", stop);\n    process.on("SIGTERM", stop);\n    await ready;',
    replace: 'await ready;\n    process.on("SIGINT", stop);\n    process.on("SIGTERM", stop);',
    mustFail: ["shutdown: a SIGTERM the instant the daemon says it is serving is handled gracefully"],
  },
];

// The control: the unmutated copy, same layout, same environment.
const control = runSuite(copyTo("control"));
rmSync(join(PKG, ".mutants", "control"), { recursive: true, force: true });
const named = [...new Set(MUTANTS.flatMap((m) => m.mustFail))];
const notPassing = named.filter((n) => !control.passed.some((p) => p.startsWith(n)));
if (control.status !== 0 || notPassing.length) {
  console.log("FAIL control: the UNMUTATED copy is not green, so no kill below could be attributed to its mutation");
  for (const f of control.failed) console.log(`       failing: ${f}`);
  for (const n of notPassing) console.log(`       not passing (failing, skipped or missing): ${n}`);
  rmSync(join(PKG, ".mutants"), { recursive: true, force: true });
  process.exit(1);
}
console.log(`ok   control: the unmutated copy passes all ${control.passed.length} tests, including all ${named.length} a mutant names`);

let failures = 0;
for (const m of MUTANTS) {
  const dir = copyTo(m.name);
  const target = join(dir, m.file);
  const src = readFileSync(target, "utf8");
  const hits = src.split(m.find).length - 1;
  if (hits !== 1) {
    console.log(`FAIL ${m.name}: expected exactly 1 match of the mutation site in ${m.file}, found ${hits} (did the guard move?)`);
    failures++;
    continue;
  }
  writeFileSync(target, src.replace(m.find, m.replace));

  const r = runSuite(dir);
  const failed = r.failed;
  const missing = m.mustFail.filter((name) => !failed.some((f) => f.startsWith(name)));
  if (r.status === 0 || missing.length) {
    console.log(`FAIL ${m.name}: the suite did not catch this mutant`);
    for (const name of missing) console.log(`       still passing: ${name}`);
    failures++;
  } else {
    console.log(`ok   ${m.name}: killed by ${failed.length} failing test(s):`);
    for (const f of failed) console.log(`       - ${f}`);
  }
  rmSync(dir, { recursive: true, force: true });
}
rmSync(join(PKG, ".mutants"), { recursive: true, force: true });
process.exit(failures ? 1 : 0);
