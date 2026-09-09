# Damon Tao — Rust kernel

`tao` is the experimental Rust rewrite of Damon around a deliberately small idea:

> Natural language in → compact data → fundamental algorithms → machine actions → natural language out → learning.

The human interface is English. Internally Damon uses typed integer IDs, arrays, hashes, compact meaning graphs, Bayesian/count-based evidence, deterministic tools, and a binary `damon.data` file. The runtime learns successful language mappings into that file so repeated requests can avoid model inference.

## Run

```bash
cargo run
```

Damon creates `~/.damon/damon.data` on first start. Override it with `DAMON_DATA=/path/to/damon.data`.

Example requests:

```text
show me what changed in Damon
run the tests in Damon
show git status
list files
```

## Model routing: free first

The language engine always tries deterministic/local learned interpretation first. A model is only used when the request is unknown.

Routing order:

1. local deterministic/learned interpretation — zero inference cost
2. local Ollama — default model `qwen3:8b`
3. optional external command from `DAMON_MODEL_COMMAND`
4. optional cloud command from `DAMON_CLOUD_COMMAND`, but only when `DAMON_ALLOW_CLOUD=1`

Examples:

```bash
DAMON_OLLAMA_MODEL=qwen3:8b cargo run
DAMON_MODEL_COMMAND='my-free-model-wrapper' cargo run
DAMON_ALLOW_CLOUD=1 DAMON_CLOUD_COMMAND='my-cloud-wrapper' cargo run
```

The external command receives the prompt on stdin and must print the response on stdout. This keeps Damon provider-independent and allows free/local providers, OpenCode-style routers, or future APIs to be plugged in without adding provider SDKs to the kernel.

Cloud use is intentionally disabled by default.

## Current kernel

The first Tao kernel implements:

- versioned/checksummed `damon.data`, append-only image journal, crash recovery, compaction, and portable backups
- compact entity IDs and name hash index
- language normalization and cheap deterministic intent scoring
- learned phrase→intent counts persisted from verified outcomes
- local-first model router with Ollama and generic command fallbacks
- deterministic policy effects
- Git status/diff, test discovery, and file-listing tools
- natural-language interactive loop
- unit tests for binary persistence and language resolution

It intentionally does **not** introduce a database, agent framework, cloud SDK, vector database, or neural runtime.

## Personal state and fixtures

Never commit live `damon.data`, journals, backups, or learned personal state.
Only synthetic format fixtures under `tests/fixtures/` belong in Git.
See [the data format and recovery guide](docs/data-format.md) for durability
guarantees, limits, migration, and natural-language backup/restore operations.

Provider attempts have a 30-second timeout and 1 MiB output limit. An invalid
interpretation falls through to the next enabled route. Each route is tried
once per request. `DAMON_CLOUD_CALL_LIMIT` bounds paid attempts per runtime
session (default 1, including failed attempts); the wrapper must enforce any
provider-specific monetary/token budget. This is a call cap, not a dollar cap.
Set `DAMON_OLLAMA_MODEL` to an empty value to skip local inference. No paid route
runs unless explicitly enabled. Tool processes have a 120-second timeout.
On macOS/Linux each subprocess has its own process group; timeout, completion,
and excessive output trigger descendant cleanup. This is not a sandbox for
untrusted project code. Only successful tool results train positive evidence;
a provider answer alone never does.
