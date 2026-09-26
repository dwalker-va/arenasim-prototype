/**
 * The board: every read and write of Dispatch state, over one SQLite file.
 *
 * Two guarantees are the reason this module exists, and both are enforced
 * here rather than by caller convention:
 *
 *  - OPTIMISTIC CONCURRENCY. Every card carries a `version`. A write that
 *    names a stale `expected_version` is refused with the current card; it
 *    never overwrites (no last-writer-wins).
 *  - COMPARE-AND-SET CLAIMS. `claimCard` takes the claim only if the card is
 *    claimable at the instant of the write (`agent == null`, or on a review
 *    card `agent.status == "done"`). A second claim on the same card fails.
 *
 * Every write runs in one IMMEDIATE transaction (the write lock is held from
 * the read that validates to the row update), and the row update itself is
 * conditioned on the version that was read — so the guards hold even for two
 * processes on one file, not only inside the single daemon.
 *
 * Storage shape: a card's fields live as one JSON document (`doc`), with
 * `column` and `role` exposed as generated, indexed columns. That keeps the
 * existing card schema's field meanings — and the legacy shapes the real board
 * carries (agent strings, a missing priority) — round-trippable exactly,
 * while the protocol's queries stay SQL. Activity is a separate, append-only
 * table (triggers refuse UPDATE and DELETE). Events are a third table with a
 * monotonic cursor, the feed `wait` and the web UI follow.
 */
import Database from "better-sqlite3";
import { EventEmitter } from "node:events";
import { randomUUID } from "node:crypto";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";

export const COLUMNS = [
  "backlog",
  "needs_input",
  "in_progress",
  "review",
  "human_review",
  "done",
  "archived",
] as const;
export type Column = (typeof COLUMNS)[number];
export const ROLES = ["engineer", "tester", "release-manager", "pm"] as const;
export type Role = (typeof ROLES)[number];
/** Columns a non-pm card may not enter without its own PR, `pr` (the AS-4 gate). */
export const GATED_COLUMNS: readonly string[] = ["review", "human_review"];

/** A reference: a prerequisite PR, the PR where a finding was made, a workshop page. */
export interface Link {
  label: string;
  url: string;
}
/** The card's OWN implementing pull request — never one of its references. */
export interface Pr {
  number: number;
  url: string;
}
/** A pull request URL; the number is read from it. */
const PR_URL = /^https?:\/\/[^\s/]+\/\S*\/pull\/(\d+)\/?$/;
export interface ActivityEntry {
  t: string;
  by: string;
  msg: string;
}
export interface BoardEvent {
  cursor: number;
  t: string;
  actor: string;
  kind: string;
  card: string | null;
  data: Record<string, unknown>;
}
/** A card as the protocol sees it. Unknown legacy fields ride along. */
export type Card = Record<string, unknown> & {
  id: string;
  title: string;
  column: Column;
  activity?: ActivityEntry[];
};

export type ErrorCode =
  | "not_found"
  | "stale_version"
  | "claim_refused"
  | "gate_refused"
  | "invalid"
  | "not_empty"
  | "cursor_ahead";

export class BoardError extends Error {
  constructor(
    public code: ErrorCode,
    message: string,
    /** The card as it is NOW, for refusals a caller must re-read to recover from. */
    public card?: Card,
  ) {
    super(message);
    this.name = "BoardError";
  }
  toJSON() {
    return { error: this.code, message: this.message, card: this.card };
  }
}

/** The fields `list_cards` returns unless the caller asks for others. */
export const SUMMARY_FIELDS = [
  "id",
  "title",
  "column",
  "role",
  "priority",
  "agent",
  "pr",
  "worktree",
  "links",
  "updated",
  "released",
] as const;

/** Patchable through update_card / move_card. `column` is move_card's alone. */
const PATCHABLE = new Set([
  "title",
  "body",
  "role",
  "priority",
  "links",
  "pr",
  "worktree",
  "question",
  "agent",
  "released",
]);

/** Pull the `<script id="state">` JSON out of a saved artifact board page. */
export function extractStateFromHtml(html: string): unknown {
  const m = /<script id="state" type="application\/json">([\s\S]*?)<\/script>/.exec(html);
  if (!m) throw new Error('no <script id="state" type="application/json"> block found');
  return JSON.parse(m[1]);
}

/** Board timestamps are UTC to the second with no zone suffix, as the artifact wrote them. */
export function now(): string {
  return new Date().toISOString().slice(0, 19);
}

