# Contributing to Tao

Work on `tao`. Preserve unrelated Git changes. Do not merge to or push `main`.
Keep commits coherent, inspect diffs, and commit only after the relevant checks
pass. Before pushing, run:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Use typed IDs, arrays, slices, explicit little-endian records, bounded beams,
and simple algorithms. Keep dependencies minimal and justify additions. Do not
introduce databases, ORMs, agent frameworks, or a parallel workflow subsystem.
Never raw-dump Rust structures into persistent files. Update format documentation
and migration tests when adding persistent fields.

Natural English is the user interface. Models suggest validated meanings; they
cannot invent executable commands or authorize effects. Atomic/composite work
must go through the same planner, policy, and execution boundary. Test actual
exit codes, file contents, and Git state. Do not equate teacher output with proof.
Every external process must have bounded lifetime and output.

Live state belongs outside Git. Only synthetic `tests/fixtures/` brain files are
allowed. Do not commit secrets, private project paths, or learned personal state.
Tests must allocate isolated temporary locations and clean up after themselves.

`legacy/python` preserves historical semantics for reference. Port useful ideas
into the Rust architecture; do not route Tao through the Python runtime. If
moving or changing legacy files, run its tests from `legacy/python` with its dev
extras installed. Its CI is separate from the Rust CI.
