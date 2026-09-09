# Damon Tao

Damon is a local-first Rust runtime with natural English as its human interface.
The `tao` branch uses compact IDs and arrays for memory, hashes for recognition,
graphs for meaning, count-based uncertainty, and deterministic tools for work.
Verified learned meanings and small exact rules are the fast path. An optional
language model reads new language into bounded meaning rows. It never executes
actions directly.

Networking starts with byte arrays, interfaces, addresses, packets, neighbors,
routes, sockets, connections, protocol state, and a network graph—not HTTP or the
web. Likewise, computer use resolves to native capabilities over system, network,
compute, and data primitives before any application or UI fallback. See the
[network architecture](docs/network-architecture.md) and
[capability direction](docs/capabilities.md).
The [core structure diagrams](docs/core-architecture.md) show the runtime as UML.

The Rust core is standard-library-first. Its bounded JSON reader, canonical
meaning encoding, storage, graph, cache, and network algorithms use Rust's
standard library and operating-system interfaces. HTTPS uses the established
`ureq` and `rustls` crates instead of custom TLS.
There is no runtime database, ORM, vector database, agent framework, or provider
SDK. SwiftUI is only the thin macOS window around the Rust process.

## Run locally

### Install the macOS app

Download the Universal macOS DMG from the latest GitHub release, open it, and drag
`Damon.app` to Applications. The first public build is ad-hoc signed so its
nested runtime cannot be altered unnoticed, but it is not Apple-notarized yet.
On first launch, Control-click Damon and choose **Open** if Gatekeeper asks.

The app is a deliberately small native chat window. Enter an OpenCode Zen API key;
macOS stores it in Keychain while Damon keeps it only in runtime memory. The model
picker lists supported free Zen models and remembers only the selection. The status
shows real runtime stages: processing, asking Zen, checking meaning, running,
verifying, learning, or ready. Type ordinary English and Damon replies with
structured results from the local runtime. Conversation is the interface; internal
commands, tool IDs, capability IDs, and semantic graphs are never shown. Live
memory remains on your Mac under `~/.damon/`.

### Build from source

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

## Deterministic/free-first inference

Routing reuses verified learned graphs and exact known meanings first. If language
is still unknown, exactly one configured teacher is selected:

1. OpenCode Zen, after the user supplies `DAMON_OPENCODE_API_KEY` or saves a key
   from the macOS app. The picker includes only supported free chat models.
2. Optional local/free wrapper set by `DAMON_MODEL_COMMAND`.
3. Optional Ollama fallback, enabled explicitly with `DAMON_OLLAMA_MODEL`.

The selected teacher receives one short canonical prompt with only relevant meaning
IDs and context-local entity slots. It cannot select a tool, command, application,
protocol, or persistent entity ID. Damon parses and validates the answer, with at
most one semantic repair call. Provider adapters never retry internally. External
commands receive the bounded prompt on stdin and return strict semantic JSON on
stdout. Ollama remains timeout- and size-bounded and reports “Thinking” only after
actual thinking content arrives. Set `DAMON_ZEN_MODEL` or use the picker to choose
a Zen model; set `DAMON_OLLAMA_MODEL` only when deliberately enabling Ollama.

## Remembered approvals

When an exact action needs an effect outside the current policy, Damon explains the
missing access. Saying `do it` approves only the pending capability, target,
canonical arguments, effects, and implementation version. The approval is saved
before execution and survives restart. A changed path, target, effect, capability,
or implementation asks again. Say `forget my approvals` to revoke them all. Models
cannot create approvals, and macOS permissions and missing credentials still apply.

## Implementation status

The tested kernel has recoverable binary memory, a stable namespaced semantic
registry, strict bounded candidate IR, deterministic canonical CBOR, context-slot
binding, conditional plans, structured ResultIR, exact-graph learning/reuse,
dependency-aware cached discovery, reusable verification plans, policy checks,
bounded subprocess lifetimes, compact learned strategy statistics, and basic
coding inspection/testing. It also includes native copy and bounded file search,
learned verified procedures, fundamental network discovery/diagnosis/socket
inventory, and a native macOS conversation window.
See [semantic graphs](docs/semantic-engine.md) and the
[implementation roadmap](docs/tao-roadmap.md) for the remaining work. General
source-edit synthesis and privileged packet capture are intentionally not claimed
as complete capabilities.

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

Release builds are produced by the checked-in macOS packaging script and GitHub
Actions workflow. See [release packaging](docs/releasing.md).