const SCHEMA = `
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS cards (
  id         TEXT NOT NULL UNIQUE,
  version    INTEGER NOT NULL DEFAULT 1,
  doc        TEXT NOT NULL CHECK (json_valid(doc)),
  deleted_at TEXT,
  col  TEXT GENERATED ALWAYS AS (json_extract(doc, '$.column')) VIRTUAL,
  role TEXT GENERATED ALWAYS AS (json_extract(doc, '$.role')) VIRTUAL
);
CREATE INDEX IF NOT EXISTS cards_col ON cards(col);
CREATE TABLE IF NOT EXISTS activity (
  card_id TEXT NOT NULL REFERENCES cards(id),
  seq     INTEGER NOT NULL,
  t       TEXT NOT NULL,
  by      TEXT NOT NULL,
  msg     TEXT NOT NULL,
  PRIMARY KEY (card_id, seq)
);
CREATE TRIGGER IF NOT EXISTS activity_no_update BEFORE UPDATE ON activity
  BEGIN SELECT RAISE(ABORT, 'activity is append-only'); END;
CREATE TRIGGER IF NOT EXISTS activity_no_delete BEFORE DELETE ON activity
  BEGIN SELECT RAISE(ABORT, 'activity is append-only'); END;
CREATE TABLE IF NOT EXISTS events (
  cursor  INTEGER PRIMARY KEY AUTOINCREMENT,
  t       TEXT NOT NULL,
  actor   TEXT NOT NULL,
  kind    TEXT NOT NULL,
  card_id TEXT,
  data    TEXT NOT NULL DEFAULT '{}'
);
`;

interface CardRow {
  id: string;
  version: number;
  doc: string;
}

// ---- value validation (writes; import is deliberately more permissive) ----

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function invalid(msg: string): never {
  throw new BoardError("invalid", msg);
}

function checkActor(actor: unknown): string {
  if (typeof actor !== "string" || !actor.trim()) invalid("actor is required: a non-empty session/actor tag");
  return actor;
}

/** Every versioned write names the version it read; a missing one would make the write unconditional. */
export function checkVersion(v: unknown): number {
  if (!Number.isInteger(v)) invalid("expected_version is required: the integer version of the card as you read it");
  return v as number;
}

function checkColumn(c: unknown): Column {
  if (!(COLUMNS as readonly unknown[]).includes(c)) invalid(`column must be one of ${COLUMNS.join(", ")}`);
  return c as Column;
}

function checkPr(v: unknown): Pr | null {
  if (v === null) return null;
  const url = isObj(v) && typeof v.url === "string" ? v.url.trim() : "";
  const m = PR_URL.exec(url);
  if (!m) invalid("pr must be null or {url: <a pull-request URL, .../pull/<n>>, number?}");
  const number = Number(m[1]);
  if ((v as Record<string, unknown>).number !== undefined && (v as Record<string, unknown>).number !== number) {
    invalid(`pr.number ${String((v as Record<string, unknown>).number)} does not match its url (#${number})`);
  }
  return { number, url };
}

/** A worktree is recorded, never checked: any absolute, single-line path. */
function checkWorktree(v: unknown): string | null {
  if (v === null) return null;
  if (typeof v !== "string" || !v.startsWith("/") || /[\r\n]/.test(v)) invalid("worktree must be null or an absolute path");
  return v as string;
}

/** Validate a patch; returns it with `pr` normalised to {number, url}. */
function checkPatch(patch: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = { ...patch };
  for (const [k, v] of Object.entries(patch)) {
    if (k === "column") invalid("column changes go through move_card, which enforces the column rules");
    if (!PATCHABLE.has(k)) invalid(`field '${k}' is not patchable (patchable: ${[...PATCHABLE].join(", ")})`);
    switch (k) {
      case "title":
        if (typeof v !== "string" || !v.trim()) invalid("title must be a non-empty string");
        break;
      case "body":
        if (typeof v !== "string") invalid("body must be a string");
        break;
      case "role":
        if (!(ROLES as readonly unknown[]).includes(v)) invalid(`role must be one of ${ROLES.join(", ")}`);
        break;
      case "priority":
        if (typeof v !== "string" || !/^P[1-4]$/.test(v)) invalid("priority must be P1..P4");
        break;
      case "links":
        if (!Array.isArray(v) || !v.every((l) => isObj(l) && typeof l.label === "string" && typeof l.url === "string"))
          invalid("links must be an array of {label, url}");
        break;
      case "question":
        if (v !== null && !(isObj(v) && typeof v.text === "string" && (v.answer === undefined || typeof v.answer === "string")))
          invalid("question must be null or {text, answer?}");
        break;
      case "agent":
        // A working claim is made ONLY by claim_card, which applies the claim
        // rule; a patch may clear a claim or close one out, never take one.
        if (v !== null && !(isObj(v) && v.status === "done")) {
          invalid(
            isObj(v) && v.status === "working"
              ? "agent.status 'working' is set only by claim_card, which enforces the claim rule"
              : "agent in a patch must be null or {status: done, started?, finished?, name?}",
          );
        }
        break;
      case "released":
        if (typeof v !== "string" || !v) invalid("released must be a non-empty tag string");
        break;
      case "pr":
        out.pr = checkPr(v);
        break;
      case "worktree":
        checkWorktree(v);
        break;
    }
  }
  return out;
}

function hasPr(doc: Record<string, unknown>): boolean {
  return isObj(doc.pr) && typeof doc.pr.url === "string" && PR_URL.test(doc.pr.url);
}

/**
 * Migration: each card's own PR, derived from its hand-off record — never
 * from `links`, which are references. The hand-off entries are the Engineer's
 * own report (`by: "engineer"`, message starting READY — "READY_FOR_REVIEW —
 * PR #N ...") and the orchestrator's record of it (`by: "orchestrator"`,
 * starting "ENGINEER DONE" or "READY FOR REVIEW"). The PR is the first
 * "PR #N" in each such entry. Exactly one distinct N across a card's
 * hand-offs gives `pr`; none gives null; several give null and are reported
 * as ambiguous. The url is the card's own link to /pull/N when it has one,
 * else built from the board's single repository base (reported as
 * constructed; with no single base it is left null and reported ambiguous).
 */
