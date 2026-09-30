// Shared scaffolding: temp databases, a spawned daemon, an MCP client.
import { mkdtempSync, rmSync, existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import { Board, HUMAN_REVIEW_MIGRATION_NOTE } from "../dist/board.js";
import { mainCheckoutRoot } from "../dist/paths.js";

export const PKG = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const CLI = join(PKG, "dist", "cli.js");

/** A fresh temp dir; removed by the returned cleanup. */
export function tempDir() {
  const dir = mkdtempSync(join(tmpdir(), "dispatch-board-test-"));
  return { dir, db: join(dir, "board.db"), cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

/** A Board over a temp DB, closed and deleted by t.after. */
export function tempBoard(t) {
  const tmp = tempDir();
  const board = new Board(tmp.db);
  t.after(() => {
    board.close();
    tmp.cleanup();
  });
  return { board, ...tmp };
}

/**
 * A card taken straight to `column` with the given own PR, for rule tests.
 * `agent` is "working" or "done": the claim is taken the only way one can be
 * — claim_card — and "done" then closes it out.
 */
export function seed(board, { role = "engineer", column = "backlog", pr = null, agent } = {}) {
  let c = board.createCard({ title: "t", body: "b", role, pr }, { actor: "seed" });
  // Archived means shipped: a card gets there with its release tag.
  if (column !== "backlog") c = board.moveCard(c.id, column, c.version, { actor: "seed", ...(column === "archived" ? { patch: { released: "v0.0.1" } } : {}) });
  if (agent === "working" || agent === "done") c = board.claimCard(c.id, "Seeded-Agent", { actor: "seed" });
  if (agent === "done") c = board.finishClaim(c.id, { actor: "seed" });
  return c;
}

/**
 * Spawn the real daemon on an ephemeral port over a temp DB. Resolves once
 * it is listening; `stop()` sends SIGTERM and waits for exit.
 */
export async function spawnDaemon(t, extraEnv = {}) {
  const tmp = tempDir();
  const child = spawn(process.execPath, [CLI, "serve", "--db", tmp.db, "--port", "0"], {
    env: { ...process.env, ...extraEnv },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  const port = await new Promise((res, rej) => {
    const timer = setTimeout(() => rej(new Error(`daemon did not start:\n${stderr}`)), 15000);
    child.stderr.on("data", (d) => {
      stderr += d;
      const m = /on http:\/\/127\.0\.0\.1:(\d+)/.exec(stderr);
      if (m) {
        clearTimeout(timer);
        res(Number(m[1]));
      }
    });
    child.on("exit", (code) => rej(new Error(`daemon exited ${code}:\n${stderr}`)));
  });
  const stop = () =>
    new Promise((res) => {
      // A signal-killed child has exitCode null and signalCode set: either means it is gone.
      if (child.exitCode !== null || child.signalCode !== null) return res();
      child.once("exit", () => res());
      child.kill("SIGTERM");
    });
  t.after(async () => {
    await stop();
    tmp.cleanup();
  });
  return { port, base: `http://127.0.0.1:${port}`, db: tmp.db, child, stop };
}

/**
 * The export an imported SCHEMA 1 state must come back as: schema 2 with no
 * milestones, and every human_review card a merged card, re-stamped `updated`
 * and carrying one extra activity line — the migration note, by "migrate".
 * Both times are the migration's, so they are read from `output` (and the
 * line's text and author checked).
 */
export function asSchema2(input, output) {
  const byId = new Map(output.cards.map((c) => [c.id, c]));
  return {
    schema: 2,
    nextId: input.nextId,
    cards: input.cards.map((c) => {
      if (c.column !== "human_review") return c;
      const line = byId.get(c.id)?.activity?.at(-1);
      if (!line || line.by !== "migrate" || line.msg !== HUMAN_REVIEW_MIGRATION_NOTE) {
        throw new Error(`${c.id}: expected the migration line last in its activity, got ${JSON.stringify(line)}`);
      }
      const updated = byId.get(c.id).updated;
      if (!/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d$/.test(updated)) throw new Error(`${c.id}: updated ${updated} is not a board timestamp`);
      return { ...c, column: "merged", updated, activity: [...c.activity, line] };
    }),
    milestones: [],
  };
}

export async function mcpClient(t, base) {
  const client = new Client({ name: "test", version: "0" });
  await client.connect(new StreamableHTTPClientTransport(new URL(`${base}/mcp`)));
  t.after(() => client.close());
  return client;
}

/** Call a tool; returns {ok, value} with the JSON payload parsed. */
export async function call(client, name, args) {
  const r = await client.callTool({ name, arguments: args });
  return { ok: !r.isError, value: JSON.parse(r.content[0].text) };
}

/** Run `cli.js wait ...` and resolve with its stdout lines and exit code. */
export function runWait(args) {
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

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function post(base, path, body) {
  const r = await fetch(`${base}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  return { status: r.status, body: await r.json() };
}

/**
 * The real saved artifact page, when this machine has it. It is gitignored
 * (it holds the whole live board), so a checkout without it skips the
 * real-state round trip with a message and still runs the synthetic one.
 */
export function realBoardPage() {
  const main = mainCheckoutRoot(resolve(PKG, "..", ".."));
  const candidates = [process.env.DISPATCH_BOARD_FIXTURE, join(main, ".claude", "pm-outbox", "dispatch-board-artifact.html")];
  return candidates.find((p) => p && existsSync(p));
}

/**
 * The real board's most recent export, when this machine has one: the
 * cutover's own backup directory (<main checkout>/.dispatch/backups/*.json),
 * or $DISPATCH_BOARD_EXPORT_FIXTURE. Read in place, never copied into the repo.
 */
export function realBoardExport() {
  if (process.env.DISPATCH_BOARD_EXPORT_FIXTURE) return process.env.DISPATCH_BOARD_EXPORT_FIXTURE;
  const dir = join(mainCheckoutRoot(resolve(PKG, "..", "..")), ".dispatch", "backups");
  if (!existsSync(dir)) return undefined;
  const files = readdirSync(dir)
    .filter((f) => f.endsWith(".json"))
    .map((f) => join(dir, f))
    .filter((f) => {
      try {
        return JSON.parse(readFileSync(f, "utf8")).schema === 1;
      } catch {
        return false;
      }
    });
  return files.sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
}
