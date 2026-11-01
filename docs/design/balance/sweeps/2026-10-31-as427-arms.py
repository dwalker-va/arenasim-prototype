#!/usr/bin/env python3
"""AS-427: build the arms of the extra-swing price check.

Each arm is the SAME release binary with its own `assets/` beside it (the
asset root a binary reads first, `arenasim::paths::assets_dir`), differing only
in Hand of Justice. Every "power" arm drops the proc and pays its price in
attack power instead (1.5 budget points a point), at a stated multiple of
haste's weight (`WEIGHT_ATTACK_SPEED`, 214.27 per unit chance):

  ship-swing      as shipped: +21 AP, 5% extra swing
  ship-power      no proc, +21 AP + 0.05 x 1.5 x 214.27 / 1.5   (the declared price)
  dose-swing      +21 AP, 50% extra swing
  dose-power-1x   no proc, the 50% swing paid for at 1.0x haste's weight
  dose-power-1.5x                                     at 1.5x  (the declared price)
  dose-power-2x                                       at 2.0x

A fair price predicts no difference between a swing arm and the power arm
that pays for it. The shipped pair is the item; the dose pairs bracket the
exchange RATE where it is big enough to see. The 5% item is charged the rate
the dose brackets, an ASSUMPTION: the damage each side adds is linear in
chance, but its worth in wins is not.

Ran on commit 29a716e6 (this card on main 2c63c1ff). A re-run on a later main
differs in Druid rows: #397 changed the Druid's kit after it.

    cargo build --release
    docs/design/balance/sweeps/2026-10-31-as427-arms.py target/release/arenasim assets OUT
    OUT/ship-swing/arenasim --batch sweep.jsonl --out ship-swing.csv --jobs 3 --trace-mode off
    ...
    scripts/paired_sweep.py ship-power.csv ship-swing.csv \\
      --affects Warrior,Rogue,Paladin,Shaman,Druid --tier directional --expect nothing
"""
import shutil
import sys
from pathlib import Path

BASE_AP = 21.0
SHIPPED = """            attack_power: 21.0,
            proc: Some((
                trigger: MeleeHit,
                chance: 0.05,
                effect: ExtraSwing,
            )),"""

HASTE_WEIGHT = 214.27  # WEIGHT_ATTACK_SPEED, per unit chance
DECLARED = 1.5  # EXTRA_SWING_HASTE_RATIO
AP_WEIGHT = 1.5


def swing(chance: float) -> str:
    return SHIPPED.replace("chance: 0.05", f"chance: {chance}")


def power(chance: float, rate: float) -> str:
    """The swing traded for attack power at `rate` times haste's weight."""
    ap = BASE_AP + rate * HASTE_WEIGHT * chance / AP_WEIGHT
    return f"            attack_power: {ap:.2f},"


ARMS = {
    "ship-swing": swing(0.05),
    "ship-power": power(0.05, DECLARED),
    "dose-swing": swing(0.5),
    "dose-power-1x": power(0.5, 1.0),
    "dose-power-1.5x": power(0.5, 1.5),
    "dose-power-2x": power(0.5, 2.0),
}


def main() -> None:
    binary, assets, out = (Path(a) for a in sys.argv[1:4])
    items = (assets / "config" / "items.ron").read_text()
    if items.count(SHIPPED) != 1:
        sys.exit("items.ron does not carry the shipped Hand of Justice block exactly once")
    for name, block in ARMS.items():
        arm = out / name
        if arm.exists():
            shutil.rmtree(arm)
        shutil.copytree(assets, arm / "assets")
        shutil.copy2(binary, arm / "arenasim")
        (arm / "assets" / "config" / "items.ron").write_text(items.replace(SHIPPED, block))
        print(f"{name}: {block.strip().splitlines()[0]} ... -> {arm}")


if __name__ == "__main__":
    main()
