# a resource whose store read is discovered after the bake keeps no answer at launch

**Status:** Open
**Systems:** runner/src/runner/kept.rs, the bake reader flag
**Author:** Charlie Cheever
**Date:** 2026-11-13

## What is wrong

Fable raised a resource whose initial ask starts an asynchronous request and
whose reply reads storage. The bake has not seen that read, so its `reader` flag
is false. A live runner discovers the read and persists the fresh answer, but
`seed_kept` in `runner/src/runner/kept.rs` only seeds declared readers. The next
cold launch forgets the entry and shows the compiled value or placeholder.

`a_read_discovered_after_the_bake_does_not_declare_a_boot_reader` in
`runner/src/runner/kept_identity_tests.rs` records this case: `account` reads
`token` in its reply, keeps `public`, and the next launch shows `nobody` with no
reader flag or retained entry.

## Why this is separate

The empty-identity issue concerns admission of an entry for a declared reader.
Making a leftover entry imply a store reader also changes bake classification,
dependency propagation, gates and reload. That inference was removed from the
empty-identity fix because it introduced unconfirmed reader status that could
leak to dependents or disappear during idleness. Solve discovery and its lifetime
as one change, including an initially closed gate, reload while a request is
pending, and a reply that reads no store. Explicitly kept modules already declare
retention and do not need this inference.
