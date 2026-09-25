/**
 * The daemon: ONE process owns the board DB and serves, on 127.0.0.1 only,
 *
 *   /mcp          MCP over Streamable HTTP (stateless: a fresh server per
 *                 request, so a daemon restart never strands a client session)
 *   /             the web UI (ui/board.html)
 *   /api/...      the UI's JSON API (every write is actor "board")
 *   /api/events   long-poll event feed — what `cli.js wait` blocks on
 *   /api/stream   SSE nudges for the UI's live refresh
 *
 * One process is the point: no cross-process write races, one source of truth.
 */
import { createServer, IncomingMessage, Server, ServerResponse } from "node:http";
import { readFileSync, unlinkSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import { Board, BoardError, BoardEvent, SUMMARY_FIELDS } from "./board.js";
import { buildMcpServer } from "./mcp.js";
import { liveDaemon, lockPath } from "./paths.js";

/** Actor tag for every write made through the web UI — a user gesture. */
export const UI_ACTOR = "board";

const UI_FIELDS = [...SUMMARY_FIELDS, "question", "created"];

function uiHtmlPath(): string {
  // dist/server.js -> ../ui/board.html
  return join(dirname(fileURLToPath(import.meta.url)), "..", "ui", "board.html");
}

class HttpError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

function send(res: ServerResponse, status: number, body: unknown, type = "application/json; charset=utf-8"): void {
  const text = typeof body === "string" ? body : JSON.stringify(body);
  res.writeHead(status, { "content-type": type, "cache-control": "no-store" });
  res.end(text);
}

async function readJson(req: IncomingMessage, limit = 4 * 1024 * 1024): Promise<unknown> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const c of req) {
    size += (c as Buffer).length;
    if (size > limit) throw new HttpError(413, "request body too large");
    chunks.push(c as Buffer);
  }
  if (!size) return undefined;
  try {
    return JSON.parse(Buffer.concat(chunks).toString("utf8"));
  } catch {
    throw new HttpError(400, "body is not valid JSON");
  }
}

function boardErrorStatus(e: BoardError): number {
  switch (e.code) {
    case "not_found":
      return 404;
    case "stale_version":
    case "claim_refused":
    case "not_empty":
      return 409;
    default:
      return 422;
  }
}

export interface DaemonOptions {
  dbPath: string;
  port: number;
  host?: string;
  log?: (msg: string) => void;
}

export interface Daemon {
  server: Server;
  board: Board;
  port: number;
  close(): Promise<void>;
}

/**
 * Block until an event after `since` that no ignored actor wrote, or until
 * `timeoutMs`. Always returns the cursor to resume from.
 */
export function waitForEvents(
  board: Board,
  since: number,
  ignore: string[],
  timeoutMs: number,
  signal?: AbortSignal,
): Promise<{ events: BoardEvent[]; cursor: number }> {
  const first = board.eventsSince(since, { ignore_actors: ignore });
  if (first.events.length || timeoutMs <= 0) return Promise.resolve(first);
  return new Promise((resolve) => {
    let cursor = first.cursor;
    const done = () => {
      clearTimeout(timer);
      board.off("event", onEvent);
      signal?.removeEventListener("abort", onAbort);
    };
    const onEvent = () => {
      const r = board.eventsSince(cursor, { ignore_actors: ignore });
      cursor = r.cursor;
      if (r.events.length) {
        done();
        resolve(r);
      }
    };
    const onAbort = () => {
      done();
      resolve({ events: [], cursor });
    };
    const timer = setTimeout(onAbort, timeoutMs);
    board.on("event", onEvent);
    signal?.addEventListener("abort", onAbort);
  });
}

