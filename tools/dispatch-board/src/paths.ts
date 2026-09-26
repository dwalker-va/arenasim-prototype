/**
 * Where the board lives. One DB for every worktree: by default the MAIN
 * checkout's gitignored `.dispatch/board.db`, found by reading git's own files
 * (no git subprocess), so a daemon started from any worktree opens the same
 * board. `DISPATCH_BOARD_DB` overrides it; `DISPATCH_BOARD_PORT` the port.
 */
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const DEFAULT_PORT = 7453;

/** tools/dispatch-board/ — this file compiles to tools/dispatch-board/dist/paths.js. */
export function packageDir(): string {
  return resolve(dirname(fileURLToPath(import.meta.url)), "..");
}

/** The checkout this daemon runs from (tools/dispatch-board/../..). */
export function repoRoot(): string {
  return resolve(packageDir(), "..", "..");
}

/**
 * The game's icon, as `packaging/generate_icon.py` emits it — the single
 * source. The board serves these files in place (never a copy), so a
 * regenerated icon reaches the browser tab with no change here.
 */
export function packagingIconDir(): string {
  // DISPATCH_BOARD_PACKAGING_DIR: for a copy of this package that is not in
  // its checkout (the mutation harness's copies) — never needed in normal use.
  if (process.env.DISPATCH_BOARD_PACKAGING_DIR) return resolve(process.env.DISPATCH_BOARD_PACKAGING_DIR);
  return join(repoRoot(), "packaging");
}

/** The main checkout's root, from any worktree of it. */
export function mainCheckoutRoot(fromRepoRoot: string): string {
  const dotGit = join(fromRepoRoot, ".git");
  if (!existsSync(dotGit)) return fromRepoRoot;
  if (statSync(dotGit).isDirectory()) return fromRepoRoot;
  // A worktree: `.git` is a file "gitdir: <main>/.git/worktrees/<name>", and
  // that gitdir's `commondir` points (relatively) at <main>/.git.
  const m = /^gitdir:\s*(.+)$/m.exec(readFileSync(dotGit, "utf8"));
  if (!m) return fromRepoRoot;
  const gitdir = resolve(fromRepoRoot, m[1].trim());
  const commondirFile = join(gitdir, "commondir");
  const common = existsSync(commondirFile) ? resolve(gitdir, readFileSync(commondirFile, "utf8").trim()) : resolve(gitdir, "..", "..");
  return dirname(common);
}

export function defaultDbPath(): string {
  if (process.env.DISPATCH_BOARD_DB) return resolve(process.env.DISPATCH_BOARD_DB);
  return join(mainCheckoutRoot(repoRoot()), ".dispatch", "board.db");
}

export function defaultPort(): number {
  const p = Number(process.env.DISPATCH_BOARD_PORT);
  return Number.isInteger(p) && p > 0 ? p : DEFAULT_PORT;
}

/** The daemon's lock file sits beside the DB: `<db>.daemon.json`. */
export function lockPath(db: string): string {
  return `${db}.daemon.json`;
}

export interface LockInfo {
  pid: number;
  port: number;
  started: string;
}

/** The live daemon holding this DB, or null (no lock, or its process is gone). */
export function liveDaemon(db: string): LockInfo | null {
  const p = lockPath(db);
  if (!existsSync(p)) return null;
  let info: LockInfo;
  try {
    info = JSON.parse(readFileSync(p, "utf8")) as LockInfo;
  } catch {
    return null;
  }
  try {
    process.kill(info.pid, 0);
    return info;
  } catch (e) {
    // EPERM: the process exists but is not ours — still alive.
    return (e as NodeJS.ErrnoException).code === "EPERM" ? info : null;
  }
}
