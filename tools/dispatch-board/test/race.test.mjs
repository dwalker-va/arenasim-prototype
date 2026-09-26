// A TRUE concurrent claim race: N threads, N SQLite connections on one file,
// released together by a barrier, all claiming the same card. Exactly one may
// win each round; every loser must be refused (never an error, never a win).
import { test } from "node:test";
import assert from "node:assert/strict";
import { Worker } from "node:worker_threads";
import { Board } from "../dist/board.js";
import { tempDir } from "./helpers.mjs";

const CONTENDERS = 8;
const ROUNDS = 25;

test(`concurrent claims: ${CONTENDERS} connections x ${ROUNDS} cards, exactly one winner per card`, async (t) => {
  const tmp = tempDir();
  t.after(tmp.cleanup);
  const setup = new Board(tmp.db);
  const cardIds = [];
  for (let i = 0; i < ROUNDS; i++) {
    const c = setup.createCard({ title: `race ${i}`, role: "engineer" }, { actor: "setup" });
    cardIds.push(setup.moveCard(c.id, "in_progress", c.version, { actor: "setup" }).id);
  }
  setup.close();

  const barrier = new SharedArrayBuffer(4);
  const results = await Promise.all(
    Array.from({ length: CONTENDERS }, (_, i) =>
      new Promise((resolve, reject) => {
        const w = new Worker(new URL("./race-worker.mjs", import.meta.url), {
          workerData: { db: tmp.db, cardIds, name: `Engineer-${i}`, barrier, contenders: CONTENDERS },
        });
        w.once("message", resolve);
        w.once("error", reject);
      }),
    ),
  );

  const check = new Board(tmp.db);
  t.after(() => check.close());
  for (let round = 0; round < ROUNDS; round++) {
    const outcomes = results.map((r) => r[round]);
    assert.equal(outcomes.filter((o) => o === "won").length, 1, `card ${cardIds[round]}: ${outcomes.join(", ")}`);
    assert.equal(outcomes.filter((o) => o === "claim_refused").length, CONTENDERS - 1, `losers are refused: ${outcomes.join(", ")}`);
    const winner = `Engineer-${outcomes.indexOf("won")}`;
    assert.equal(check.getCard(cardIds[round]).agent.name, winner, "the stored claim is the winner's");
  }
});
