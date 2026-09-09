# Damon

**A free, local-first agentic runtime for macOS. Python does the work; local models decide what to do next.**

Damon is an open-source attempt to replace a large fraction of paid coding-agent and desktop-assistant work with a runtime you own. It is designed around deterministic tools, Ollama/local inference, explicit policy, persistent state, and verification from real system results rather than model claims.

> Status: early alpha. The first target is a reliable local coding agent; voice, a persistent daemon, Keychain-backed credentials, browser/macOS control, MCP, and broader personal-computer automation come next.

## Why Damon

Most agent systems make the model the center of the universe. Damon does the opposite.

```text
request
   │
   ▼
local model ── chooses a typed action
   │
   ▼
policy engine ── validates permission
   │
   ▼
Python tool ── performs deterministic work
   │
   ▼
verification ── exit codes / diffs / tests / state
   │
   └──────────────► model
```

The model is a planner, not a security boundary and not a source of truth.

## Principles

- **local-first** — deterministic tools first, Ollama second, optional cloud escalation later
- **tool-first** — Python/CLI/API tools perform operations instead of prompting the model to imitate them
- **model-independent** — providers sit behind a small interface
- **verifiable** — tests, diffs, exit codes, and machine state beat model assertions
- **persistent** — SQLite is the initial durable substrate for jobs and memory
- **safe autonomy** — permissions live below the model
- **small-model friendly** — few relevant tools, strict schemas, compact observations, bounded loops
- **simple architecture** — standard Python before frameworks and infrastructure

## What works now

The first implementation includes:

- bounded single-agent execution loop
- Ollama `/api/chat` provider with tool calling
- local-first model router with bounded failure-driven escalation
- routing telemetry for generations, failures, decisions, and escalations
- automatic JSON schemas from typed Python functions
- tool registry and executor
- deterministic policy decisions (`allow`, `ask`, `deny`)
- workspace-confined filesystem tools
- minimal patch application via `git apply --check`
- non-shell subprocess execution with destructive/privileged executables blocked
- Git status/diff tools
- deterministic project inspection and code search
- async event bus
- SQLite-backed persistent jobs
- deterministic verification helper
- CLI (`ask`, `tools`, `doctor`)
- pytest coverage and macOS GitHub Actions CI

## Install

Damon targets Python 3.12+ and macOS first.

```bash
git clone https://github.com/savagemechanic/damon.git
cd damon
python3 -m venv .venv
source .venv/bin/activate
pip install -e '.[dev]'
pytest -q
```

Install Ollama separately and pull a tool-capable model, for example:

```bash
ollama pull qwen3:8b
damon doctor
```

## Use

Run Damon against a repository:

```bash
cd ~/code/my-project
damon tools
damon ask "inspect this repository and explain the test failure"
```

Or target another workspace explicitly:

```bash
damon --root ~/code/my-project ask "find the bug, make the smallest fix, and run the relevant tests"
```

Override the local model:

```bash
DAMON_MODEL=qwen3:14b damon ask "review this repository"
```

Configure a local escalation chain (cheapest/fastest first):

```bash
DAMON_MODELS=qwen3:8b,qwen3:14b damon ask "fix the failing test"
```

Damon starts with the first eligible route and escalates only after deterministic evidence such as repeated tool failures or a provider generation failure. Escalation is bounded and monotonic within a run. Cloud routes are opt-in and the agent loop does not know which provider is serving a request.

### Optional cloud fallbacks

OpenCode Zen and OpenRouter can be added after the local chain without adding SDK dependencies. Credentials are read from environment variables and are never embedded in route configuration.

Use a free OpenCode Zen fallback:

```bash
export OPENCODE_API_KEY=...
DAMON_MODELS=qwen3:8b,qwen3:14b \
DAMON_ZEN_FREE_MODEL=mimo-v2.5-free \
  damon ask --allow-cloud "fix the failing test"
```

Paid routes require both an explicit run budget and a conservative per-request ceiling. Damon refuses a paid request when its ceiling cannot fit inside the remaining budget:

```bash
export OPENROUTER_API_KEY=...
DAMON_OPENROUTER_MODEL=provider/model \
DAMON_OPENROUTER_INPUT_RATE=1.0 \
DAMON_OPENROUTER_OUTPUT_RATE=2.0 \
DAMON_OPENROUTER_REQUEST_CEILING=0.25 \
DAMON_MAX_SPEND_USD=1.00 \
  damon ask --allow-cloud "finish this coding task"
```

Token rates are used for telemetry; the request ceiling is the hard preflight guard. `--local-only` overrides cloud configuration and removes all non-local routes.

## Architecture

```text
interfaces/                    voice / CLI / later menu bar
       │
       ▼
+-----------------------------+
|         Damon runtime       |
| agent loop       event bus  |
| tool registry    policy     |
| verification     state      |
+-------------+---------------+
              │
       +------+-------+
       │              │
       ▼              ▼
    models/         tools/
    Ollama          filesystem
    future          shell
    providers       Git
                   code
                   macOS / browser / etc.
              │
              ▼
           SQLite
```

The repository intentionally has no LangChain, LangGraph, Redis, Celery, Postgres, Node runtime, or workflow DSL in its core.

## Tool model

A reusable Damon capability is an ordinary typed Python function:

```python
from damon import tool

@tool(permission="filesystem.read")
def read_project_file(path: str) -> str:
    """Read a project file."""
    ...
```

Damon derives the model-facing schema from the signature and keeps permission metadata outside the model.

This is the direction of travel: **Python packages become Damon's capability ecosystem.**

## Safety model

Damon assumes model output can be wrong or hostile.

Current invariants include:

1. filesystem tools cannot escape the selected workspace root;
2. subprocess execution does not invoke a shell;
3. a deterministic executable blocklist rejects obvious destructive/privileged commands;
4. Git push is denied by default policy;
5. write operations are separated from read operations in policy;
6. patch application is dry-run checked before mutation;
7. the agent loop is bounded.

This is only the start. A production release still needs stronger sandboxing, approval UX, secret brokering, audit persistence, and adversarial testing.

## Roadmap

### 0.1 — coding-agent foundation

- [x] Python-native runtime
- [x] Ollama tool calling
- [x] typed tool registry
- [x] filesystem/shell/Git/code tools
- [x] policy engine
- [x] SQLite jobs
- [x] deterministic verification
- [x] local-first model router with bounded escalation
- [x] OpenAI-compatible cloud provider adapter (OpenCode Zen / OpenRouter / similar)
- [x] explicit cloud opt-in and conservative paid-request budget guards
- [x] CLI and tests
- [ ] structured patch/edit planner
- [ ] automatic test/lint/typecheck discovery
- [ ] richer execution transcripts
- [ ] approval flow for gated actions

### Next

- persistent `damond` managed by `launchd`
- Unix-domain-socket local API
- project memory and context retrieval
- macOS Keychain credential broker with opaque secret references
- MCP client/server boundary
- sandboxed untrusted execution
- GitHub integration
- browser tools
- Kokoro TTS + local STT
- macOS application control
- event triggers and scheduling
- optional cloud model escalation

## Non-goals

Damon is not trying to invent a graph framework, distributed workflow engine, multi-agent role-playing system, or proprietary plugin format. Those abstractions should only appear if real workloads prove they are necessary.

## Contributing

Damon is intentionally early. Contributions that make local agents more deterministic, secure, capable, or inexpensive are welcome. Keep dependencies justified, interfaces typed, diffs narrow, failures explicit, and meaningful changes tested.

## License

MIT
