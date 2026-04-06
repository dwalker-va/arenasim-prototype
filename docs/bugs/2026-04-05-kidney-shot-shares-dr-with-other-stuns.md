---
date: 2026-04-05
type: bug
severity: medium
status: open
---

# Bug: Kidney Shot Shares DR Category with All Stuns

## Summary

Kidney Shot shares the `Stuns` DR category with Cheap Shot, Hammer of Justice, and all other stuns. In WoW Classic/TBC, Kidney Shot has its own DR category — it should only DR with other Kidney Shots (e.g., two Rogues both using Kidney Shot on the same target).

## Observed Behavior

Cheap Shot (4s stun) followed by Kidney Shot results in Kidney Shot at 50% DR (3s instead of 6s), because both map to `AuraType::Stun` → `DRCategory::Stuns`.

## Expected Behavior

Kidney Shot should have its own DR category. A Cheap Shot followed by Kidney Shot should apply Kidney Shot at full duration. Two Kidney Shots from different Rogues should DR with each other.

## Root Cause

The DR system maps `AuraType` → `DRCategory`. Since Kidney Shot and Cheap Shot both use `AuraType::Stun`, they share `DRCategory::Stuns`. Fixing this requires the DR system to support per-ability DR categories rather than per-AuraType categories.

## Fix Required

Refactor the DR system to allow abilities to specify their DR category independently of their AuraType. This would let Kidney Shot use `AuraType::Stun` (for the gameplay effect) while having a separate `DRCategory::KidneyShot` (for diminishing returns tracking).
