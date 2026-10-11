/**
 * Launching a review guide check's replay in the game client (AS-237).
 *
 * The review page's ▶ button names a milestone and a guide check key; the
 * daemon looks the check's stored replay config up on the board (never from
 * the request), writes it to a file under the board's data directory, and
 * starts the game binary detached with argv exactly `[binary, "--replay",
 * <file>]`. It never runs anything but that one binary, and never `cargo run`:
 * a missing binary is an error that says to build it, because a silent
 * ten-minute build behind a button reads as a broken button.
 *
 * The spawn is injectable, so the tests never launch the game.
 */
import { spawn } from "node:child_process";
import { accessSync, closeSync, constants, existsSync, mkdirSync, openSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { randomBytes } from "node:crypto";
import { join, resolve } from "node:path";
import { mainCheckoutRoot, repoRoot } from "./paths.js";

/** What a launch needs from a process: its pid, its exit, its spawn error, and to be let go. */
export interface GameProcess {
  pid?: number;
  on(event: "exit", listener: (code: number | null, signal: NodeJS.Signals | null) => void): unknown;
  on(event: "error", listener: (err: Error) => void): unknown;
  unref(): void;
}

/** Start `cmd` with `args`, its stderr appended to `stderrPath`, detached from the daemon. */
export type SpawnGame = (cmd: string, args: string[], opts: { cwd: string; stderrPath: string }) => GameProcess;

/**
 * The real spawn: a new process group (so the game outlives a daemon
 * restart), stdin/stdout ignored, stderr to a file rather than a pipe — a
 * pipe closed once the daemon stops listening would fail the game's next
 * write to stderr.
 */
export const spawnDetached: SpawnGame = (cmd, args, { cwd, stderrPath }) => {
  const fd = openSync(stderrPath, "a");
  try {
    return spawn(cmd, args, { cwd, detached: true, stdio: ["ignore", "ignore", fd] });
  } finally {
    closeSync(fd);
  }
};

/** The env var naming the game binary; the daemon's `--game-bin` flag wins over it. */
export const GAME_BIN_ENV = "DISPATCH_BOARD_GAME_BIN";

/**
 * Which game binary to launch: the `--game-bin` flag, else
 * `$DISPATCH_BOARD_GAME_BIN`, else `<main checkout>/target/release/arenasim`
 * — the main checkout resolved the way the board resolves its database.
 * `checkout` is set only for the default, where the binary's age against that
 * checkout's HEAD means something.
 */
export function resolveGameBinary(flag?: string): { binary: string; checkout: string | null } {
  if (flag) return { binary: resolve(flag), checkout: null };
  if (process.env[GAME_BIN_ENV]) return { binary: resolve(process.env[GAME_BIN_ENV] as string), checkout: null };
  const main = mainCheckoutRoot(repoRoot());
  return { binary: join(main, "target", "release", "arenasim"), checkout: main };
}

/**
 * When the checkout's HEAD last moved, read from git's own files (no git
 * subprocess): the later of HEAD's and its branch ref's modification times.
 * Null when it cannot be read — a packed ref with no loose file, or no git.
 */
export function headMovedAt(checkout: string): number | null {
  try {
    const dotGit = join(checkout, ".git");
    let gitdir = dotGit;
    let common = dotGit;
    if (statSync(dotGit).isFile()) {
      const m = /^gitdir:\s*(.+)$/m.exec(readFileSync(dotGit, "utf8"));
      if (!m) return null;
      gitdir = resolve(checkout, m[1].trim());
      const cd = join(gitdir, "commondir");
      common = existsSync(cd) ? resolve(gitdir, readFileSync(cd, "utf8").trim()) : gitdir;
    }
    const headFile = join(gitdir, "HEAD");
    let t = statSync(headFile).mtimeMs;
    const ref = /^ref:\s*(\S+)/.exec(readFileSync(headFile, "utf8"));
    if (ref) {
      for (const base of [gitdir, common]) {
        const f = join(base, ref[1]);
        if (existsSync(f)) {
          t = Math.max(t, statSync(f).mtimeMs);
          break;
        }
      }
    }
    return t;
  } catch {
    return null;
  }
}

export interface ReplayOptions {
  /** The game binary (see resolveGameBinary). */
  binary: string;
  /** The checkout whose HEAD the binary's age is compared against; null skips the check. */
  checkout?: string | null;
  /** The board's data directory: replay files go in `<dataDir>/replays/`. */
  dataDir: string;
  /** The game's working directory; default: the main checkout. */
  cwd?: string;
  spawn?: SpawnGame;
  now?: () => number;
  /** A process that exits within this many ms is reported as a failed launch. */
  earlyExitMs?: number;
  /** A second launch of the same check while one is starting, or within this many ms of its "launched", is ignored. */
  rateLimitMs?: number;
}

export type ReplayResult =
  | { status: "launched"; message: string; pid: number | null; file: string; stale: boolean }
  | { status: "exited"; message: string; code: number | null; signal: string | null; stderr_tail: string; file: string }
  | { status: "failed"; message: string; file: string };

/** A launch refused before anything ran: the HTTP status and the error code to answer with. */
export class ReplayRefused extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message);
  }
}

const STDERR_TAIL = 2000;
/** The rate limit's mark for a check whose launch has not been answered yet. */
const STARTING = Number.POSITIVE_INFINITY;
const KEEP_FILES_MS = 24 * 3600 * 1000;

