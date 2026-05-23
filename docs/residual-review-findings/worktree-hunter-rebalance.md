# Residual Review Findings — worktree-hunter-rebalance

Source: `ce-code-review mode:autofix` run `20260522-145158-1a9b3e48` against plan
`docs/plans/2026-05-22-001-fix-hunter-mana-economy-plan.md`.

Branch: `worktree-hunter-rebalance`
Run artifact: `/tmp/compound-engineering/ce-code-review/20260522-145158-1a9b3e48/`

The autofix loop applied 2 safe_auto fixes (HEREDOC for `--help` + CLAUDE.md
discoverability subsection). These residual findings were left for downstream
resolution.

## Residual Review Findings

- **[P2][manual]** `scripts/hunter_2v2_matrix.sh:108,121` — `|| true` + case-default-to-DRAW silently masks binary crashes as draws.

  *Evidence:* `"$BINARY_PATH" --headless "$CFG_PATH" >/dev/null 2>&1 || true` suppresses non-zero exits; the case-statement catch-all on missing `Winner:` line then increments `DRAWS`. A simulator crash, panic, or argument-parse regression becomes indistinguishable from a real draw in the resulting CSV. Three reviewers (correctness, maintainability, agent-native) corroborated (anchor 100).

  *Suggested fix:* Capture the binary's exit code; if non-zero, increment a separate `FAILURES` counter and surface a warning to stderr. Distinguish "engine error" from "real draw" in the CSV (e.g., add a `failures` column, or fail-fast on non-zero exit). The Paladin+Priest matchup currently reports 10/10 draws — without this fix, that result is indistinguishable from 10/10 silent crashes.

- **[P3][gated_auto]** `scripts/hunter_2v2_matrix.sh:132` — `AVG_DURATION` divides by N rather than the count of matches that successfully wrote a `Duration:` line.

  *Evidence:* `AVG_DURATION=$(awk -v t="$TOTAL_DURATION" -v n="$N" 'BEGIN {printf "%.2f", t / n}')` — uses `N`, not a `COMPLETED` counter. When any match's log is missing, the average is systematically too low.

  *Suggested fix:* Track a `COMPLETED` counter that increments only when `Duration:` is parsed; divide `TOTAL_DURATION` by `COMPLETED`. Falls naturally out of fixing the first finding (since both stem from the same silent-failure path).

- **[P3][advisory]** `scripts/hunter_2v2_matrix.sh:15-16` — `OPPONENTS` and `HEALER='Priest'` are hardcoded; wrapper is Hunter+Priest-specific.

  *Evidence:* Plan's optional follow-up calls for Paladin partner re-runs. Agent-native reviewer suggested lifting into `--matrix` as a team-template flag. Acceptable at one consumer; revisit if Paladin partner re-runs are wanted or a second class needs 2v2 validation.

  *Suggested fix:* Defer to a future iteration. If a second use case emerges, consider either renaming to `scripts/healer_2v2_matrix.sh` with `--t1-class` / `--t2-classes` args, OR lifting team templates into `--matrix` itself as a CLI flag.

## Testing gaps (advisory)

These are forward-looking, not actionable in this PR:

- No Hunter-specific stat regression test pinning `max_mana=240`, `mana_regen=0` (the 12-tuple positional stat structure has no Hunter pin).
- No automated smoke test for `scripts/hunter_2v2_matrix.sh`.
- No invariant test enforcing `mana_regen == 0` across all mana classes (newly true after this change).

## Mode-aware demotion suppressions

4 findings below anchor 75 were suppressed by the synthesis pipeline:

- Non-round mana_cost values (45% confidence — judgment call about readability)
- Inline comment embeds drift-prone derivation (50% — low-impact)
- Documentation surface heavy for the size of the code change (40% — judgment call)
- `OPPONENTS`/`HEALER` hardcoding at low confidence (55% — included above as advisory anyway because agent-native corroborated)
