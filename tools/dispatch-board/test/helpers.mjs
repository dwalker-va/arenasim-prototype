// Shared scaffolding: temp databases, a spawned daemon, an MCP client.
import { mkdtempSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import { Board } from "../dist/board.js";
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

/** A card taken straight to `column` with the given agent/links, for rule tests. */
export function seed(board, { role = "engineer", column = "backlog", links = [], agent } = {}) {
  let c = board.createCard({ title: "t", body: "b", role, links }, { actor: "seed" });
  if (column !== "backlog") c = board.moveCard(c.id, column, c.version, { actor: "seed" });
  if (agent !== undefined) c = board.updateCard(c.id, { agent }, c.version, { actor: "seed" });
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
      if (child.exitCode !== null) return res();
      child.once("exit", () => res());
      child.kill("SIGTERM");
    });
  t.after(async () => {
    await stop();
    tmp.cleanup();
  });
  return { port, base: `http://127.0.0.1:${port}`, db: tmp.db, child, stop };
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
