# Damon Tao

Damon is a local-first Rust runtime with natural English as its human interface.
The `tao` branch uses compact IDs and arrays for memory, hashes for recognition,
graphs for meaning, count-based uncertainty, and deterministic tools for work.
LLMs are fallback teachers. They never execute actions directly.

There is no runtime database, ORM, vector database, agent framework, or provider
SDK. The kernel currently has no third-party Rust dependencies.

## Run locally

Install Rust 1.89 or newer and Git on macOS or Linux, then:

```bash
cargo run
```

Speak naturally in the interactive prompt:

```text
run the tests in Damon
show me what changed
run the tests and if they pass show me the diff
show me the files I changed yesterday
show memory status
compact my memory
back up my memory to "/existing/directory/backup.data"
restore my memory from "/existing/directory/backup.data"
exit
```

The initial `Damon` project is pinned to the startup directory's absolute path.
Register and select other projects naturally:

```text
remember project CPython at "/path/to/cpython"
remember alias py for CPython
focus on py
check it
do the same thing to Damon
show my projects
```

Project aliases, focus, recent references, and the last verified action persist.
Unknown project names trigger clarification instead of silently selecting Damon. Test discovery
currently recognizes Cargo, pytest configuration, and npm projects. It executes
project code; this runtime is not a sandbox. Results come from real process exit
codes and Git output. Git history can identify yesterday's committed files, but
cannot reconstruct the date of uncommitted edits.

## Memory

Live state defaults to `~/.damon/damon.data`. Set `DAMON_DATA` to override it.
Memory uses checksummed binary snapshots, generations, a synced append-only
journal, previous-snapshot recovery, bounded learning retention, and compaction.
The runtime reports damaged tails instead of silently discarding the whole brain.

**Never commit live memory, backups, journals, private learned state, or secrets.**
Synthetic format/migration fixtures under `tests/fixtures/` are tracked. See the
[data format and recovery contract](docs/data-format.md) for exact guarantees,
limits, migration, and recovery failure modes. Backups are not encrypted and
must be protected as personal data. Credentials do not belong in the brain.

## Local/free-first inference

Routing tries deterministic or verified learned graphs first, then:

1. Ollama (`qwen3:8b` by default; `DAMON_OLLAMA_MODEL` selects another model).
2. Optional free/local wrapper set by `DAMON_MODEL_COMMAND`.
3. Optional `DAMON_CLOUD_COMMAND`, only with `DAMON_ALLOW_CLOUD=1`.

Commands receive a compact prompt on stdin and return a structured graph on
stdout. Provider commands are trusted operator configuration, never model output.
Each enabled route is attempted once, with a 30-second timeout. Invalid graphs
fall through to the next route. Cloud is disabled by default. When enabled,
`DAMON_CLOUD_CALL_LIMIT` caps attempts per session (default one); a paid wrapper
must also enforce its provider-specific monetary budget. No provider SDK is
required. Set `DAMON_OLLAMA_MODEL=''` to skip Ollama.

## Implementation status

The tested kernel has recoverable binary memory, bounded candidate graphs,
validated teacher graphs, conditional plans, exact-graph learning/reuse,
dependency-aware cached discovery, reusable verification plans, policy checks,
bounded subprocess lifetimes, compact learned strategy statistics, and basic
coding inspection/testing.
See [semantic graphs](docs/semantic-engine.md) and the
[implementation roadmap](docs/tao-roadmap.md) for the remaining work. Rich file
editing, cached coding results, and learned procedures are ongoing work, not
complete capabilities.

## Development

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Tests create isolated deterministic files/repositories and use no paid providers.
See [contributor guidance](CONTRIBUTING.md). The former Python implementation is
preserved under [`legacy/python/`](legacy/python/README.md) as historical reference.
It is not used by Tao; its separate tests remain in CI.