export function derivePr(cards: Record<string, unknown>[]): {
  pr: Record<string, Pr | null>;
  ambiguous: { id: string; candidates: number[] }[];
  constructed: string[];
} {
  const isHandoff = (a: ActivityEntry) =>
    (a.by === "engineer" && /^READY/.test(a.msg)) || (a.by === "orchestrator" && /^(ENGINEER DONE|READY FOR REVIEW)/.test(a.msg));
  const bases = new Set<string>();
  for (const c of cards) {
    for (const l of Array.isArray(c.links) ? c.links : []) {
      const m = isObj(l) && typeof l.url === "string" ? /^(https?:\/\/\S+?)\/pull\/\d+\/?$/.exec(l.url) : null;
      if (m) bases.add(m[1]);
    }
  }
  const base = bases.size === 1 ? [...bases][0] : null;
  const out = { pr: {} as Record<string, Pr | null>, ambiguous: [] as { id: string; candidates: number[] }[], constructed: [] as string[] };
  for (const c of cards) {
    const id = String(c.id);
    const nums = new Set<number>();
    for (const a of (Array.isArray(c.activity) ? c.activity : []) as ActivityEntry[]) {
      if (!isHandoff(a)) continue;
      const m = /PR #(\d+)/.exec(a.msg);
      if (m) nums.add(Number(m[1]));
    }
    if (nums.size !== 1) {
      out.pr[id] = null;
      if (nums.size > 1) out.ambiguous.push({ id, candidates: [...nums].sort((x, y) => x - y) });
      continue;
    }
    const n = [...nums][0];
    const own = (Array.isArray(c.links) ? c.links : []).find(
      (l) => isObj(l) && typeof l.url === "string" && new RegExp(`/pull/${n}/?$`).test(l.url) && PR_URL.test(l.url),
    ) as Link | undefined;
    if (own) out.pr[id] = { number: n, url: own.url };
    else if (base) {
      out.pr[id] = { number: n, url: `${base}/pull/${n}` };
      out.constructed.push(id);
    } else {
      out.pr[id] = null;
      out.ambiguous.push({ id, candidates: [n] });
    }
  }
  return out;
}

/**
 * A patched agent {status: done} closes out a working claim; it may not
 * invent one. Checked against the STORED agent, inside the write.
 */
function checkAgentTransition(id: string, stored: unknown, patch: Record<string, unknown>, current: () => Card): void {
  if (isObj(patch.agent) && !(isObj(stored) && stored.status === "working")) {
    throw new BoardError("claim_refused", `${id} has no working claim to mark done`, current());
  }
}

/** Who holds a claim, for its activity line (legacy boards stored a bare string). */
function claimant(a: unknown): string {
  return isObj(a) && typeof a.name === "string" ? a.name : typeof a === "string" && a ? a : "unnamed";
}

/** The one wording every path uses when a claim ends; it always names the claimant. */
function claimLine(how: "released" | "finished", agent: unknown): string {
  return how === "released" ? `Claim released (was ${claimant(agent)})` : `Claim finished (${claimant(agent)})`;
}

/**
 * The activity line for a patch that changes `agent`, or null when it does
 * not. A claim never ends silently, whichever tool ended it.
 */
function claimChangeNote(stored: unknown, patch: Record<string, unknown>): string | null {
  if (!("agent" in patch) || JSON.stringify(patch.agent) === JSON.stringify(stored ?? null)) return null;
  if (patch.agent === null) return stored == null ? null : claimLine("released", stored);
  return claimLine("finished", stored);
}

function checkAppend(a: unknown): { heading: string; text: string } | undefined {
  if (a === undefined) return undefined;
  if (!isObj(a) || typeof a.heading !== "string" || !a.heading.trim() || typeof a.text !== "string" || !a.text.trim()) {
    invalid("append must be {heading, text}, both non-empty");
  }
  return { heading: a.heading as string, text: a.text as string };
}

/** The body with `text` appended under a `## heading` section. */
function withSection(body: unknown, heading: string, text: string): string {
  const b = typeof body === "string" ? body.replace(/\s+$/, "") : "";
  const section = `## ${heading.replace(/^#+\s*/, "")}\n\n${text}`;
  return b ? `${b}\n\n${section}` : section;
}

/** The AS-4 PR gate: a non-pm card enters review/human_review only with its own PR. */
function gateAllows(doc: Record<string, unknown>, to: string): boolean {
  return !GATED_COLUMNS.includes(to) || doc.role === "pm" || hasPr(doc);
}

/**
 * Why a card cannot be claimed right now, or null when it can. The claim
 * rule is the protocol's dedup guard: in_progress needs `agent == null`;
 * review needs `agent == null` or `agent.status == "done"`.
 */
