# A kept module ready at a cold boot shows the compiled answer and is never asked

**Status:** Open
**Systems:** runner/src/runner/settlement.rs, runner/src/runner/kept_admission.rs
**Severity:** P3
**Author:** Claude (Opus 5.5)
**Date:** 2026-11-14
**Related:** issues/closed/20261113-kept-answer-refused-for-good-when-identity-starts-empty.md

## What is wrong

A resource of a kept module source (a module whose substitute says
`{"kept":true}`) is not asked at a cold boot whose data source is already
ready, when its arguments equal the bake's `initial_args`. Settlement takes
the compiled answer (`settlement.rs`, the `None if valid_args && (boot ||
!ready) …` arm: ready, arguments equal to `initial_args`, not a store reader),
so the kept answer for those arguments is consumed and dropped, and the module
is never asked, not by `bind_extensions` and not by a later `data_ready`.

## The failure

`kept_module_plan(false)` (runner/src/runner/kept_identity_tests.rs), booted
with `Source { ready: true, delayed_identity: true, .. }` and a kept entry
`[""]` → `"private"`, then `bind_extensions(&["test.kept"], &[])` and
`data_ready()`: `account` shows the compiled `"public"` and `asks` is empty.
origin/main at `2dab70171` behaves the same, so this predates the late
admission change. Found by review (Astra, round 10 of that change).

## Fix

For a kept module ready at boot, do not adopt the compiled answer: keep the
matching kept answer as a provisional seed and ask the module, as a store
reader is asked at `data_ready`. Decide first whether a ready cold boot's
compiled answer counts as an answer for a kept module (LLP 1048.003 D6,
LLP 1027 D4); the boot tests helper always boots unready, so add a
ready-at-boot test.
