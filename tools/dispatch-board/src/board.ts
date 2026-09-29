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
import { createHash, randomUUID } from "node:crypto";
import { existsSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

export const COLUMNS = [
  "backlog",
  "needs_input",
  "in_progress",
  "review",
  "merged",
  "done",
  "archived",
] as const;
export type Column = (typeof COLUMNS)[number];
export const ROLES = ["engineer", "tester", "release-manager", "pm"] as const;
export type Role = (typeof ROLES)[number];
/** Columns a non-pm card may not enter without its own PR, `pr` (the AS-4 gate). */
export const GATED_COLUMNS: readonly string[] = ["review", "merged"];
/** What a card changed, for the milestone review's WHAT CHANGED grouping. */
export const AREAS = ["combat", "visuals", "ai", "ui", "tooling"] as const;
/** Where a card's balance sweep happened: on the card, in the milestone sweep, or nowhere. */
export const SWEEP_STATUSES = ["done-on-card", "deferred-to-milestone", "none"] as const;
export const MILESTONE_STATUSES = ["open", "in_review", "released"] as const;
/** The milestone columns a card is finished in: its work is on main. */
const FINISHED_COLUMNS: readonly string[] = ["merged", "done", "archived"];

/**
 * The database layout this build reads and writes. 1 is the AS-153 board
 * (with a `human_review` column); 2 adds milestones and the `merged` column.
 * A writable open of an older database is refused until `migrate` has run.
 */
export const SCHEMA_VERSION = 2;
/** The one activity line a human_review card gains when it becomes a merged card (import or migrate). */
export const HUMAN_REVIEW_MIGRATION_NOTE =
  "Migrated human_review → merged (schema 2): Tester-approved before milestones existed, merge not recorded — mark_merged records it";

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
/**
 * A pull request URL; the number is read from it. The ONE definition: the
 * daemon also serves it to the web UI (see server.ts), so the page's gate
 * check can never be looser or stricter than the board's.
 */
export const PR_URL = /^https?:\/\/[^\s/]+\/\S*\/pull\/(\d+)\/?$/;
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

/** A user decision recorded on a card: dated, with the numbers it turned on. */
export interface Ruling {
  t: string;
  by: string;
  text: string;
  numbers?: Record<string, number | string>;
}
/** A milestone as the protocol sees it; `version` rides along like a card's. */
export type Milestone = Record<string, unknown> & {
  name: string;
  status: (typeof MILESTONE_STATUSES)[number];
  created: string;
};

export type ErrorCode =
  | "not_found"
  | "stale_version"
  | "claim_refused"
  | "gate_refused"
  | "invalid"
  | "not_empty"
  | "cursor_ahead"
  | "needs_migration";

export class BoardError extends Error {
  constructor(
    public code: ErrorCode,
    message: string,
    /** The card as it is NOW, for refusals a caller must re-read to recover from. */
    public card?: Card,
    /** The milestone (or review item) as it is now, for a refused milestone write. */
    public current?: Record<string, unknown>,
  ) {
    super(message);
    this.name = "BoardError";
  }
  toJSON() {
    return { error: this.code, message: this.message, card: this.card, ...(this.current ? { current: this.current } : {}) };
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
  "milestone",
  "iteration",
  "area",
] as const;

/**
 * Patchable through update_card / move_card. `column` is move_card's alone;
 * `rulings` grow only through record_ruling, and `merge_sha` is set only by
 * mark_merged, so neither can be rewritten by a patch.
 */
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
  "milestone",
  "iteration",
  "area",
  "summary",
  "human_testing",
  "sweep",
  "gaps",
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
CREATE TABLE IF NOT EXISTS milestones (
  name    TEXT NOT NULL UNIQUE,
  version INTEGER NOT NULL DEFAULT 1,
  doc     TEXT NOT NULL CHECK (json_valid(doc))
);
CREATE TABLE IF NOT EXISTS review_items (
  milestone  TEXT NOT NULL REFERENCES milestones(name),
  key        TEXT NOT NULL,
  version    INTEGER NOT NULL DEFAULT 1,
  checked_at TEXT,
  checked_by TEXT,
  draft      TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (milestone, key)
);
`;

interface MilestoneRow {
  name: string;
  version: number;
  doc: string;
}
/** One checklist tick and/or feedback draft on a milestone's review page. */
export interface ReviewItem {
  key: string;
  version: number;
  checked_at: string | null;
  checked_by: string | null;
  draft: string;
}

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

/** A milestone name: "0.7", "v0.8-hotfix" — short, URL-safe, starts alphanumeric. */
export const MILESTONE_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,31}$/;

function checkMilestoneName(v: unknown): string {
  if (typeof v !== "string" || !MILESTONE_NAME.test(v)) invalid("milestone name must be 1-32 of [A-Za-z0-9._-], starting alphanumeric (e.g. 0.7)");
  return v as string;
}

/** A commit id as git abbreviates or prints it. */
function checkSha(v: unknown, what: string): string {
  if (typeof v !== "string" || !/^[0-9a-f]{7,40}$/i.test(v)) invalid(`${what} must be a commit sha (7-40 hex digits)`);
  return (v as string).toLowerCase();
}

function checkText(v: unknown, what: string, opts: { nullable?: boolean; nonEmpty?: boolean; singleLine?: boolean } = {}): string | null {
  if (v === null && opts.nullable) return null;
  if (typeof v !== "string") invalid(`${what} must be a string${opts.nullable ? " or null" : ""}`);
  if (opts.nonEmpty && !(v as string).trim()) invalid(`${what} must not be empty`);
  if (opts.singleLine && /[\r\n]/.test(v as string)) invalid(`${what} must be one line`);
  return v as string;
}

function checkUrl(v: unknown, what: string): string {
  if (typeof v !== "string" || !/^https?:\/\/\S+$/.test(v)) invalid(`${what} must be an http(s) URL`);
  return v as string;
}

/** A ruling's numbers: {label: value}, each a finite number or a short string ("+36pt", "z=5.2"). */
function checkNumbers(v: unknown): Record<string, number | string> | undefined {
  if (v === undefined || v === null) return undefined;
  if (!isObj(v)) invalid("numbers must be an object of {label: number | string}");
  const entries = Object.entries(v as Record<string, unknown>);
  if (entries.length > 20) invalid("numbers holds at most 20 entries");
  for (const [k, x] of entries) {
    if (!k.trim()) invalid("numbers labels must be non-empty");
    const ok = (typeof x === "number" && Number.isFinite(x)) || (typeof x === "string" && x.trim() !== "" && x.length <= 80);
    if (!ok) invalid(`numbers.${k} must be a finite number or a non-empty string`);
  }
  return entries.length ? (v as Record<string, number | string>) : undefined;
}

function checkSweep(v: unknown): { status: string; summary?: string } | null {
  if (v === null) return null;
  if (!isObj(v) || !(SWEEP_STATUSES as readonly unknown[]).includes(v.status)) {
    invalid(`sweep must be null or {status: ${SWEEP_STATUSES.join(" | ")}, summary?}`);
  }
  const keys = Object.keys(v as object).filter((k) => k !== "status" && k !== "summary");
  if (keys.length) invalid(`sweep has unknown field(s): ${keys.join(", ")}`);
  if ((v as Record<string, unknown>).summary !== undefined) checkText((v as Record<string, unknown>).summary, "sweep.summary", { singleLine: true });
  return v as { status: string; summary?: string };
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
      case "milestone":
        // Existence (and not-released) is checked inside the write, against the table.
        if (v !== null) checkMilestoneName(v);
        break;
      case "iteration":
        if (v !== null && !(Number.isInteger(v) && (v as number) >= 1)) invalid("iteration must be null or an integer >= 1");
        break;
      case "area":
        if (v !== null && !(AREAS as readonly unknown[]).includes(v)) invalid(`area must be null or one of ${AREAS.join(", ")}`);
        break;
      case "summary":
      case "human_testing":
      case "gaps":
        checkText(v, k, { nullable: true });
        break;
      case "sweep":
        checkSweep(v);
        break;
    }
  }
  return out;
}

/** The named fields of `o` that it has, in the order named. */
function pick(o: Record<string, unknown>, keys: string[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const k of keys) if (k in o) out[k] = o[k];
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
 * "PR #N" in each such entry. The orchestrator's move record ("Moved:
 * in_progress -> review. PR #N ...") counts too, but only when the PR is its
 * immediate subject: further into such an entry a PR is usually a reference
 * (a merge order, a sibling card). Exactly one distinct N across a card's
 * hand-offs gives `pr`; none gives null; several give null and are reported
 * as ambiguous. The url is the card's own link to /pull/N when it has one,
 * else built from the board's single repository base (reported as
 * constructed; with no single base it is left null and reported ambiguous).
 */
export function derivePr(
  cards: Record<string, unknown>[],
  /** The whole board, whose PR urls name the repository; defaults to `cards`. */
  board: Record<string, unknown>[] = cards,
): {
  pr: Record<string, Pr | null>;
  ambiguous: { id: string; candidates: number[] }[];
  constructed: string[];
  /** Hand-off entries naming 2+ distinct PRs: the first was taken; listed so a human can check it. */
  multi: { id: string; t: string; numbers: number[] }[];
} {
  // The PR a hand-off entry names as the card's own; null for a hand-off
  // naming none; undefined for an entry that is not a hand-off record.
  const handoffPr = (a: ActivityEntry): number | null | undefined => {
    if ((a.by === "engineer" && /^READY/.test(a.msg)) || (a.by === "orchestrator" && /^(ENGINEER DONE|READY FOR REVIEW)/.test(a.msg))) {
      const m = /PR #(\d+)/.exec(a.msg);
      return m ? Number(m[1]) : null;
    }
    if (a.by === "orchestrator") {
      const m = /^Moved: in_progress (?:->|→) review\. PR #(\d+)\b/.exec(a.msg);
      if (m) return Number(m[1]);
    }
    return undefined;
  };
  const bases = new Set<string>();
  for (const c of board) {
    // Every PR url the board already has — reference links and carried prs — names its repository.
    for (const l of [...(Array.isArray(c.links) ? c.links : []), ...(isObj(c.pr) ? [c.pr] : [])]) {
      const m = isObj(l) && typeof l.url === "string" ? /^(https?:\/\/\S+?)\/pull\/\d+\/?$/.exec(l.url) : null;
      if (m) bases.add(m[1]);
    }
  }
  const base = bases.size === 1 ? [...bases][0] : null;
  const out = {
    pr: {} as Record<string, Pr | null>,
    ambiguous: [] as { id: string; candidates: number[] }[],
    constructed: [] as string[],
    multi: [] as { id: string; t: string; numbers: number[] }[],
  };
  for (const c of cards) {
    const id = String(c.id);
    const nums = new Set<number>();
    for (const a of (Array.isArray(c.activity) ? c.activity : []) as ActivityEntry[]) {
      const n = handoffPr(a);
      if (n === undefined) continue;
      if (n !== null) nums.add(n);
      const named = [...new Set([...a.msg.matchAll(/PR #(\d+)/g)].map((m) => Number(m[1])))];
      if (named.length > 1) out.multi.push({ id, t: a.t, numbers: named });
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

/**
 * A card's human-testing text as checklist steps: one per non-empty line,
 * with the PR's `**Human testing:**` lead-in and list markers stripped. Each
 * step's key hashes its own text, so a tick stays on the step it was given to
 * and a REWORDED step comes back unticked — it is a new thing to check.
 */
export function checklistSteps(cardId: string, text: unknown): { key: string; text: string }[] {
  const seen = new Set<string>();
  const out: { key: string; text: string }[] = [];
  for (const line of stepLines(text)) {
    if (NOTHING_TO_CHECK.test(line)) continue;
    const key = `${cardId}:${createHash("sha1").update(line).digest("hex").slice(0, 10)}`;
    if (seen.has(key)) continue; // the same step twice is one thing to check
    seen.add(key);
    out.push({ key, text: line });
  }
  return out;
}

/**
 * The PR convention's explicit "nothing to check" — "Nothing needs human
 * testing." with at most a qualifying clause, one sentence. It is a statement,
 * not a step: it never becomes a checkbox, and the review page says the card
 * has nothing to check rather than warning that its steps are missing.
 */
const NOTHING_TO_CHECK = /^nothing (?:else )?needs? (?:any )?human testing\b[^.]*\.?$/i;

/** A human-testing text's lines, with the PR's `**Human testing:**` lead-in and list markers stripped. */
function stepLines(text: unknown): string[] {
  if (typeof text !== "string") return [];
  return text
    .split(/\r?\n/)
    .map((raw) =>
      raw
        .replace(/^\s*\*\*Human testing:\*\*\s*/i, "")
        .replace(/^\s*(?:[-*+]|\d+[.)])\s+/, "")
        .replace(/^\[[ xX]\]\s+/, "")
        .trim(),
    )
    .filter(Boolean);
}

/** A card whose human testing says, explicitly, that there is nothing to check. */
export function saysNothingToCheck(text: unknown): boolean {
  return stepLines(text).some((l) => NOTHING_TO_CHECK.test(l));
}

/**
 * The two columns a plain move may not reach, so each keeps its meaning for
 * every writer (the web UI's drag included):
 *  - `merged` means merged to main, which only `mark_merged` records. A move
 *    may put back a card whose merge is already recorded, never one without.
 *  - a work card on an unreleased milestone reaches `done` only when
 *    `close_milestone` closes that milestone — `done` is the milestone's
 *    approval, not the card's. (pm cards finish on their own; a card on a
 *    released milestone, or on none, moves freely.)
 * Returns why the move is refused, or null.
 */
function milestoneMoveRefusal(
  doc: Record<string, unknown>,
  from: string,
  to: string,
  status: (name: string) => string | null,
): string | null {
  if (to === "merged" && from !== "merged" && doc.merge_sha == null) {
    return "a card enters merged only through mark_merged, which records its merge on main";
  }
  if (to === "done" && from !== "done" && doc.role !== "pm" && typeof doc.milestone === "string") {
    const s = status(doc.milestone);
    if (s !== null && s !== "released") {
      return `it is on milestone ${doc.milestone}, whose cards reach done only when close_milestone closes it`;
    }
  }
  return null;
}

/** The AS-4 PR gate: a non-pm card enters review/merged only with its own PR. */
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
  /** Only cards on this milestone; null for cards on none. */
  milestone?: string | null;
  iteration?: number;
}

export interface WriteMeta {
  actor: string;
  /** Activity `by`; defaults to the actor. */
  by?: string;
}

/** The schema a database is at: null for a brand-new (empty) file. Reads only. */
function storedSchema(db: Database.Database): number | null {
  const hasMeta = db.prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta'").get();
  if (!hasMeta) return null;
  const r = db.prepare("SELECT value FROM meta WHERE key = 'schema'").get() as { value: string } | undefined;
  return r ? Number(r.value) : null;
}

export class Board extends EventEmitter {
  readonly db: Database.Database;
  /** The layout of the database this Board opened (SCHEMA_VERSION once migrated). */
  readonly schema: number;

  /**
   * `readonly`: the export path — reads any schema, writes nothing.
   * `allowOld`: open an older schema WITHOUT touching it (migrate's own open);
   * every other writable open of an older database is refused, so a new
   * daemon can never serve cards in a column it does not know.
   */
  constructor(readonly path: string, opts: { readonly?: boolean; allowOld?: boolean } = {}) {
    super();
    if (!opts.readonly && path !== ":memory:") mkdirSync(dirname(path), { recursive: true });
    this.db = new Database(path, { readonly: !!opts.readonly, fileMustExist: !!opts.readonly });
    this.db.pragma("busy_timeout = 10000");
    const stored = storedSchema(this.db);
    if (stored !== null && stored > SCHEMA_VERSION) {
      this.db.close();
      throw new BoardError("needs_migration", `${path} is at schema ${stored}, newer than this build (${SCHEMA_VERSION}): run the build that wrote it`);
    }
    if (opts.readonly) {
      this.schema = stored ?? SCHEMA_VERSION;
      return;
    }
    if (stored !== null && stored < SCHEMA_VERSION) {
      if (!opts.allowOld) {
        this.db.close();
        throw new BoardError(
          "needs_migration",
          `${path} is at schema ${stored}; this build needs ${SCHEMA_VERSION}. Stop the daemon, back it up (dist/cli.js export), then run dist/cli.js migrate.`,
        );
      }
      this.schema = stored;
      return;
    }
    this.db.pragma("journal_mode = WAL");
    this.db.pragma("foreign_keys = ON");
    this.db.exec(SCHEMA);
    this.db
      .prepare("INSERT OR IGNORE INTO meta(key, value) VALUES ('schema', ?), ('next_id', '1'), ('id_prefix', 'AS-'), ('board_id', ?)")
      .run(String(SCHEMA_VERSION), randomUUID());
    this.schema = SCHEMA_VERSION;
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

  private milestoneRow(name: string): MilestoneRow {
    const r = this.db.prepare("SELECT name, version, doc FROM milestones WHERE name = ?").get(name) as MilestoneRow | undefined;
    if (!r) throw new BoardError("not_found", `no milestone ${name}`);
    return r;
  }

  private milestoneOf(r: MilestoneRow): Milestone {
    return Object.assign(JSON.parse(r.doc) as Milestone, { version: r.version });
  }

  /** Load a milestone for a versioned write, refusing a stale expected_version up front. */
  private loadMilestone(name: string, expected: number): { m: MilestoneRow; doc: Record<string, unknown> } {
    const m = this.milestoneRow(name);
    if (m.version !== expected) {
      throw new BoardError(
        "stale_version",
        `milestone ${name} is at version ${m.version}, not ${expected}: it changed since you read it. Re-read and re-apply.`,
        undefined,
        this.milestoneOf(m),
      );
    }
    return { m, doc: JSON.parse(m.doc) };
  }

  private commitMilestone(name: string, fromVersion: number, doc: Record<string, unknown>): void {
    doc.updated = now();
    const info = this.db
      .prepare("UPDATE milestones SET doc = ?, version = version + 1 WHERE name = ? AND version = ?")
      .run(JSON.stringify(doc), name, fromVersion);
    if (info.changes !== 1) {
      throw new BoardError("stale_version", `milestone ${name} changed underneath this write; re-read it`, undefined, this.milestoneOf(this.milestoneRow(name)));
    }
  }

  /**
   * A card may be put on a milestone only if the milestone exists and is not
   * released: a released milestone's card list is what its release shipped.
   */
  private checkAttach(name: unknown): void {
    if (name === null || name === undefined) return;
    const r = this.db.prepare("SELECT doc FROM milestones WHERE name = ?").get(name) as { doc: string } | undefined;
    if (!r) invalid(`no milestone ${String(name)}: create it first (create_milestone)`);
    if ((JSON.parse(r.doc) as Milestone).status === "released") invalid(`milestone ${String(name)} is released; its card list is closed`);
  }

  /** A milestone's status, or null when there is no such milestone. */
  private milestoneStatus(name: string): string | null {
    const r = this.db.prepare("SELECT doc FROM milestones WHERE name = ?").get(name) as { doc: string } | undefined;
    return r ? String((JSON.parse(r.doc) as Milestone).status) : null;
  }

  /** A card's milestone fields after a patch: a card that gains a milestone and has no iteration is iteration 1. */
  private applyMilestonePatch(doc: Record<string, unknown>, patch: Record<string, unknown>): void {
    if ("milestone" in patch && patch.milestone !== doc.milestone) this.checkAttach(patch.milestone);
    Object.assign(doc, patch);
    if (doc.milestone != null && doc.iteration == null) doc.iteration = 1;
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
    if (opts.milestone === null) {
      where.push("json_extract(doc, '$.milestone') IS NULL");
    } else if (opts.milestone !== undefined) {
      where.push("json_extract(doc, '$.milestone') = ?");
      args.push(checkMilestoneName(opts.milestone));
    }
    if (opts.iteration !== undefined) {
      if (!Number.isInteger(opts.iteration)) invalid("iteration must be an integer");
      where.push("json_extract(doc, '$.iteration') = ?");
      args.push(opts.iteration);
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
      milestone?: string | null;
      iteration?: number | null;
      area?: string | null;
      summary?: string | null;
      human_testing?: string | null;
      sweep?: unknown;
      gaps?: string | null;
    },
    meta: WriteMeta,
  ): Card {
    const actor = checkActor(meta.actor);
    const column = checkColumn(input.column ?? "backlog");
    // The milestone fields ride along only when given, so a card filed
    // without them keeps the same shape as every card before milestones.
    const extra: Record<string, unknown> = {};
    for (const k of ["milestone", "iteration", "area", "summary", "human_testing", "sweep", "gaps"] as const) {
      if (input[k] !== undefined) extra[k] = input[k];
    }
    const checked = checkPatch({
      title: input.title,
      body: input.body ?? "",
      role: input.role,
      priority: input.priority ?? "P2",
      links: input.links ?? [],
      pr: input.pr ?? null,
      worktree: input.worktree ?? null,
      ...extra,
    });
    return this.write(() => {
      this.checkAttach(extra.milestone);
      if (extra.milestone != null && extra.iteration == null) extra.iteration = 1;
      const t = now();
      const doc: Record<string, unknown> = {
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
        ...extra,
      };
      if (!gateAllows(doc, column)) {
        throw new BoardError("gate_refused", `a non-pm card needs its own PR (pr) to enter ${column}`);
      }
      const why = milestoneMoveRefusal(doc, "", column, (name) => this.milestoneStatus(name));
      if (why) invalid(`a new card cannot be filed there: ${why}`);
      return this.insertCard(doc, meta.by ?? actor, actor, "Created");
    });
  }

  /** Allocate the next id to `doc` and insert it (inside a write). */
  private insertCard(doc: Record<string, unknown>, by: string, actor: string, msg: string): Card {
    const prefix = this.meta("id_prefix");
    let n = Number(this.meta("next_id"));
    while (this.db.prepare("SELECT 1 FROM cards WHERE id = ?").get(`${prefix}${n}`)) n++;
    const id = `${prefix}${n}`;
    doc = { id, ...doc };
    this.db.prepare("INSERT INTO cards(id, doc) VALUES (?, ?)").run(id, JSON.stringify(doc));
    this.db.prepare("UPDATE meta SET value = ? WHERE key = 'next_id'").run(String(n + 1));
    this.appendActivityRow(id, { t: String(doc.created), by, msg });
    this.recordEvent(actor, "created", id, {
      column: doc.column,
      title: doc.title,
      ...(doc.milestone != null ? { milestone: doc.milestone, iteration: doc.iteration } : {}),
    });
    return this.getCard(id);
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
      this.applyMilestonePatch(doc, patch);
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
      this.applyMilestonePatch(doc, patch);
      if (append) doc.body = withSection(doc.body, append.heading, append.text);
      if (!gateAllows(doc, to)) {
        throw new BoardError(
          "gate_refused",
          `${id} needs its own PR to enter ${to} (role ${String(doc.role)}; only pm cards are exempt). Set pr: {url} in this move's patch — links are references and do not count.`,
          this.current(id),
        );
      }
      const why = milestoneMoveRefusal(doc, from, to, (name) => this.milestoneStatus(name));
      if (why) throw new BoardError("invalid", `${id}: ${why}`, this.current(id));
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

  /**
   * Record a user decision on a card: dated, with the numbers it turned on.
   * Rulings are append-only — like activity, and unlike the body sections
   * they used to live in — so the milestone review can list them verbatim.
   * An append, so `expected_version` is optional (as for append_to_body).
   */
  recordRuling(
    id: string,
    input: { text: string; numbers?: Record<string, number | string> },
    meta: WriteMeta & { expected_version?: number },
  ): Card {
    const actor = checkActor(meta.actor);
    const text = checkText(input.text, "text", { nonEmpty: true }) as string;
    const numbers = checkNumbers(input.numbers);
    if (meta.expected_version !== undefined) checkVersion(meta.expected_version);
    return this.write(() => {
      const { r, doc } = this.loadExpecting(id, meta.expected_version);
      const ruling: Ruling = { t: now(), by: meta.by ?? actor, text, ...(numbers ? { numbers } : {}) };
      doc.rulings = [...(Array.isArray(doc.rulings) ? doc.rulings : []), ruling];
      this.commitDoc(id, r.version, doc);
      this.appendActivityRow(id, { t: ruling.t, by: ruling.by, msg: `Ruling recorded: ${text}` });
      this.recordEvent(actor, "ruling", id, { ...(doc.milestone != null ? { milestone: doc.milestone } : {}) });
      return this.getCard(id, { activity_limit: 5 });
    });
  }

  /**
   * A Tester-approved card whose PR the orchestrator has merged: review ->
   * merged, recording the PR and its merge commit, and closing out a working
   * (Tester) claim — one write. A card already in merged with no merge
   * recorded (a migrated human_review card) may have it recorded here too.
   */
  markMerged(
    id: string,
    input: { pr: unknown; merge_sha: unknown },
    expectedVersion: number,
    meta: WriteMeta & { activity?: string },
  ): Card {
    const actor = checkActor(meta.actor);
    const pr = checkPr(input.pr ?? null);
    if (!pr) invalid("pr is required: the card's own pull request, {url}");
    const sha = checkSha(input.merge_sha, "merge_sha");
    checkVersion(expectedVersion);
    return this.write(() => {
      const { r, doc } = this.loadExpecting(id, expectedVersion);
      const from = doc.column as string;
      if (from !== "review" && !(from === "merged" && doc.merge_sha == null)) {
        const why = from === "merged" ? `its merge is already recorded (${String(doc.merge_sha)})` : `it is in ${from}`;
        throw new BoardError("invalid", `${id} cannot be marked merged: ${why}. Only a review card (or a merged card with no merge recorded) can.`, this.current(id));
      }
      if (isObj(doc.pr) && doc.pr.number !== pr.number) {
        throw new BoardError("invalid", `${id}'s own PR is #${String(doc.pr.number)}, not #${pr.number}`, this.current(id));
      }
      if (doc.role === "pm") throw new BoardError("invalid", `${id} is a pm card: it has no PR to merge`, this.current(id));
      const stored = doc.agent;
      doc.pr = pr;
      doc.merge_sha = sha;
      doc.merged_at = now();
      doc.column = "merged";
      if (isObj(stored) && stored.status === "working") doc.agent = { ...stored, status: "done", finished: now() };
      const claimNote = claimChangeNote(stored, { agent: doc.agent ?? null });
      this.commitDoc(id, r.version, doc);
      const by = meta.by ?? actor;
      if (meta.activity) this.appendActivityRow(id, { t: now(), by, msg: meta.activity });
      this.appendActivityRow(id, { t: now(), by, msg: `Merged: PR #${pr.number} at ${sha.slice(0, 10)}` });
      if (claimNote) this.appendActivityRow(id, { t: now(), by, msg: claimNote });
      this.recordEvent(actor, "merged", id, {
        from,
        to: "merged",
        pr: pr.number,
        merge_sha: sha,
        ...(doc.milestone != null ? { milestone: doc.milestone } : {}),
      });
      return this.getCard(id, { activity_limit: 5 });
    });
  }

  // ---------------------------------------------------------------- milestones

  listMilestones(): Record<string, unknown>[] {
    const rows = this.db.prepare("SELECT name, version, doc FROM milestones ORDER BY rowid").all() as MilestoneRow[];
    const counts = this.db
      .prepare(
        "SELECT json_extract(doc, '$.milestone') AS m, col, count(*) AS n FROM cards WHERE deleted_at IS NULL AND json_extract(doc, '$.milestone') IS NOT NULL GROUP BY m, col",
      )
      .all() as { m: string; col: string; n: number }[];
    return rows.map((r) => {
      const m = this.milestoneOf(r);
      const cards: Record<string, number> = {};
      for (const c of counts) if (c.m === r.name) cards[c.col] = c.n;
      return { name: m.name, status: m.status, created: m.created, released_at: m.released_at ?? null, tag: m.tag ?? null, version: r.version, cards };
    });
  }

  createMilestone(name: string, meta: WriteMeta): Milestone {
    const actor = checkActor(meta.actor);
    checkMilestoneName(name);
    return this.write(() => {
      if (this.db.prepare("SELECT 1 FROM milestones WHERE name = ?").get(name)) invalid(`milestone ${name} already exists`);
      const t = now();
      // Card ids are allocated in order, so the ids issued while the milestone
      // is open (from first_id, up to end_id once it closes) are exactly the
      // cards filed during it — the review's follow-ups — with no clock involved.
      const doc = { name, status: "open", created: t, updated: t, first_id: Number(this.meta("next_id")), sweep: null, submissions: [] };
      this.db.prepare("INSERT INTO milestones(name, doc) VALUES (?, ?)").run(name, JSON.stringify(doc));
      this.recordEvent(actor, "milestone_created", null, { milestone: name });
      return this.milestoneOf(this.milestoneRow(name));
    });
  }

  /**
   * Status open <-> in_review, the tag and release URL, and the SHA the review
   * checklist applies to. `released` is reached only through close_milestone,
   * and is terminal.
   */
  updateMilestone(name: string, patch: Record<string, unknown>, expectedVersion: number, meta: WriteMeta): Milestone {
    const actor = checkActor(meta.actor);
    if (!isObj(patch) || !Object.keys(patch).length) invalid("patch must be a non-empty object");
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(patch)) {
      switch (k) {
        case "status":
          if (v !== "open" && v !== "in_review") invalid("status may be set to open or in_review; released is close_milestone's");
          out.status = v;
          break;
        case "tag":
          out.tag = v === null ? null : checkText(v, "tag", { nonEmpty: true, singleLine: true });
          break;
        case "release_url":
          out.release_url = v === null ? null : checkUrl(v, "release_url");
          break;
        case "review_sha":
          out.review_sha = v === null ? null : checkSha(v, "review_sha");
          break;
        default:
          invalid(`milestone field '${k}' is not patchable (patchable: status, tag, release_url, review_sha)`);
      }
    }
    checkVersion(expectedVersion);
    return this.write(() => {
      const { m, doc } = this.loadMilestone(name, expectedVersion);
      if (doc.status === "released" && "status" in out) {
        throw new BoardError("invalid", `milestone ${name} is released; its status is final`, undefined, this.milestoneOf(m));
      }
      Object.assign(doc, out);
      this.commitMilestone(name, m.version, doc);
      this.recordEvent(actor, "milestone_updated", null, { milestone: name, fields: Object.keys(out) });
      return this.milestoneOf(this.milestoneRow(name));
    });
  }

  /** The milestone sweep's result: one summary line and a link to the committed doc/CSV. */
  setMilestoneSweep(name: string, input: { summary: unknown; link?: unknown }, expectedVersion: number, meta: WriteMeta): Milestone {
    const actor = checkActor(meta.actor);
    const summary = checkText(input.summary, "summary", { nonEmpty: true }) as string;
    const link = input.link === undefined || input.link === null ? null : (checkText(input.link, "link", { nonEmpty: true, singleLine: true }) as string);
    checkVersion(expectedVersion);
    return this.write(() => {
      const { m, doc } = this.loadMilestone(name, expectedVersion);
      doc.sweep = { summary, link, t: now(), by: meta.by ?? actor };
      this.commitMilestone(name, m.version, doc);
      this.recordEvent(actor, "milestone_sweep", null, { milestone: name });
      return this.milestoneOf(this.milestoneRow(name));
    });
  }

  /**
   * Close a milestone the user has approved: every one of its merged cards
   * moves to done, and the milestone becomes released — one write. Refused
   * while any of its cards is unfinished (move those to another milestone, or
   * finish them, first). Returns the done cards the release will bundle.
   */
  closeMilestone(
    name: string,
    expectedVersion: number,
    meta: WriteMeta & { tag?: string; release_url?: string },
  ): { milestone: Milestone; cards: Record<string, unknown>[] } {
    const actor = checkActor(meta.actor);
    const tag = meta.tag === undefined ? undefined : (checkText(meta.tag, "tag", { nonEmpty: true, singleLine: true }) as string);
    const releaseUrl = meta.release_url === undefined ? undefined : checkUrl(meta.release_url, "release_url");
    checkVersion(expectedVersion);
    return this.write(() => {
      const { m, doc } = this.loadMilestone(name, expectedVersion);
      if (doc.status === "released") throw new BoardError("invalid", `milestone ${name} is already released`, undefined, this.milestoneOf(m));
      const rows = this.milestoneCardRows(name);
      const unfinished = rows.map((r) => JSON.parse(r.doc) as Record<string, unknown>).filter((c) => !FINISHED_COLUMNS.includes(c.column as string));
      if (unfinished.length) {
        throw new BoardError(
          "invalid",
          `milestone ${name} has unfinished cards: ${unfinished.map((c) => `${String(c.id)} (${String(c.column)})`).join(", ")} — finish them or move them to another milestone first`,
          undefined,
          this.milestoneOf(m),
        );
      }
      const by = meta.by ?? actor;
      const moved: string[] = [];
      for (const r of rows) {
        const c = JSON.parse(r.doc) as Record<string, unknown>;
        if (c.column !== "merged") continue;
        c.column = "done";
        this.commitDoc(r.id, r.version, c);
        this.appendActivityRow(r.id, { t: now(), by, msg: `Milestone ${name} closed: merged → done` });
        this.recordEvent(actor, "moved", r.id, { from: "merged", to: "done", milestone: name });
        moved.push(r.id);
      }
      doc.status = "released";
      doc.released_at = now();
      doc.end_id = Number(this.meta("next_id"));
      if (tag !== undefined) doc.tag = tag;
      if (releaseUrl !== undefined) doc.release_url = releaseUrl;
      this.commitMilestone(name, m.version, doc);
      this.recordEvent(actor, "milestone_closed", null, { milestone: name, cards: moved });
      const cards = this.milestoneCardRows(name)
        .map((r) => JSON.parse(r.doc) as Record<string, unknown>)
        .filter((c) => c.column === "done" && !c.released)
        .map((c) => pick(c, ["id", "title", "role", "pr", "merge_sha", "summary", "area", "iteration"]));
      return { milestone: this.milestoneOf(this.milestoneRow(name)), cards };
    });
  }

  private milestoneCardRows(name: string): CardRow[] {
    return this.db
      .prepare("SELECT id, version, doc FROM cards WHERE deleted_at IS NULL AND json_extract(doc, '$.milestone') = ? ORDER BY rowid")
      .all(name) as CardRow[];
  }

  private reviewItems(name: string): Map<string, ReviewItem> {
    const rows = this.db
      .prepare("SELECT key, version, checked_at, checked_by, draft FROM review_items WHERE milestone = ? ORDER BY rowid")
      .all(name) as ReviewItem[];
    return new Map(rows.map((r) => [r.key, r]));
  }

  /**
   * What a review page may tick or comment on, right now: the milestone as a
   * whole, each of its cards, and each current checklist step of a finished
   * card. A key outside this set is refused, so a tick cannot land on a step
   * that has since been reworded away.
   */
  private reviewKeys(name: string): { comment: Set<string>; check: Map<string, { card: Record<string, unknown>; text: string }> } {
    const comment = new Set(["milestone"]);
    const check = new Map<string, { card: Record<string, unknown>; text: string }>();
    for (const r of this.milestoneCardRows(name)) {
      const c = JSON.parse(r.doc) as Record<string, unknown>;
      comment.add(`card:${r.id}`);
      if (!FINISHED_COLUMNS.includes(c.column as string)) continue;
      for (const s of checklistSteps(r.id, c.human_testing)) {
        check.set(`check:${s.key}`, { card: c, text: s.text });
        comment.add(`check:${s.key}`);
      }
    }
    return { comment, check };
  }

  /** Write one review item under its own version; creates it at version 1 when expected is 0. */
  private writeReviewItem(name: string, key: string, expected: number, set: Partial<ReviewItem>): ReviewItem {
    const cur = this.reviewItems(name).get(key);
    const have = cur?.version ?? 0;
    if (have !== expected) {
      throw new BoardError(
        "stale_version",
        `review item ${key} on milestone ${name} is at version ${have}, not ${expected}: it changed elsewhere (another window?)`,
        undefined,
        cur ? { ...cur } : { key, version: 0, checked_at: null, checked_by: null, draft: "" },
      );
    }
    const next: ReviewItem = { key, version: have + 1, checked_at: null, checked_by: null, draft: "", ...(cur ?? {}), ...set };
    next.version = have + 1;
    if (cur) {
      const info = this.db
        .prepare("UPDATE review_items SET version = ?, checked_at = ?, checked_by = ?, draft = ? WHERE milestone = ? AND key = ? AND version = ?")
        .run(next.version, next.checked_at, next.checked_by, next.draft, name, key, have);
      if (info.changes !== 1) throw new BoardError("stale_version", `review item ${key} changed underneath this write`);
    } else {
      this.db
        .prepare("INSERT INTO review_items(milestone, key, version, checked_at, checked_by, draft) VALUES (?, ?, ?, ?, ?, ?)")
        .run(name, key, next.version, next.checked_at, next.checked_by, next.draft);
    }
    return next;
  }

  private openForReview(name: string): Record<string, unknown> {
    const doc = JSON.parse(this.milestoneRow(name).doc) as Record<string, unknown>;
    if (doc.status === "released") invalid(`milestone ${name} is released; its review is closed`);
    return doc;
  }

  /**
   * Tick or untick one checklist step. Ticks are the user's working state,
   * not board events: nothing acts on them, and the orchestrator's wait would
   * otherwise wake for every box. Open pages still refresh (a `nudge`).
   */
  setReviewCheck(name: string, key: string, checked: boolean, expectedVersion: number, meta: WriteMeta): ReviewItem {
    const actor = checkActor(meta.actor);
    if (typeof checked !== "boolean") invalid("checked must be true or false");
    if (!Number.isInteger(expectedVersion)) invalid("expected_version is required: the item's version as you read it (0 for an item never written)");
    const out = this.write(() => {
      this.openForReview(name);
      if (!this.reviewKeys(name).check.has(key)) invalid(`${key} is not a current checklist step of milestone ${name}`);
      return this.writeReviewItem(name, key, expectedVersion, checked ? { checked_at: now(), checked_by: meta.by ?? actor } : { checked_at: null, checked_by: null });
    });
    this.emit("nudge", { milestone: name, key });
    return out;
  }

  /** Save one feedback draft (the milestone, a card, or a checklist step). Not a board event either — submitting is. */
  saveReviewDraft(name: string, key: string, text: string, expectedVersion: number, meta: WriteMeta): ReviewItem {
    checkActor(meta.actor);
    checkText(text, "text");
    if (text.length > 20000) invalid("feedback is at most 20000 characters");
    if (!Number.isInteger(expectedVersion)) invalid("expected_version is required: the item's version as you read it (0 for an item never written)");
    const out = this.write(() => {
      this.openForReview(name);
      // A comment whose step has since been reworded away can still be cleared.
      const clearing = text === "" && this.reviewItems(name).has(key);
      if (!clearing && !this.reviewKeys(name).comment.has(key)) invalid(`${key} is not a review item of milestone ${name}`);
      return this.writeReviewItem(name, key, expectedVersion, { draft: text });
    });
    this.emit("nudge", { milestone: name, key });
    return out;
  }

  /**
   * Submit the review: every non-empty feedback draft becomes a new card on
   * the same milestone at its source's iteration + 1, linked back to the item
   * it came from; the drafts are cleared; one `review_submitted` event names
   * the new cards (none, when the user had no feedback). `expected` is the
   * version of every draft the user is submitting, as they saw it — a draft
   * changed or added elsewhere refuses the whole submission.
   */
  submitReview(name: string, expected: Record<string, number>, meta: WriteMeta): { milestone: Milestone; cards: Card[] } {
    const actor = checkActor(meta.actor);
    if (!isObj(expected) || !Object.values(expected).every((v) => Number.isInteger(v))) {
      invalid("expected must map each submitted draft's key to the version you read");
    }
    return this.write(() => {
      const m = this.milestoneRow(name);
      const doc = this.openForReview(name);
      const items = this.reviewItems(name);
      const drafts = [...items.values()].filter((i) => i.draft.trim());
      const stale = [
        ...drafts.filter((d) => expected[d.key] !== d.version).map((d) => d.key),
        ...Object.entries(expected)
          .filter(([k, v]) => (items.get(k)?.version ?? 0) !== v)
          .map(([k]) => k),
      ];
      if (stale.length) {
        throw new BoardError(
          "stale_version",
          `feedback changed since you read it: ${[...new Set(stale)].join(", ")} — reload the review page and submit again`,
          undefined,
          { drafts: Object.fromEntries(drafts.map((d) => [d.key, { text: d.draft, version: d.version }])) },
        );
      }
      const { check } = this.reviewKeys(name);
      const cardsOn = this.milestoneCardRows(name).map((r) => JSON.parse(r.doc) as Record<string, unknown>);
      const top = Math.max(1, ...cardsOn.map((c) => (Number.isInteger(c.iteration) ? (c.iteration as number) : 1)));
      const by = meta.by ?? actor;
      const t = now();
      const created: Card[] = [];
      for (const d of drafts) {
        // A step reworded since its comment was written still names its card in its key.
        const step = check.get(d.key);
        const srcId = d.key === "milestone" ? null : d.key.split(":")[1];
        const src = srcId ? cardsOn.find((c) => c.id === srcId) : undefined;
        if (d.key !== "milestone" && !src) invalid(`${d.key} no longer names an item on milestone ${name}: clear that comment and submit again`);
        const iteration = src ? (Number.isInteger(src.iteration) ? (src.iteration as number) : 1) + 1 : top + 1;
        const what = src ? `${String(src.id)}: ${String(src.title)}` : `milestone ${name}`;
        const first = d.draft.trim().split(/\r?\n/)[0];
        const title = `Review feedback on ${src ? String(src.id) : `milestone ${name}`}: ${first.length > 80 ? `${first.slice(0, 79)}…` : first}`;
        const body =
          `User feedback from the milestone ${name} review (${t.slice(0, 10)}), on ${what}` +
          (step ? `, checklist step "${step.text}"` : "") +
          `:\n\n${d.draft.trim()}\n`;
        const srcPr = src && isObj(src.pr) && typeof src.pr.url === "string" ? [{ label: `${String(src.id)} PR #${String(src.pr.number)} (feedback source)`, url: src.pr.url }] : [];
        const card = this.insertCard(
          {
            title,
            body,
            column: "backlog",
            role: "engineer",
            priority: "P2",
            links: srcPr,
            pr: null,
            worktree: null,
            question: null,
            agent: null,
            activity: null,
            created: t,
            updated: t,
            milestone: name,
            iteration,
            source: { milestone: name, key: d.key, card: srcId },
          },
          by,
          actor,
          `Filed from the milestone ${name} review: feedback on ${src ? String(src.id) : "the milestone"}`,
        );
        created.push(card);
        if (src) {
          const sr = this.row(String(src.id));
          this.commitDoc(sr.id, sr.version, JSON.parse(sr.doc));
          this.appendActivityRow(sr.id, { t, by, msg: `Milestone ${name} review feedback filed as ${card.id} (iteration ${iteration})` });
        }
        this.writeReviewItem(name, d.key, d.version, { draft: "" });
      }
      doc.submissions = [...(Array.isArray(doc.submissions) ? doc.submissions : []), { t, by, cards: created.map((c) => c.id) }];
      this.commitMilestone(name, m.version, doc);
      this.recordEvent(actor, "review_submitted", null, { milestone: name, cards: created.map((c) => c.id) });
      return { milestone: this.milestoneOf(this.milestoneRow(name)), cards: created };
    });
  }

  /**
   * The milestone review page's whole payload, assembled from the cards'
   * structured fields — nothing on it is hand-written for the page.
   */
  getMilestone(name: string): Record<string, unknown> {
    const m = this.milestoneOf(this.milestoneRow(name));
    const cards = this.milestoneCardRows(name).map((r) => Object.assign(JSON.parse(r.doc) as Record<string, unknown>, { version: r.version }));
    const items = this.reviewItems(name);
    const finished = cards.filter((c) => FINISHED_COLUMNS.includes(c.column as string));
    const brief = (c: Record<string, unknown>) =>
      pick(c, ["id", "title", "column", "role", "pr", "merge_sha", "merged_at", "summary", "area", "iteration", "worktree", "source", "version"]);
    const areaOrder = [...AREAS, null];
    const byArea = (list: Record<string, unknown>[]) =>
      areaOrder
        .map((area) => ({ area, cards: list.filter((c) => (area === null ? !(AREAS as readonly unknown[]).includes(c.area) : c.area === area)) }))
        .filter((g) => g.cards.length);

    const checkGroups = byArea(finished.filter((c) => checklistSteps(String(c.id), c.human_testing).length)).map((g) => ({
      area: g.area,
      cards: g.cards.map((c) => ({
        id: c.id,
        title: c.title,
        steps: checklistSteps(String(c.id), c.human_testing).map((s) => {
          const it = items.get(`check:${s.key}`);
          return { key: `check:${s.key}`, text: s.text, checked: !!it?.checked_at, checked_at: it?.checked_at ?? null, checked_by: it?.checked_by ?? null, version: it?.version ?? 0 };
        }),
      })),
    }));
    const merged = finished.filter((c) => typeof c.merge_sha === "string" && typeof c.merged_at === "string");
    // Timestamps are to the second; of two merges in one second the later-filed card wins.
    const newest = merged.reduce<Record<string, unknown> | null>((best, c) => (!best || String(c.merged_at) >= String(best.merged_at) ? c : best), null);
    const idNum = (id: unknown) => Number(/(\d+)$/.exec(String(id))?.[1] ?? NaN);
    const firstId = Number.isInteger(m.first_id) ? (m.first_id as number) : Infinity;
    const endId = Number.isInteger(m.end_id) ? (m.end_id as number) : Infinity;
    const followups = (this.db.prepare("SELECT id, version, doc FROM cards WHERE deleted_at IS NULL ORDER BY rowid").all() as CardRow[])
      .filter((r) => idNum(r.id) >= firstId && idNum(r.id) < endId)
      .map((r) => JSON.parse(r.doc) as Record<string, unknown>)
      .filter((c) => c.milestone !== name)
      .map((c) => pick(c, ["id", "title", "column", "role", "milestone", "created"]));

    return {
      milestone: m,
      what_changed: byArea(finished).map((g) => ({ area: g.area, cards: g.cards.map(brief) })),
      in_flight: cards.filter((c) => !FINISHED_COLUMNS.includes(c.column as string)).map(brief),
      checklist: {
        applies_to: {
          tag: m.tag ?? null,
          review_sha: m.review_sha ?? null,
          newest_merge: newest ? { sha: newest.merge_sha, card: newest.id, at: newest.merged_at } : null,
        },
        groups: checkGroups,
        // No steps and no statement is a gap; an explicit "nothing needs human testing" is an answer.
        without_steps: finished
          .filter((c) => c.role !== "pm" && !checklistSteps(String(c.id), c.human_testing).length && !saysNothingToCheck(c.human_testing))
          .map((c) => pick(c, ["id", "title"])),
        nothing_to_check: finished
          .filter((c) => !checklistSteps(String(c.id), c.human_testing).length && saysNothingToCheck(c.human_testing))
          .map((c) => pick(c, ["id", "title"])),
      },
      // Oldest first; rulings in the same second keep card order, then recording order (a stable sort).
      decisions: cards
        .flatMap((c) => (Array.isArray(c.rulings) ? (c.rulings as Ruling[]) : []).map((r) => ({ card: c.id, title: c.title, ...r })))
        .sort((a, b) => a.t.localeCompare(b.t)),
      balance: {
        sweep: m.sweep ?? null,
        deferred: cards.filter((c) => isObj(c.sweep) && c.sweep.status === "deferred-to-milestone").map((c) => ({ ...pick(c, ["id", "title"]), note: (c.sweep as { summary?: string }).summary ?? null })),
        on_card: cards.filter((c) => isObj(c.sweep) && c.sweep.status === "done-on-card").map((c) => ({ ...pick(c, ["id", "title"]), note: (c.sweep as { summary?: string }).summary ?? null })),
        unstated: finished.filter((c) => c.role !== "pm" && !isObj(c.sweep)).map((c) => pick(c, ["id", "title"])),
      },
      gaps: {
        stated: cards.filter((c) => typeof c.gaps === "string" && c.gaps.trim()).map((c) => ({ id: c.id, title: c.title, gaps: c.gaps })),
        followups,
      },
      feedback: {
        drafts: Object.fromEntries([...items.values()].filter((i) => i.draft).map((i) => [i.key, { text: i.draft, version: i.version }])),
        versions: Object.fromEntries([...items.values()].map((i) => [i.key, i.version])),
        submissions: m.submissions ?? [],
      },
    };
  }

  // ---------------------------------------------------------------- import / export

  isEmpty(): boolean {
    const n = (this.db.prepare("SELECT (SELECT count(*) FROM cards) + (SELECT count(*) FROM activity) + (SELECT count(*) FROM milestones) AS n").get() as { n: number }).n;
    return n === 0;
  }


  /**
   * Load a board state into an EMPTY database, preserving ids, card order,
   * every field (legacy shapes included), activity and timestamps exactly.
   * Refuses a non-empty database.
   *
   * `{schema: 2, nextId, cards, milestones}` is this build's own export.
   * `{schema: 1, nextId, cards}` — an AS-153 export or the artifact's state —
   * is migrated on the way in: each card gains `pr` and `worktree` (see
   * derivePr), and a `human_review` card becomes a `merged` one, exactly as
   * `migrate` does it to a database.
   */
  importState(
    state: unknown,
    meta: WriteMeta,
  ): {
    cards: number;
    activity: number;
    nextId: number;
    milestones: number;
    migration: {
      pr_derived: number;
      pr_null: number;
      pr_kept: number;
      ambiguous: { id: string; candidates: number[] }[];
      constructed_url: string[];
      /** Non-pm cards still in flight (review, merged/human_review, unreleased done) left with no pr: act on these. */
      live_without_pr: string[];
      multi_pr_handoffs: { id: string; t: string; numbers: number[] }[];
      /** human_review cards now in merged with no merge recorded: mark_merged each once its PR is merged. */
      human_review_to_merged: string[];
    };
  } {
    const actor = checkActor(meta.actor);
    if (!isObj(state)) invalid("state must be an object");
    if (state.schema !== 1 && state.schema !== 2) invalid(`unsupported schema ${String(state.schema)} (expected 1 or 2)`);
    const legacy = state.schema === 1;
    if (!Number.isInteger(state.nextId)) invalid("nextId must be an integer");
    if (!Array.isArray(state.cards)) invalid("cards must be an array");
    const milestones = legacy ? [] : state.milestones;
    if (!Array.isArray(milestones)) invalid("a schema 2 state carries milestones: an array");
    const names = new Set<string>();
    (milestones as unknown[]).forEach((m, i) => {
      if (!isObj(m)) invalid(`milestones[${i}] is not an object`);
      checkMilestoneName(m.name);
      if (names.has(m.name as string)) invalid(`duplicate milestone ${String(m.name)}`);
      names.add(m.name as string);
      if (!(MILESTONE_STATUSES as readonly unknown[]).includes(m.status)) invalid(`milestone ${String(m.name)}: status must be one of ${MILESTONE_STATUSES.join(", ")}`);
      if (!Array.isArray(m.review)) invalid(`milestone ${String(m.name)}: review must be an array`);
      (m.review as unknown[]).forEach((it, j) => {
        if (!isObj(it) || typeof it.key !== "string" || typeof it.draft !== "string") invalid(`milestone ${String(m.name)}.review[${j}] must be {key, checked_at, checked_by, draft}`);
      });
    });
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
          c.pr = checkPr(c.pr); // stored normalised: {number, url}
        } catch {
          invalid(`${String(c.id)}: pr must be null or {number, url} with a pull-request url`);
        }
      }
      if ("worktree" in c) checkWorktree(c.worktree);
      if (!(legacy && c.column === "human_review")) checkColumn(c.column);
      if (c.milestone != null && !names.has(c.milestone as string)) invalid(`${String(c.id)}: milestone ${String(c.milestone)} is not in the state's milestones`);
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
    const derived = derivePr(toDerive, cards as Record<string, unknown>[]);
    const migration = {
      pr_derived: Object.values(derived.pr).filter((p) => p !== null).length,
      pr_null: Object.values(derived.pr).filter((p) => p === null).length,
      pr_kept: cards.length - toDerive.length,
      ambiguous: derived.ambiguous,
      constructed_url: derived.constructed,
      live_without_pr: (cards as Record<string, unknown>[])
        .filter((c) => {
          const pr = "pr" in c ? c.pr : derived.pr[String(c.id)];
          const live = c.column === "review" || c.column === "human_review" || c.column === "merged" || (c.column === "done" && !c.released);
          return live && c.role !== "pm" && pr == null;
        })
        .map((c) => String(c.id)),
      multi_pr_handoffs: derived.multi,
      human_review_to_merged: (cards as Record<string, unknown>[]).filter((c) => legacy && c.column === "human_review").map((c) => String(c.id)),
    };
    return this.write(() => {
      if (!this.isEmpty()) throw new BoardError("not_empty", "import only loads into an EMPTY database; this one already has cards");
      for (const m of milestones as Record<string, unknown>[]) {
        const { review, ...doc } = m;
        this.db.prepare("INSERT INTO milestones(name, doc) VALUES (?, ?)").run(m.name, JSON.stringify(doc));
        for (const it of review as ReviewItem[]) {
          this.db
            .prepare("INSERT INTO review_items(milestone, key, checked_at, checked_by, draft) VALUES (?, ?, ?, ?, ?)")
            .run(m.name, it.key, it.checked_at ?? null, it.checked_by ?? null, it.draft);
        }
      }
      let acts = 0;
      const ins = this.db.prepare("INSERT INTO cards(id, doc) VALUES (?, ?)");
      for (const c of cards as Record<string, unknown>[]) {
        const doc: Record<string, unknown> = { ...c, activity: null };
        if (!("pr" in c)) doc.pr = derived.pr[String(c.id)];
        if (!("worktree" in c)) doc.worktree = null;
        const fromReview = legacy && c.column === "human_review";
        if (fromReview) Object.assign(doc, { column: "merged", updated: now() });
        ins.run(c.id, JSON.stringify(doc));
        for (const a of c.activity as ActivityEntry[]) {
          this.appendActivityRow(c.id as string, a);
          acts++;
        }
        if (fromReview) {
          this.appendActivityRow(c.id as string, { t: now(), by: "migrate", msg: HUMAN_REVIEW_MIGRATION_NOTE });
          acts++;
        }
      }
      this.db.prepare("UPDATE meta SET value = ? WHERE key = 'next_id'").run(String(state.nextId));
      if (prefix !== null) this.db.prepare("UPDATE meta SET value = ? WHERE key = 'id_prefix'").run(prefix);
      this.recordEvent(actor, "imported", null, { cards: cards.length, activity: acts, milestones: (milestones as unknown[]).length });
      return { cards: cards.length, activity: acts, nextId: state.nextId as number, milestones: (milestones as unknown[]).length, migration };
    });
  }

  /**
   * The full state: the backup and restore path. A schema 2 board exports
   * `{schema: 2, nextId, cards, milestones}` (each milestone with its review
   * ticks and drafts); a database not yet migrated exports exactly the
   * `{schema: 1, nextId, cards}` it always did, so the pre-cutover backup is
   * the same file whichever build writes it.
   */
  exportState(): { schema: number; nextId: number; cards: Card[]; milestones?: Record<string, unknown>[] } {
    const rows = this.db
      .prepare("SELECT id, version, doc FROM cards WHERE deleted_at IS NULL ORDER BY rowid")
      .all() as CardRow[];
    const cards = rows.map((r) => {
      const doc = JSON.parse(r.doc) as Card;
      if ("activity" in doc) doc.activity = this.activity(r.id);
      return doc;
    });
    const nextId = Number(this.meta("next_id"));
    if (this.schema < 2) return { schema: this.schema, nextId, cards };
    const milestones = (this.db.prepare("SELECT name, version, doc FROM milestones ORDER BY rowid").all() as MilestoneRow[]).map((r) => ({
      ...(JSON.parse(r.doc) as Record<string, unknown>),
      review: [...this.reviewItems(r.name).values()].map(({ key, checked_at, checked_by, draft }) => ({ key, checked_at, checked_by, draft })),
    }));
    return { schema: 2, nextId, cards, milestones };
  }

  /**
   * Bring a schema 1 database to schema 2, in place, with the daemon
   * stopped. First a consistent copy of the whole file is written to
   * `backup` (SQLite's online backup) — restoring that file IS the rollback,
   * board id and event cursors included. Then, in one transaction: the
   * milestone tables are created, every `human_review` card becomes a
   * `merged` card with no milestone and no merge recorded (Tester-approved,
   * so the new flow's next step is the orchestrator's merge and mark_merged),
   * each gains one activity line, one `migrated` event is recorded, and the
   * schema is set to 2. Every other card, activity row and event is left
   * byte-for-byte as it was. Running it on a schema 2 database does nothing.
   */
  static async migrate(
    path: string,
    opts: { backup?: string; actor?: string } = {},
  ): Promise<{ from: number; to: number; backup: string | null; human_review_to_merged: string[]; cards: number }> {
    const b = new Board(path, { allowOld: true });
    try {
      const from = b.schema;
      const cards = (b.db.prepare("SELECT count(*) AS n FROM cards").get() as { n: number }).n;
      if (from === SCHEMA_VERSION) return { from, to: SCHEMA_VERSION, backup: null, human_review_to_merged: [], cards };
      const backup = opts.backup ?? `${path}.schema${from}-${now().replace(/:/g, "")}.bak`;
      if (existsSync(backup)) invalid(`backup file ${backup} already exists; name another with --backup`);
      await b.db.backup(backup);
      b.db.pragma("journal_mode = WAL");
      b.db.pragma("foreign_keys = ON");
      const actor = opts.actor ?? "migrate";
      const moved = b.write(() => {
        if (storedSchema(b.db) !== from) throw new BoardError("needs_migration", `${path} changed schema during the migration; re-run it`);
        b.db.exec(SCHEMA);
        const rows = b.db.prepare("SELECT id, version, doc FROM cards WHERE col = 'human_review' ORDER BY rowid").all() as CardRow[];
        const t = now();
        for (const r of rows) {
          const doc = JSON.parse(r.doc) as Record<string, unknown>;
          doc.column = "merged";
          doc.updated = t;
          const info = b.db.prepare("UPDATE cards SET doc = ?, version = version + 1 WHERE id = ? AND version = ?").run(JSON.stringify(doc), r.id, r.version);
          if (info.changes !== 1) throw new BoardError("stale_version", `${r.id} changed during the migration; re-run it`);
          b.appendActivityRow(r.id, { t, by: actor, msg: HUMAN_REVIEW_MIGRATION_NOTE });
        }
        b.db.prepare("UPDATE meta SET value = ? WHERE key = 'schema'").run(String(SCHEMA_VERSION));
        const ids = rows.map((r) => r.id);
        b.recordEvent(actor, "migrated", null, { from, to: SCHEMA_VERSION, human_review_to_merged: ids });
        return ids;
      });
      return { from, to: SCHEMA_VERSION, backup, human_review_to_merged: moved, cards };
    } finally {
      b.close();
    }
  }
}