export async function startDaemon(opts: DaemonOptions): Promise<Daemon> {
  const log = opts.log ?? ((m: string) => process.stderr.write(`[dispatch-board] ${m}\n`));
  const other = liveDaemon(opts.dbPath);
  if (other && other.pid !== process.pid) {
    throw new Error(`another daemon (pid ${other.pid}, port ${other.port}) already owns ${opts.dbPath}`);
  }
  const board = new Board(opts.dbPath);
  board.setMaxListeners(0);
  const host = opts.host ?? "127.0.0.1";
  let port = opts.port;
  const allowedHosts = () => [`127.0.0.1:${port}`, `localhost:${port}`];

  const handle = async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
    // DNS-rebinding and cross-site guard: only our own origin may talk to us.
    if (!allowedHosts().includes(String(req.headers.host))) throw new HttpError(403, "bad Host header");
    const origin = req.headers.origin;
    if (origin && !allowedHosts().some((h) => origin === `http://${h}`)) throw new HttpError(403, "cross-origin request refused");

    const url = new URL(req.url ?? "/", `http://${req.headers.host}`);
    const path = url.pathname;

    if (path === "/mcp") {
      const body = req.method === "POST" ? await readJson(req) : undefined;
      const server = buildMcpServer(board);
      const transport = new StreamableHTTPServerTransport({
        sessionIdGenerator: undefined,
        enableJsonResponse: true,
        enableDnsRebindingProtection: true,
        allowedHosts: allowedHosts(),
      });
      res.on("close", () => {
        void transport.close();
        void server.close();
      });
      await server.connect(transport);
      await transport.handleRequest(req, res, body);
      return;
    }

    if (req.method === "GET" && (path === "/" || path === "/index.html")) {
      send(res, 200, readFileSync(uiHtmlPath(), "utf8"), "text/html; charset=utf-8");
      return;
    }

    if (req.method === "GET" && path === "/api/health") {
      send(res, 200, { ok: true, pid: process.pid, db: opts.dbPath, head: board.head() });
      return;
    }

    if (req.method === "GET" && path === "/api/board") {
      send(res, 200, { head: board.head(), cards: board.listCards({ include_archived: true, fields: UI_FIELDS }) });
      return;
    }

    if (req.method === "GET" && path === "/api/events") {
      const since = Number(url.searchParams.get("since") ?? "0");
      if (!Number.isInteger(since) || since < 0) throw new HttpError(400, "since must be a non-negative integer");
      const waitS = Math.min(Math.max(Number(url.searchParams.get("wait") ?? "0") || 0, 0), 300);
      const ignore = url.searchParams.getAll("ignore_actor");
      const ac = new AbortController();
      res.on("close", () => ac.abort());
      const r = await waitForEvents(board, since, ignore, waitS * 1000, ac.signal);
      if (!res.writableEnded && !res.destroyed) send(res, 200, r);
      return;
    }

    if (req.method === "GET" && path === "/api/stream") {
      res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-store", connection: "keep-alive" });
      res.write(`data: ${JSON.stringify({ head: board.head() })}\n\n`);
      const onEvent = (ev: BoardEvent) => res.write(`data: ${JSON.stringify({ head: ev.cursor, kind: ev.kind, card: ev.card })}\n\n`);
      const ping = setInterval(() => res.write(": ping\n\n"), 25000);
      board.on("event", onEvent);
      res.on("close", () => {
        clearInterval(ping);
        board.off("event", onEvent);
      });
      return;
    }

    const cardMatch = /^\/api\/cards\/([^/]+)(?:\/(move|update|answer|delete))?$/.exec(path);
    if (req.method === "GET" && cardMatch && !cardMatch[2]) {
      send(res, 200, board.getCard(decodeURIComponent(cardMatch[1])));
      return;
    }

    if (req.method === "POST" && path.startsWith("/api/")) {
      // A JSON content type forces a CORS preflight on any cross-site form
      // post, which this server never answers — a second CSRF guard.
      if (!String(req.headers["content-type"] ?? "").startsWith("application/json")) {
        throw new HttpError(415, "POST bodies must be application/json");
      }
      const b = ((await readJson(req)) ?? {}) as Record<string, any>;
      const meta = { actor: UI_ACTOR, by: UI_ACTOR };
      if (path === "/api/cards") {
        send(res, 200, board.createCard(b as any, meta));
        return;
      }
      if (cardMatch && cardMatch[2]) {
        const id = decodeURIComponent(cardMatch[1]);
        switch (cardMatch[2]) {
          case "move":
            send(res, 200, board.moveCard(id, b.column, b.expected_version, { ...meta, patch: b.patch, activity: b.activity }));
            return;
          case "update":
            send(res, 200, board.updateCard(id, b.patch ?? {}, b.expected_version, { ...meta, activity: b.activity }));
            return;
          case "answer":
            send(res, 200, board.answerQuestion(id, b.answer, b.expected_version, meta));
            return;
          case "delete":
            board.deleteCard(id, b.expected_version, meta);
            send(res, 200, { ok: true });
            return;
        }
      }
    }

    throw new HttpError(404, "not found");
  };

  const server = createServer((req, res) => {
    handle(req, res).catch((e) => {
      if (res.headersSent) {
        res.end();
        return;
      }
      if (e instanceof BoardError) send(res, boardErrorStatus(e), e.toJSON());
      else if (e instanceof HttpError) send(res, e.status, { error: "http", message: e.message });
      else {
        log(`internal error: ${(e as Error).stack ?? String(e)}`);
        send(res, 500, { error: "internal", message: String((e as Error).message ?? e) });
      }
    });
  });

  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(opts.port, host, () => {
      server.off("error", reject);
      resolve();
    });
  });
  port = (server.address() as { port: number }).port;
  writeFileSync(lockPath(opts.dbPath), JSON.stringify({ pid: process.pid, port, started: new Date().toISOString() }));
  log(`serving ${opts.dbPath} on http://${host}:${port}  (MCP: /mcp, UI: /)`);

  let closed = false;
  return {
    server,
    board,
    port,
    async close() {
      if (closed) return;
      closed = true;
      server.closeAllConnections();
      await new Promise<void>((r) => server.close(() => r()));
      try {
        const l = liveDaemon(opts.dbPath);
        if (l && l.pid === process.pid && existsSync(lockPath(opts.dbPath))) unlinkSync(lockPath(opts.dbPath));
      } catch {
        /* lock already gone */
      }
      board.close();
    },
  };
}
