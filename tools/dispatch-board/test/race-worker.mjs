// One contender in the claim race: its own thread, its own SQLite connection.
import { workerData, parentPort } from "node:worker_threads";
import { Board } from "../dist/board.js";

const { db, cardIds, name, barrier, contenders } = workerData;
const board = new Board(db);
const gate = new Int32Array(barrier);
const results = [];
for (let round = 0; round < cardIds.length; round++) {
  // Barrier: everyone arrives, then everyone claims at once.
  const target = (round + 1) * contenders;
  if (Atomics.add(gate, 0, 1) + 1 === target) Atomics.notify(gate, 0);
  else while (Atomics.load(gate, 0) < target) Atomics.wait(gate, 0, Atomics.load(gate, 0), 50);
  try {
    board.claimCard(cardIds[round], name, { actor: name });
    results.push("won");
  } catch (e) {
    results.push(e.code ?? String(e));
  }
}
board.close();
parentPort.postMessage(results);
