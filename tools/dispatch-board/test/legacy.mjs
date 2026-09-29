// A SCHEMA 1 database, as the AS-153 board wrote it — the database the
// cutover migrates. The DDL below is that build's SCHEMA, verbatim and frozen
// (it must never follow src/board.ts), and `legacyDb` loads a state the way
// its import did: one doc per card with `activity: null`, activity rows in
// order, meta, and an `imported` event. The cutover rehearsal checked this
// against a database written by the AS-153 build itself (identical
// sqlite_master and meta keys).
import Database from "better-sqlite3";
import { randomUUID } from "node:crypto";

export const LEGACY_SCHEMA_1 = `
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

/**
 * Write `state` ({schema: 1, nextId, cards}, cards carrying pr/worktree) into
 * a fresh schema 1 database at `path`, plus `extraEvents` after the import
 * event (a live board's history). Returns the board id it minted.
 */
export function legacyDb(path, state, extraEvents = []) {
  const db = new Database(path);
  db.pragma("journal_mode = WAL");
  db.pragma("foreign_keys = ON");
  db.exec(LEGACY_SCHEMA_1);
  const boardId = randomUUID();
  db.prepare("INSERT OR IGNORE INTO meta(key, value) VALUES ('schema', '1'), ('next_id', '1'), ('id_prefix', 'AS-'), ('board_id', ?)").run(boardId);
  db.transaction(() => {
    let acts = 0;
    for (const c of state.cards) {
      db.prepare("INSERT INTO cards(id, doc) VALUES (?, ?)").run(c.id, JSON.stringify({ ...c, activity: null }));
      c.activity.forEach((a, i) => {
        db.prepare("INSERT INTO activity(card_id, seq, t, by, msg) VALUES (?, ?, ?, ?, ?)").run(c.id, i + 1, a.t, a.by, a.msg);
        acts++;
      });
    }
    db.prepare("UPDATE meta SET value = ? WHERE key = 'next_id'").run(String(state.nextId));
    db.prepare("INSERT INTO events(t, actor, kind, card_id, data) VALUES (?, ?, ?, ?, ?)").run(
      "2026-09-26T00:00:00",
      "import",
      "imported",
      null,
      JSON.stringify({ cards: state.cards.length, activity: acts }),
    );
    for (const e of extraEvents) {
      db.prepare("INSERT INTO events(t, actor, kind, card_id, data) VALUES (?, ?, ?, ?, ?)").run(e.t, e.actor, e.kind, e.card ?? null, JSON.stringify(e.data ?? {}));
    }
  })();
  db.close();
  return boardId;
}

/** Everything in a database, raw, for byte-for-byte before/after comparison. */
export function dumpDb(path) {
  const db = new Database(path, { readonly: true });
  try {
    return {
      master: db.prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name").all(),
      meta: Object.fromEntries(db.prepare("SELECT key, value FROM meta").all().map((r) => [r.key, r.value])),
      cards: db.prepare("SELECT id, version, doc, deleted_at FROM cards ORDER BY rowid").all(),
      activity: db.prepare("SELECT card_id, seq, t, by, msg FROM activity ORDER BY card_id, seq").all(),
      events: db.prepare("SELECT cursor, t, actor, kind, card_id, data FROM events ORDER BY cursor").all(),
    };
  } finally {
    db.close();
  }
}
