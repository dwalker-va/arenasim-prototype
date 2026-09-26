#!/usr/bin/env node
/**
 * dispatch-board CLI.
 *
 *   serve                          run the daemon (MCP + web UI + event feed)
 *   wait [--since N|head] [--board ID] [--ignore-actor A]... [--follow] [--timeout S]
 *                                  block for board events, one JSON line each
 *   head                           print {"cursor", "board"}: where to arm a wait from
 *   import <state.json|board.html> load a board state into an EMPTY db
 *   export [--out FILE]            write the full state as JSON
 *
 * Common flags: --db PATH (default: main checkout's .dispatch/board.db, or
 * $DISPATCH_BOARD_DB), --port N (default 7453, or $DISPATCH_BOARD_PORT).
 */
import { readFileSync, writeFileSync } from "node:fs";
import { Board, extractStateFromHtml } from "./board.js";
import { defaultDbPath, defaultPort, liveDaemon } from "./paths.js";
import { startDaemon } from "./server.js";

interface Args {
  cmd: string;
  positional: string[];
  flags: Map<string, string[]>;
}

const BOOLEAN_FLAGS = new Set(["follow"]);

function parse(argv: string[]): Args {
  const [cmd = "help", ...rest] = argv;
  const flags = new Map<string, string[]>();
  const positional: string[] = [];
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i];
    if (a.startsWith("--")) {
      const eq = a.indexOf("=");
      const name = eq > 0 ? a.slice(2, eq) : a.slice(2);
      let value: string;
      if (eq > 0) value = a.slice(eq + 1);
      else if (BOOLEAN_FLAGS.has(name)) value = "true";
      else if (i + 1 < rest.length) value = rest[++i];
      else die(`--${name} needs a value`);
      flags.set(name, [...(flags.get(name) ?? []), value]);
    } else positional.push(a);
  }
  return { cmd, positional, flags };
}

function flag(a: Args, name: string): string | undefined {
  const v = a.flags.get(name);
  return v ? v[v.length - 1] : undefined;
}

function die(msg: string, code = 1): never {
  process.stderr.write(`dispatch-board: ${msg}\n`);
  process.exit(code);
}

