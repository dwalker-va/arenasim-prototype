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
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const PKG = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

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
    name: "pr-link-gate",
    file: "dist/board.js",
    find: 'return !GATED_COLUMNS.includes(to) || doc.role === "pm" || hasLinks(doc);',
    replace: "return true;",
    mustFail: [
      "PR-link gate: a linkless non-pm card is refused into review and human_review",
      "MCP: the PR-link gate refuses a linkless engineer card and admits a pm card",
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
];

let failures = 0;
for (const m of MUTANTS) {
  const dir = join(PKG, ".mutants", m.name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  for (const sub of ["dist", "ui", "test"]) cpSync(join(PKG, sub), join(dir, sub), { recursive: true });
  const target = join(dir, m.file);
  const src = readFileSync(target, "utf8");
  const hits = src.split(m.find).length - 1;
  if (hits !== 1) {
    console.log(`FAIL ${m.name}: expected exactly 1 match of the mutation site in ${m.file}, found ${hits} (did the guard move?)`);
    failures++;
    continue;
  }
  writeFileSync(target, src.replace(m.find, m.replace));

  const tests = ["board", "race", "daemon", "roundtrip"].map((n) => join(dir, "test", `${n}.test.mjs`));
  const r = spawnSync(process.execPath, ["--test", ...tests], { encoding: "utf8", cwd: dir });
  const failed = [...r.stdout.matchAll(/^not ok \d+ - (.*)$/gm)].map((x) => x[1]);
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