export function claimRefusal(doc: Record<string, unknown>): string | null {
  if (doc.role === "pm") return "pm cards are never claimed: a PM session is interactive and opened by the user";
  const agent = doc.agent ?? null;
  if (doc.column === "in_progress") {
    return agent === null ? null : "already claimed (agent is not null)";
  }
  if (doc.column === "review") {
    if (agent === null) return null;
    if (isObj(agent) && agent.status === "done") return null;
    return "already claimed (review card's agent is working)";
  }
  return `only in_progress and review cards are claimable (card is in ${String(doc.column)})`;
}

export interface ListOptions {
  column?: string | string[];
  role?: string;
  /** Summary fields to return; `["*"]` returns every field except activity. */
  fields?: string[];
  include_archived?: boolean;
}

export interface WriteMeta {
  actor: string;
  /** Activity `by`; defaults to the actor. */
  by?: string;
}

export class Board extends EventEmitter {
  readonly db: Database.Database;

  constructor(readonly path: string, opts: { readonly?: boolean } = {}) {
    super();
    if (!opts.readonly && path !== ":memory:") mkdirSync(dirname(path), { recursive: true });
    this.db = new Database(path, { readonly: !!opts.readonly, fileMustExist: !!opts.readonly });
    this.db.pragma("busy_timeout = 10000");
    if (!opts.readonly) {
      this.db.pragma("journal_mode = WAL");
      this.db.pragma("foreign_keys = ON");
      this.db.exec(SCHEMA);
      this.db
        .prepare("INSERT OR IGNORE INTO meta(key, value) VALUES ('schema', '1'), ('next_id', '1'), ('id_prefix', 'AS-'), ('board_id', ?)")
        .run(randomUUID());
    }
  }

  close(): void {
    this.db.close();
  }

  // ---------------------------------------------------------------- internals

  private meta(key: string): string {
    const r = this.db.prepare("SELECT value FROM meta WHERE key = ?").get(key) as { value: string } | undefined;
    if (!r) throw new Error(`meta key ${key} missing`);
    return r.value;
  }

  private row(id: string): CardRow {
    const r = this.db.prepare("SELECT id, version, doc FROM cards WHERE id = ? AND deleted_at IS NULL").get(id) as
      | CardRow
      | undefined;
    if (!r) throw new BoardError("not_found", `no card ${id}`);
    return r;
  }

  private activity(id: string, limit?: number): ActivityEntry[] {
    if (limit !== undefined && limit >= 0) {
      const rows = this.db
        .prepare("SELECT t, by, msg FROM activity WHERE card_id = ? ORDER BY seq DESC LIMIT ?")
        .all(id, limit) as ActivityEntry[];
      return rows.reverse();
    }
    return this.db.prepare("SELECT t, by, msg FROM activity WHERE card_id = ? ORDER BY seq").all(id) as ActivityEntry[];
  }

  private activityCount(id: string): number {
    return (this.db.prepare("SELECT count(*) AS n FROM activity WHERE card_id = ?").get(id) as { n: number }).n;
  }

  /** Materialise a card: the doc with its activity slot filled, plus version. */
  private materialise(r: CardRow, activityLimit?: number): Card {
    const doc = JSON.parse(r.doc) as Card;
    if ("activity" in doc) doc.activity = this.activity(r.id, activityLimit);
    return Object.assign(doc, { version: r.version });
  }

  /** Current card for a refusal: fields + the last few activity entries. */
  private current(id: string): Card {
    const r = this.row(id);
    const c = this.materialise(r, 5);
    c.activity_total = this.activityCount(id);
    return c;
  }

  private appendActivityRow(id: string, e: ActivityEntry): void {
    const next = (this.db.prepare("SELECT coalesce(max(seq), 0) + 1 AS s FROM activity WHERE card_id = ?").get(id) as {
      s: number;
    }).s;
    this.db.prepare("INSERT INTO activity(card_id, seq, t, by, msg) VALUES (?, ?, ?, ?, ?)").run(id, next, e.t, e.by, e.msg);
  }

  private pending: BoardEvent[] = [];

  private recordEvent(actor: string, kind: string, card: string | null, data: Record<string, unknown>): void {
    const t = now();
    const info = this.db
      .prepare("INSERT INTO events(t, actor, kind, card_id, data) VALUES (?, ?, ?, ?, ?)")
      .run(t, actor, kind, card, JSON.stringify(data));
    this.pending.push({ cursor: Number(info.lastInsertRowid), t, actor, kind, card, data });
  }

  /**
   * Run a write. IMMEDIATE takes the write lock before the first read, so the
   * read-validate-write sequence is atomic against every other connection.
   * Events are emitted only after COMMIT, so no listener sees a rolled-back write.
   */
  private write<T>(fn: () => T): T {
    this.pending = [];
    let out: T;
    try {
      out = this.db.transaction(fn).immediate();
    } catch (e) {
      this.pending = [];
      throw e;
    }
    const evs = this.pending;
    this.pending = [];
    for (const ev of evs) this.emit("event", ev);
    return out;
  }

