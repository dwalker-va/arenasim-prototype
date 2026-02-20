---
status: pending
priority: p3
issue_id: "016"
tags: [code-review, defensive, spawn]
dependencies: []
---

# Post-Spawn Count Validation Missing From Graphical Mode

## Problem Statement

The headless runner has post-spawn count validation but `setup_play_match` in `mod.rs` does not. The config parsing fix makes this largely moot (None entries can't occur from headless), but graphical mode could theoretically produce None slots.

## Proposed Solution

Either add matching validation to graphical mode, or remove it from headless mode (since the config fix makes it unreachable). Prefer removing since it's dead code after the config.rs fix.

## Work Log

- 2026-02-20: Found during code review