export class ReplayLauncher {
  private last = new Map<string, number>();
  private readonly spawnGame: SpawnGame;
  private readonly now: () => number;
  private readonly earlyExitMs: number;
  private readonly rateLimitMs: number;

  constructor(private opts: ReplayOptions) {
    this.spawnGame = opts.spawn ?? spawnDetached;
    this.now = opts.now ?? Date.now;
    this.earlyExitMs = opts.earlyExitMs ?? 2000;
    this.rateLimitMs = opts.rateLimitMs ?? 2000;
  }

  get binary(): string {
    return this.opts.binary;
  }

  /** The binary's problem, if it has one: missing, or not an executable file. */
  private binaryProblem(): string | null {
    const b = this.opts.binary;
    try {
      if (!statSync(b).isFile()) return `game binary not found at ${b} (not a file) — build first: cargo build --release`;
    } catch {
      return `game binary not found at ${b} — build first: cargo build --release`;
    }
    try {
      accessSync(b, constants.X_OK);
    } catch {
      return `game binary at ${b} is not executable — build first: cargo build --release`;
    }
    return null;
  }

  /** Whether the binary predates the checkout's HEAD — it may not hold the code under review. */
  private isStale(): boolean {
    if (!this.opts.checkout) return false;
    const moved = headMovedAt(this.opts.checkout);
    return moved !== null && statSync(this.opts.binary).mtimeMs < moved;
  }

  /**
   * Launch `config` (a guide check's stored replay config) for `milestone`'s
   * check `key`. Resolves once the launch has either survived `earlyExitMs`
   * or died inside it.
   */
  async launch(milestone: string, key: string, config: Record<string, unknown>): Promise<ReplayResult> {
    const problem = this.binaryProblem();
    if (problem) throw new ReplayRefused(503, "binary_not_found", problem);
    const rk = `${milestone}\u0000${key}`;
    const t = this.now();
    const prev = this.last.get(rk);
    if (prev === STARTING) throw new ReplayRefused(429, "rate_limited", "this replay is already starting — ignored");
    if (prev !== undefined && t - prev < this.rateLimitMs) {
      throw new ReplayRefused(429, "rate_limited", `this replay was launched ${((t - prev) / 1000).toFixed(1)}s ago — ignored`);
    }
    this.last.set(rk, STARTING);
    let result: ReplayResult;
    try {
      result = await this.start(config, t);
    } catch (e) {
      this.last.delete(rk);
      throw e;
    }
    // The window runs from the moment the launch is reported, not from the
    // spawn: the answer itself takes earlyExitMs, so a click on "launched" is
    // a double click. A launch that failed frees the check for a retry at once.
    if (result.status === "launched") this.last.set(rk, this.now());
    else this.last.delete(rk);
    return result;
  }

  /** Write the replay file, spawn the game, and watch it for earlyExitMs. */
  private async start(config: Record<string, unknown>, t: number): Promise<ReplayResult> {
    const dir = join(this.opts.dataDir, "replays");
    mkdirSync(dir, { recursive: true });
    this.prune(dir, t);
    // The file name carries nothing from the request: no path a milestone or key could steer.
    const stem = join(dir, `replay-${t}-${randomBytes(4).toString("hex")}`);
    const file = `${stem}.json`;
    const stderrPath = `${stem}.stderr.log`;
    writeFileSync(file, `${JSON.stringify(config, null, 2)}\n`);
    const stale = this.isStale();

    let child: GameProcess;
    try {
      child = this.spawnGame(this.opts.binary, ["--replay", file], { cwd: this.opts.cwd ?? mainCheckoutRoot(repoRoot()), stderrPath });
    } catch (e) {
      return { status: "failed", message: `could not start the game: ${(e as Error).message}`, file };
    }

    const outcome = await new Promise<{ kind: "alive" } | { kind: "exit"; code: number | null; signal: string | null } | { kind: "error"; err: Error }>(
      (res) => {
        const timer = setTimeout(() => res({ kind: "alive" }), this.earlyExitMs);
        child.on("exit", (code, signal) => {
          clearTimeout(timer);
          res({ kind: "exit", code, signal });
        });
        child.on("error", (err) => {
          clearTimeout(timer);
          res({ kind: "error", err });
        });
      },
    );
    child.unref();

    if (outcome.kind === "error") return { status: "failed", message: `could not start the game: ${outcome.err.message}`, file };
    if (outcome.kind === "exit") {
      let tail = "";
      try {
        tail = readFileSync(stderrPath, "utf8").slice(-STDERR_TAIL).trim();
      } catch {
        /* nothing written */
      }
      const how = outcome.code !== null ? `exited with code ${outcome.code}` : `was killed by ${outcome.signal}`;
      return { status: "exited", message: `the game ${how}`, code: outcome.code, signal: outcome.signal, stderr_tail: tail, file };
    }
    return {
      status: "launched",
      message: stale ? "launched — but the binary is older than the checkout's HEAD; rebuild (cargo build --release) to replay current code" : "launched",
      pid: child.pid ?? null,
      file,
      stale,
    };
  }

  /** Drop replay files over a day old: each launch writes two, and nothing else reads them after startup. */
  private prune(dir: string, t: number): void {
    for (const f of readdirSync(dir)) {
      if (!f.startsWith("replay-")) continue;
      const p = join(dir, f);
      try {
        if (t - statSync(p).mtimeMs > KEEP_FILES_MS) rmSync(p, { force: true });
      } catch {
        /* raced with another prune */
      }
    }
  }
}