  /**
   * Replace a card's doc, conditioned on the version the caller validated
   * against. Zero rows changed means someone else won — refuse.
   */
  private commitDoc(id: string, fromVersion: number, doc: Record<string, unknown>): number {
    doc.updated = now();
    const info = this.db
      .prepare("UPDATE cards SET doc = ?, version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NULL")
      .run(JSON.stringify(doc), id, fromVersion);
    if (info.changes !== 1) {
      throw new BoardError("stale_version", `${id} changed underneath this write; re-read it`, this.current(id));
    }
    return fromVersion + 1;
  }

  /** Load for a versioned write, refusing a stale expected_version up front. */
  private loadExpecting(id: string, expected: number | undefined): { r: CardRow; doc: Record<string, unknown> } {
    const r = this.row(id);
    if (expected !== undefined) {
      if (!Number.isInteger(expected)) invalid("expected_version must be an integer");
      if (r.version !== expected) {
        throw new BoardError(
          "stale_version",
          `${id} is at version ${r.version}, not ${expected}: it changed since you read it. Re-read and re-apply.`,
          this.current(id),
        );
      }
    }
    return { r, doc: JSON.parse(r.doc) };
  }

  private summary(r: CardRow, fields?: string[]): Record<string, unknown> {
    const doc = JSON.parse(r.doc) as Record<string, unknown>;
    const out: Record<string, unknown> = {};
    if (fields && fields.includes("*")) {
      for (const [k, v] of Object.entries(doc)) if (k !== "activity") out[k] = v;
    } else {
      for (const f of fields ?? SUMMARY_FIELDS) {
        if (f === "activity") continue;
        if (f in doc) out[f] = doc[f];
      }
      out.id = doc.id;
    }
    out.version = r.version;
    return out;
  }

  // ---------------------------------------------------------------- reads

  listCards(opts: ListOptions = {}): Record<string, unknown>[] {
    const where = ["deleted_at IS NULL"];
    const args: unknown[] = [];
    const cols = opts.column === undefined ? undefined : Array.isArray(opts.column) ? opts.column : [opts.column];
    if (cols) {
      cols.forEach(checkColumn);
      where.push(`col IN (${cols.map(() => "?").join(", ")})`);
      args.push(...cols);
    } else if (!opts.include_archived) {
      where.push("col <> 'archived'");
    }
    if (opts.role) {
      where.push("role = ?");
      args.push(opts.role);
    }
    const rows = this.db
      .prepare(`SELECT id, version, doc FROM cards WHERE ${where.join(" AND ")} ORDER BY rowid`)
      .all(...args) as CardRow[];
    return rows.map((r) => this.summary(r, opts.fields));
  }

  getCard(id: string, opts: { activity_limit?: number } = {}): Card {
    const r = this.row(id);
    const c = this.materialise(r, opts.activity_limit);
    c.activity_total = this.activityCount(id);
    return c;
  }

  /**
   * This database's identity, minted when it was created. A re-created board
   * (export -> fresh db -> import) restarts its event cursors, so a waiter
   * compares this to know its cursor still means anything.
   */
  boardId(): string {
    return this.meta("board_id");
  }

  /** The latest event cursor (0 on a board with no events). */
  head(): number {
    return (this.db.prepare("SELECT coalesce(max(cursor), 0) AS c FROM events").get() as { c: number }).c;
  }

  /**
   * Events after `cursor`, minus those written by `ignoreActors`. The returned
   * `cursor` is the last event SCANNED — past any filtered-out ones — so a
   * waiter that ignores its own writes never re-reads them.
   */
  eventsSince(
    cursor: number,
    opts: { ignore_actors?: string[]; limit?: number } = {},
  ): { events: BoardEvent[]; cursor: number } {
    const limit = Math.max(1, Math.min(opts.limit ?? 200, 1000));
    const head = this.head();
    if (cursor > head) {
      // Waiting here would be silent forever: the cursor belongs to some other
      // (re-created) board. Refuse, so the caller says so and resumes from head.
      throw new BoardError("cursor_ahead", `cursor ${cursor} is past this board's newest event (${head}); the database was re-created — re-read the board and resume from head`);
    }
    const rows = this.db
      .prepare("SELECT cursor, t, actor, kind, card_id AS card, data FROM events WHERE cursor > ? ORDER BY cursor LIMIT ?")
      .all(cursor, limit) as (Omit<BoardEvent, "data"> & { data: string })[];
    const ignore = new Set(opts.ignore_actors ?? []);
    const events = rows
      .filter((r) => !ignore.has(r.actor))
      .map((r) => ({ ...r, data: JSON.parse(r.data) as Record<string, unknown> }));
    return { events, cursor: rows.length ? rows[rows.length - 1].cursor : cursor };
  }

  // ---------------------------------------------------------------- writes