function out(obj: unknown): void {
  process.stdout.write(`${JSON.stringify(obj)}\n`);
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function cmdServe(a: Args): Promise<void> {
  const ready = startDaemon({ dbPath: flag(a, "db") ?? defaultDbPath(), port: Number(flag(a, "port") ?? defaultPort()) });
  // Handlers go in BEFORE the daemon can announce itself: a SIGTERM that
  // arrives the moment it says "serving" must still close it and its lock.
  const stop = () => {
    void ready.then((d) => d.close()).then(
      () => process.exit(0),
      () => process.exit(0),
    );
  };
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
  await ready;
}

/**
 * Block for events and print each as ONE JSON line on stdout — the shape
 * Claude Code's Monitor tool turns into one notification per line.
 *
 * One-shot (default): print the first batch, exit 0. `--timeout S` exits 0
 * silently after S seconds with nothing new.
 * `--follow`: never exit; print every event as it lands.
 *
 * An unreachable daemon is printed as a line too (`{"error":
 * "daemon_unreachable"}`) — silence must never mean "down". One-shot mode
 * then exits 2; follow mode retries and prints `{"reconnected": true}`.
 *
 * A cursor that no longer means anything is printed too, never waited on in
 * silence: past the board's newest event (`{"error": "cursor_ahead"}`), or
 * from a board the daemon no longer serves (`{"error": "board_replaced"}` —
 * the db was re-created, which restarts cursors). Both resume from the
 * board's head; the line tells the reader to re-read the board. One-shot mode
 * exits 3 after printing it. `--board ID` names the board the cursor came
 * from, so a FRESH waiter (a re-arm) catches a replacement too; without it
 * the board is learned on first contact.
 */
async function cmdWait(a: Args): Promise<void> {
  const port = Number(flag(a, "port") ?? defaultPort());
  const base = flag(a, "url") ?? `http://127.0.0.1:${port}`;
  const ignore = a.flags.get("ignore-actor") ?? [];
  const follow = a.flags.has("follow");
  const timeoutS = flag(a, "timeout") === undefined ? undefined : Number(flag(a, "timeout"));
  const deadline = timeoutS === undefined ? Infinity : Date.now() + timeoutS * 1000;
  const sinceArg = flag(a, "since") ?? "head";
  let cursor: number | undefined = sinceArg === "head" ? undefined : Number(sinceArg);
  if (cursor !== undefined && (!Number.isInteger(cursor) || cursor < 0)) die("--since must be a cursor (integer >= 0) or 'head'");
  let down = false;
  let boardId: string | undefined = flag(a, "board");

  const resync = (error: string, detail: string, head: number, board: string) => {
    out({ error, cursor, head, board, detail: `${detail}; resuming from head ${head} — re-read the board` });
    cursor = head;
    boardId = board;
    if (!follow) process.exit(3);
  };

  for (;;) {
    const remaining = deadline - Date.now();
    if (remaining <= 0) process.exit(0);
    try {
      if (cursor === undefined || boardId === undefined) {
        // First contact: learn which board this cursor counts (and, for
        // --since head, where "now" is).
        const h = (await (await fetch(`${base}/api/health`)).json()) as { head: number; board: string };
        if (cursor === undefined) cursor = h.head;
        if (boardId === undefined) boardId = h.board;
      }
      const q = new URLSearchParams({ since: String(cursor), wait: String(Math.max(1, Math.min(60, Math.ceil(remaining / 1000)))) });
      for (const i of ignore) q.append("ignore_actor", i);
      const resp = await fetch(`${base}/api/events?${q}`);
      if (resp.status === 409) {
        const e = (await resp.json()) as { error: string; message: string; head: number; board: string };
        if (e.error === "cursor_ahead") {
          down = false;
          resync("cursor_ahead", e.message, e.head, e.board);
          continue;
        }
      }
      if (!resp.ok) throw new Error(`HTTP ${resp.status}: ${await resp.text()}`);
      const r = (await resp.json()) as { events: unknown[]; cursor: number; board: string };
      if (down) {
        down = false;
        out({ reconnected: true, cursor });
      }
      if (r.board !== boardId) {
        // Same port, different database: this cursor counted another board's
        // events. Drop the batch rather than deliver it as news.
        const h = (await (await fetch(`${base}/api/health`)).json()) as { head: number; board: string };
        resync("board_replaced", `the daemon now serves board ${h.board}, not ${boardId} (the db was re-created)`, h.head, h.board);
        continue;
      }
      boardId = r.board;
      cursor = r.cursor;
      for (const ev of r.events) out(ev);
      if (r.events.length && !follow) process.exit(0);
    } catch (e) {
      if (!down) {
        down = true;
        out({ error: "daemon_unreachable", url: base, cursor: cursor ?? null, detail: String((e as Error).message ?? e) });
      }
      if (!follow) process.exit(2);
      await sleep(2000);
    }
  }
}

async function cmdHead(a: Args): Promise<void> {
  const port = Number(flag(a, "port") ?? defaultPort());
  const base = flag(a, "url") ?? `http://127.0.0.1:${port}`;
  try {
    const h = (await (await fetch(`${base}/api/health`)).json()) as { head: number; board: string };
    out({ cursor: h.head, board: h.board });
  } catch (e) {
    die(`daemon unreachable at ${base}: ${String((e as Error).message ?? e)}`, 2);
  }
}

function cmdImport(a: Args): void {
  const file = a.positional[0] ?? die("usage: import <state.json | saved board .html>");
  const db = flag(a, "db") ?? defaultDbPath();
  const live = liveDaemon(db);
  if (live) die(`a daemon (pid ${live.pid}) owns ${db}; stop it before importing`);
  const text = readFileSync(file, "utf8");
  const state = /^\s*</.test(text) ? extractStateFromHtml(text) : JSON.parse(text);
  const board = new Board(db);
  try {
    const r = board.importState(state, { actor: flag(a, "actor") ?? "import" });
    out({ imported: r, db });
  } catch (e) {
    die(String((e as Error).message ?? e));
  } finally {
    board.close();
  }
}

function cmdExport(a: Args): void {
  const db = flag(a, "db") ?? defaultDbPath();
  const board = new Board(db, { readonly: true });
  try {
    const text = JSON.stringify(board.exportState(), null, 1);
    const file = flag(a, "out");
    if (file) {
      writeFileSync(file, `${text}\n`);
      process.stderr.write(`dispatch-board: exported ${db} -> ${file}\n`);
    } else process.stdout.write(`${text}\n`);
  } finally {
    board.close();
  }
}

const HELP = `usage: node dist/cli.js <command>

  serve                         run the daemon (MCP at /mcp, web UI at /, event feed)
  wait [--since N|head] [--board ID] [--ignore-actor TAG]... [--follow] [--timeout SECONDS]
                                block for board events; one JSON line per event
                                (--board: the board id the cursor came from)
  head                          print {"cursor", "board"}: where to arm a wait from
  import <state.json|page.html> load a board state into an EMPTY db (daemon stopped)
  export [--out FILE]           write the full board state as JSON

  --db PATH    board database (default: <main checkout>/.dispatch/board.db, or $DISPATCH_BOARD_DB)
  --port N     daemon port (default 7453, or $DISPATCH_BOARD_PORT)
`;

async function main(): Promise<void> {
  const a = parse(process.argv.slice(2));
  switch (a.cmd) {
    case "serve":
      return cmdServe(a);
    case "wait":
      return cmdWait(a);
    case "head":
      return cmdHead(a);
    case "import":
      return cmdImport(a);
    case "export":
      return cmdExport(a);
    case "help":
    case "--help":
    case "-h":
      process.stdout.write(HELP);
      return;
    default:
      die(`unknown command '${a.cmd}'\n\n${HELP}`);
  }
}

main().catch((e) => die(String((e as Error).stack ?? e)));
