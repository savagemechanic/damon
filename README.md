# Damon

Damon is a native macOS interface and local Python execution harness for language models. The model writes ordinary Python; Damon runs it locally, streams back real computer output, and preserves useful scripts as reusable tools.

> Status: v0.1 release candidate. Generated model code is untrusted and executes with the current user's permissions. Inspect the visible Python before relying on it.

## Architecture

```text
[queue] conversation messages
            │
            ▼
OpenCode Zen streaming API
            │
            ▼
[union] PythonAction | FinalAnswer
            │
    PythonAction
            ▼
[process tree] damond → generated_script.py → descendants
            │
            ▼
[append-only log] DamonEvent JSONL + SQLite indexes
            │
            ▼
SwiftUI Chat / Activity / Tools views
```

The model-facing protocol has two deterministic forms:

```text
ACTION: python + one fenced Python block
ACTION: finish + final response
```

Damon does not expose a large model-facing tool registry and does not depend on an agent framework, Node, Electron, Redis, Celery, Kafka, or Postgres.

## Install

Download `Damon-v0.1.0-macos-arm64.zip` and its checksum from the GitHub Release, then:

```bash
shasum -a 256 -c Damon-v0.1.0-macos-arm64.zip.sha256
ditto -x -k Damon-v0.1.0-macos-arm64.zip ~/Applications
open ~/Applications/Damon.app
```

Unsigned development builds may require Control-click → Open because they are not notarized. Damon includes an arm64 CPython runtime; a separate Python installation is not required by the packaged application.

## OpenCode Zen

Open **Damon → Settings…** (`⌘,`), then:

```text
API key → Save → Test Connection → Model → optional Thinking effort
```

The key is stored only in macOS Keychain. It is never written to `config.json`, SQLite, events, logs, prompts, or generated scripts. Models are discovered from Zen and cached for offline display. Reasoning effort is enabled only when the selected model advertises compatible options.

## Local state

```text
~/.damon/
├── config.json
├── damon.sqlite3
├── models.json
├── damon.sock
├── runs/<run-id>/
│   ├── 001.py
│   ├── events.jsonl
│   └── metadata.json
└── tools/{git,files,network,system,misc}/
```

Reusable tools remain ordinary readable `.py` files that can be opened, edited, copied, executed, or deleted independently.

## Security model

```text
LLM output = untrusted input
```

- Every generated script is persisted before execution and shown in the UI.
- Scripts run in child process groups, never through daemon `exec()`.
- Timeouts and cancellation terminate the process group.
- stdout/stderr capture is bounded while live output remains visible.
- Crashes and non-zero exits become observations; they do not crash Damon.
- The API key remains behind the Keychain boundary.
- The working directory is explicit in every run event.
- Damon does not silently elevate privileges or provide a security sandbox in v0.1.

## Development

```bash
python3 -m venv .venv
.venv/bin/pip install -e '.[dev]'
.venv/bin/pytest -q

PYTHONPATH=python/src .venv/bin/pytest -q python/tests
./scripts/swift_test.sh
./scripts/build_release.sh
open build/Damon.app
```

The current Mac Command Line Tools distribution needs the workaround encoded in `scripts/swift_test.sh` to locate Apple's Testing framework. Full Xcode runs `swift test` normally.

## Release

```text
tests → release build → embed CPython → ad-hoc sign → zip → extract
      → bundled daemon IPC smoke test → SHA-256 → GitHub Release
```

Run locally:

```bash
./scripts/smoke_test.sh
./scripts/package_release.sh
```

The tag-driven GitHub workflow publishes only after Python, Swift, app-build, and extracted-package smoke gates pass.

## Known limitations

- v0.1 is arm64-only.
- Development artifacts are ad-hoc signed and not notarized unless Apple credentials are supplied separately.
- Generated Python is not sandboxed beyond child-process isolation and bounded execution.
- Sophisticated semantic matching and automatic reuse of saved tools are post-v0.1 work.

## Roadmap

```text
v0.1 durable execution
  └── stronger sandboxing
      └── approval policy UX
          └── semantic tool reuse
              └── signed and notarized distribution
```