  createCard(
    input: {
      title: string;
      body?: string;
      role: string;
      priority?: string;
      column?: string;
      links?: Link[];
      pr?: unknown;
      worktree?: string | null;
    },
    meta: WriteMeta,
  ): Card {
    const actor = checkActor(meta.actor);
    const column = checkColumn(input.column ?? "backlog");
    const checked = checkPatch({
      title: input.title,
      body: input.body ?? "",
      role: input.role,
      priority: input.priority ?? "P2",
      links: input.links ?? [],
      pr: input.pr ?? null,
      worktree: input.worktree ?? null,
    });
    return this.write(() => {
      const prefix = this.meta("id_prefix");
      let n = Number(this.meta("next_id"));
      while (this.db.prepare("SELECT 1 FROM cards WHERE id = ?").get(`${prefix}${n}`)) n++;
      const id = `${prefix}${n}`;
      const t = now();
      const doc: Record<string, unknown> = {
        id,
        title: input.title,
        body: input.body ?? "",
        column,
        role: input.role,
        priority: input.priority ?? "P2",
        links: input.links ?? [],
        pr: checked.pr,
        worktree: checked.worktree,
        question: null,
        agent: null,
        activity: null,
        created: t,
        updated: t,
      };
      if (!gateAllows(doc, column)) {
        throw new BoardError("gate_refused", `a non-pm card needs its own PR (pr) to enter ${column}`);
      }
      this.db.prepare("INSERT INTO cards(id, doc) VALUES (?, ?)").run(id, JSON.stringify(doc));
      this.db.prepare("UPDATE meta SET value = ? WHERE key = 'next_id'").run(String(n + 1));
      this.appendActivityRow(id, { t, by: meta.by ?? actor, msg: "Created" });
      this.recordEvent(actor, "created", id, { column, title: input.title });
      return this.getCard(id);
    });
  }

  updateCard(
    id: string,
    patch: Record<string, unknown>,
    expectedVersion: number,
    meta: WriteMeta & { activity?: string },
  ): Card {
    const actor = checkActor(meta.actor);
    if (!isObj(patch)) invalid("patch must be an object");
    patch = checkPatch(patch);
    if (!Object.keys(patch).length && !meta.activity) invalid("empty patch");
    checkVersion(expectedVersion);
    return this.write(() => {
      const { r, doc } = this.loadExpecting(id, expectedVersion);
      checkAgentTransition(id, doc.agent, patch, () => this.current(id));
      const claimNote = claimChangeNote(doc.agent, patch);
      Object.assign(doc, patch);
      if (GATED_COLUMNS.includes(doc.column as string) && !gateAllows(doc, doc.column as string)) {
        throw new BoardError("gate_refused", `${id} is in ${String(doc.column)}: a non-pm card there must keep its own PR (pr)`, this.current(id));
      }
      this.commitDoc(id, r.version, doc);
      if (meta.activity) this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg: meta.activity });
      if (claimNote) this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg: claimNote });
      this.recordEvent(actor, "edited", id, { fields: Object.keys(patch) });
      return this.getCard(id, { activity_limit: 5 });
    });
  }

  moveCard(
    id: string,
    column: string,
    expectedVersion: number,
    meta: WriteMeta & { activity?: string; patch?: Record<string, unknown>; append?: { heading: string; text: string } },
  ): Card {
    const actor = checkActor(meta.actor);
    const to = checkColumn(column);
    if (!isObj(meta.patch ?? {})) invalid("patch must be an object");
    const patch = checkPatch(meta.patch ?? {});
    const append = checkAppend(meta.append);
    if (to === "in_progress" && patch.agent !== undefined && patch.agent !== null) {
      invalid("entering in_progress clears the claim (agent: null); claim it with claim_card afterwards");
    }
    checkVersion(expectedVersion);
    return this.write(() => {
      const { r, doc } = this.loadExpecting(id, expectedVersion);
      const from = doc.column as string;
      checkAgentTransition(id, doc.agent, patch, () => this.current(id));
      const stored = doc.agent;
      Object.assign(doc, patch);
      if (append) doc.body = withSection(doc.body, append.heading, append.text);
      if (!gateAllows(doc, to)) {
        throw new BoardError(
          "gate_refused",
          `${id} needs its own PR to enter ${to} (role ${String(doc.role)}; only pm cards are exempt). Set pr: {url} in this move's patch — links are references and do not count.`,
          this.current(id),
        );
      }
      doc.column = to;
      if (to === "in_progress") doc.agent = null;
      const claimNote = claimChangeNote(stored, { agent: doc.agent ?? null });
      this.commitDoc(id, r.version, doc);
      if (append) this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg: `Appended to spec: ${append.heading}` });
      this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg: meta.activity ?? `Moved: ${from} → ${to}` });
      if (claimNote) this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg: claimNote });
      this.recordEvent(actor, "moved", id, {
        from,
        to,
        ...(Object.keys(patch).length ? { fields: Object.keys(patch) } : {}),
        ...(append ? { appended: append.heading } : {}),
      });
      return this.getCard(id, { activity_limit: 5 });
    });
  }

  /**
   * Compare-and-set claim. Succeeds for exactly one caller per claimable
   * state; every other concurrent or later claim is refused with the card.
   */
  claimCard(id: string, name: string, meta: WriteMeta & { activity?: string; worktree?: string }): Card {
    const actor = checkActor(meta.actor);
    if (typeof name !== "string" || !name.trim()) invalid("name (the agent being spawned) is required");
    // The spawned agent's tree, when the caller knows it: recorded, never checked.
    const worktree = meta.worktree === undefined ? undefined : checkWorktree(meta.worktree);
    return this.write(() => {
      const { r, doc } = this.loadExpecting(id, undefined);
      const why = claimRefusal(doc);
      if (why) throw new BoardError("claim_refused", `${id}: ${why}`, this.current(id));
      doc.agent = { status: "working", started: now(), name };
      if (worktree !== undefined) doc.worktree = worktree;
      this.commitDoc(id, r.version, doc);
      this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg: meta.activity ?? `Claimed for ${name}` });
      this.recordEvent(actor, "claimed", id, { name, column: doc.column });
      return this.getCard(id, { activity_limit: 5 });
    });
  }

  private closeClaim(
    kind: "claim_released" | "claim_finished",
    id: string,
    meta: WriteMeta & { name?: string; activity?: string; expected_version?: number },
  ): Card {
    const actor = checkActor(meta.actor);
    return this.write(() => {
      const { r, doc } = this.loadExpecting(id, meta.expected_version);
      const agent = doc.agent;
      if (!isObj(agent) || agent.status !== "working") {
        throw new BoardError("claim_refused", `${id} has no working claim to ${kind === "claim_released" ? "release" : "finish"}`, this.current(id));
      }
      if (meta.name !== undefined && agent.name !== meta.name) {
        throw new BoardError("claim_refused", `${id} is claimed by ${String(agent.name)}, not ${meta.name}`, this.current(id));
      }
      doc.agent = kind === "claim_released" ? null : { ...agent, status: "done", finished: now() };
      this.commitDoc(id, r.version, doc);
      // The claim line always lands; a caller's own note is an additional line.
      if (meta.activity) this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg: meta.activity });
      this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg: claimLine(kind === "claim_released" ? "released" : "finished", agent) });
      this.recordEvent(actor, kind, id, { name: agent.name ?? null });
      return this.getCard(id, { activity_limit: 5 });
    });
  }

  /** working -> null. Startup recovery and the live-claim audit use this. */
  releaseClaim(id: string, meta: WriteMeta & { name?: string; activity?: string; expected_version?: number }): Card {
    return this.closeClaim("claim_released", id, meta);
  }

  /** working -> done (with `finished`). The Engineer/Tester hand-off and the release trigger closeout. */
  finishClaim(id: string, meta: WriteMeta & { name?: string; activity?: string; expected_version?: number }): Card {
    return this.closeClaim("claim_finished", id, meta);
  }

  appendActivity(id: string, msg: string, meta: WriteMeta): Card {
    const actor = checkActor(meta.actor);
    if (typeof msg !== "string" || !msg) invalid("msg is required");
    return this.write(() => {
      const { r, doc } = this.loadExpecting(id, undefined);
      this.commitDoc(id, r.version, doc);
      this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg });
      this.recordEvent(actor, "activity", id, { by: meta.by ?? actor });
      return this.getCard(id, { activity_limit: 5 });
    });
  }

  /** Append findings under a `## <heading>` to the body — the Tester/User findings path. */
  appendToBody(id: string, heading: string, text: string, meta: WriteMeta & { activity?: string }): Card {
    const actor = checkActor(meta.actor);
    checkAppend({ heading, text });
    return this.write(() => {
      const { r, doc } = this.loadExpecting(id, undefined);
      doc.body = withSection(doc.body, heading, text);
      this.commitDoc(id, r.version, doc);
      this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg: meta.activity ?? `Appended to spec: ${heading}` });
      this.recordEvent(actor, "body_appended", id, { heading });
      return this.getCard(id, { activity_limit: 5 });
    });
  }

  /** Answer a needs_input question: records it, returns the card to in_progress, clears the claim. */
  answerQuestion(id: string, answer: string, expectedVersion: number, meta: WriteMeta): Card {
    const actor = checkActor(meta.actor);
    if (typeof answer !== "string" || !answer.trim()) invalid("answer is required");
    checkVersion(expectedVersion);
    return this.write(() => {
      const { r, doc } = this.loadExpecting(id, expectedVersion);
      const q = doc.question;
      if (!isObj(q) || typeof q.text !== "string" || !q.text) {
        throw new BoardError("invalid", `${id} has no question to answer`, this.current(id));
      }
      const from = doc.column as string;
      doc.question = { ...q, answer };
      doc.column = "in_progress";
      doc.agent = null;
      this.commitDoc(id, r.version, doc);
      this.appendActivityRow(id, { t: now(), by: meta.by ?? actor, msg: `Question answered: ${answer}` });
      this.recordEvent(actor, "answered", id, { from, to: "in_progress" });
      return this.getCard(id, { activity_limit: 5 });
    });
  }

  /** Remove a card from the board (the page's drawer delete). Its activity rows are kept. */
  deleteCard(id: string, expectedVersion: number, meta: WriteMeta): void {
    const actor = checkActor(meta.actor);
    checkVersion(expectedVersion);
    this.write(() => {
      const { r } = this.loadExpecting(id, expectedVersion);
      const info = this.db
        .prepare("UPDATE cards SET deleted_at = ?, version = version + 1 WHERE id = ? AND version = ?")
        .run(now(), id, r.version);
      if (info.changes !== 1) throw new BoardError("stale_version", `${id} changed underneath this delete`);
      this.recordEvent(actor, "deleted", id, {});
    });
  }

  // ---------------------------------------------------------------- import / export

  isEmpty(): boolean {
    const n = (this.db.prepare("SELECT (SELECT count(*) FROM cards) + (SELECT count(*) FROM activity) AS n").get() as { n: number }).n;
    return n === 0;
  }

  /**
   * Load a board state (`{schema: 1, nextId, cards}`) into an EMPTY database,
   * preserving ids, card order, every field (legacy shapes included),
   * activity and timestamps exactly. Refuses a non-empty database.
   */
  importState(
    state: unknown,
    meta: WriteMeta,
  ): {
    cards: number;
    activity: number;
    nextId: number;
    migration: { pr_derived: number; pr_null: number; pr_kept: number; ambiguous: { id: string; candidates: number[] }[]; constructed_url: string[] };
  } {
    const actor = checkActor(meta.actor);
    if (!isObj(state)) invalid("state must be an object");
    if (state.schema !== 1) invalid(`unsupported schema ${String(state.schema)} (expected 1)`);
    if (!Number.isInteger(state.nextId)) invalid("nextId must be an integer");
    if (!Array.isArray(state.cards)) invalid("cards must be an array");
    const cards = state.cards as unknown[];
    const seen = new Set<string>();
    let prefix: string | null = null;
    let maxNum = 0;
    cards.forEach((c, i) => {
      if (!isObj(c)) invalid(`cards[${i}] is not an object`);
      const m = typeof c.id === "string" ? /^(.*?)(\d+)$/.exec(c.id) : null;
      if (!m) invalid(`cards[${i}].id must look like PREFIX-<n>`);
      if (seen.has(c.id as string)) invalid(`duplicate card id ${String(c.id)}`);
      seen.add(c.id as string);
      if (prefix !== null && m[1] !== prefix) invalid(`mixed id prefixes (${prefix}, ${m[1]})`);
      prefix = m[1];
      maxNum = Math.max(maxNum, Number(m[2]));
      if (typeof c.title !== "string") invalid(`${String(c.id)}: title must be a string`);
      if ("pr" in c) {
        try {
          checkPr(c.pr);
        } catch {
          invalid(`${String(c.id)}: pr must be null or {number, url} with a pull-request url`);
        }
      }
      if ("worktree" in c) checkWorktree(c.worktree);
      checkColumn(c.column);
      if (!Array.isArray(c.activity)) invalid(`${String(c.id)}: activity must be an array`);
      (c.activity as unknown[]).forEach((a, j) => {
        if (!isObj(a) || Object.keys(a).join(",") !== "t,by,msg" || ![a.t, a.by, a.msg].every((x) => typeof x === "string")) {
          invalid(`${String(c.id)}.activity[${j}] must be exactly {t, by, msg} strings`);
        }
      });
    });
    if ((state.nextId as number) <= maxNum) invalid(`nextId ${String(state.nextId)} is not above the highest card number ${maxNum}`);
    // Migration to the current card model: a card without `pr` gets it
    // derived (see derivePr); a card without `worktree` gets null. A state
    // that already carries them (a re-import of an export) keeps them as is.
    const toDerive = (cards as Record<string, unknown>[]).filter((c) => !("pr" in c));
    const derived = derivePr(toDerive);
    const migration = {
      pr_derived: Object.values(derived.pr).filter((p) => p !== null).length,
      pr_null: Object.values(derived.pr).filter((p) => p === null).length,
      pr_kept: cards.length - toDerive.length,
      ambiguous: derived.ambiguous,
      constructed_url: derived.constructed,
    };
    return this.write(() => {
      if (!this.isEmpty()) throw new BoardError("not_empty", "import only loads into an EMPTY database; this one already has cards");
      let acts = 0;
      const ins = this.db.prepare("INSERT INTO cards(id, doc) VALUES (?, ?)");
      for (const c of cards as Record<string, unknown>[]) {
        const doc: Record<string, unknown> = { ...c, activity: null };
        if (!("pr" in c)) doc.pr = derived.pr[String(c.id)];
        if (!("worktree" in c)) doc.worktree = null;
        ins.run(c.id, JSON.stringify(doc));
        for (const a of c.activity as ActivityEntry[]) {
          this.appendActivityRow(c.id as string, a);
          acts++;
        }
      }
      this.db.prepare("UPDATE meta SET value = ? WHERE key = 'next_id'").run(String(state.nextId));
      if (prefix !== null) this.db.prepare("UPDATE meta SET value = ? WHERE key = 'id_prefix'").run(prefix);
      this.recordEvent(actor, "imported", null, { cards: cards.length, activity: acts });
      return { cards: cards.length, activity: acts, nextId: state.nextId as number, migration };
    });
  }

  /** The full state, in the artifact's `{schema: 1, nextId, cards}` shape. The backup and rollback path. */
  exportState(): { schema: 1; nextId: number; cards: Card[] } {
    const rows = this.db
      .prepare("SELECT id, version, doc FROM cards WHERE deleted_at IS NULL ORDER BY rowid")
      .all() as CardRow[];
    const cards = rows.map((r) => {
      const doc = JSON.parse(r.doc) as Card;
      if ("activity" in doc) doc.activity = this.activity(r.id);
      return doc;
    });
    return { schema: 1, nextId: Number(this.meta("next_id")), cards };
  }
}
