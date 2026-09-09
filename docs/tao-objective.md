# Original Tao implementation objective

This is the original requested scope, preserved for cloud-agent continuation.
Some initial-state descriptions below are historical. Read `tao-roadmap.md`,
the current code, and current CI for implementation status. Do not reduce the
completion criteria to the milestones already delivered.

Work on the `tao` branch of `https://github.com/savagemechanic/damon` and treat this as a long-running implementation goal. Do not switch back to the old Python architecture. The objective is to finish and harden the Rust Tao architecture until it is a coherent, tested local-first Damon runtime.

Core architecture to preserve:

* Rust runtime
* natural language is the only human-facing interface
* English first; other human languages later
* internal state is arrays and compact integer IDs
* arrays are memory
* hashes are recognition/lookup
* trees are structure
* graphs are relationships/understanding
* algorithms are intelligence
* Bayesian/statistical inference manages uncertainty
* dynamic programming and tabulation reuse solved work
* reinforcement learning updates strategy from outcomes
* LLMs are fallback teachers/reasoners, not the core runtime
* successful LLM resolutions should become cheaper learned data
* deterministic tools do real work
* model output is never a security boundary
* local/free inference first, optional cloud escalation only when explicitly enabled
* `damon.data` is the evolving binary state/brain
* no SQLite
* no ORM
* no agent framework
* no vector database unless hard evidence later proves it necessary
* no unnecessary abstraction layers
* keep dependencies minimal
* prefer fundamental data structures and standard library code
* do not commit secrets or user-specific learned state

The desired system shape is:

```text
English input
    ↓
normalization / entity recognition
    ↓
probabilistic candidate meaning graphs
    ↓
Bayesian scoring + context + learned evidence
    ↓
beam pruning
    ↓
winning graph or LLM teacher fallback
    ↓
deterministic reasoning / planning
    ↓
policy
    ↓
tool execution
    ↓
verification from real system state
    ↓
learning
    ↓
damon.data
    ↓
natural-language response
```

Model routing should remain approximately:

```text
Damon deterministic/learned NLP
    ↓ unresolved
local Ollama
    ↓ unavailable / insufficient
optional free external provider command
    ↓ only when explicitly enabled
optional paid cloud command
```

Cloud use must remain disabled by default. Keep provider support model-independent and avoid SDK lock-in where a generic command/provider boundary works.

Your job is to systematically finish the Tao branch. Inspect the current repository first and continue from the existing Rust implementation rather than rewriting blindly.

Priority 1: make `damon.data` durable and first-class.

Implement:

* explicit binary file format version
* generation counter
* whole-image checksum or equivalent integrity verification
* append-only journal for mutations/learning
* crash-safe replay
* safe snapshot creation
* previous-known-good snapshot retention
* atomic replacement
* recovery from truncated/corrupt tail records
* compaction
* experience retention/decay or distillation so the file does not grow without bound
* migration support between data format versions
* export/import/backup/restore commands or natural-language-accessible operations
* clear separation between:

  * tracked canonical seed/format/test assets in the repo
  * ignored live personal state such as `~/.damon/damon.data`
* tests for:

  * round trips
  * corruption
  * truncated writes
  * interrupted journal
  * recovery
  * migration
  * compaction
  * large-file growth
  * repeated learning updates
* README/documentation describing what is safe to commit and what must never be committed

Priority 2: complete the semantic language engine.

Move beyond flat `intent + target`.

Represent actual semantic relations such as:

* action
* object
* target
* source
* destination
* time
* condition
* reference
* modifier
* dependency
* result

Build multiple compact candidate meaning graphs incrementally and keep only a bounded beam.

Use:

* lexical evidence
* entity hashes
* learned phrase/context counts
* grammar patterns
* conversation state
* world-state compatibility
* Bayesian priors/posteriors
* score margins
* risk-sensitive confidence thresholds

Do not enumerate all parses.

Add support for requests such as:

* “run the tests in Damon”
* “show me what changed”
* “show me the files I changed yesterday”
* “copy parser.rs from Damon to my desktop”
* “run the tests and if they pass show me the diff”
* “do the same thing to CPython”
* “check it”
* “see if yesterday’s parser fix worked”

When confidence is insufficient, ask the LLM teacher for a structured meaning graph rather than an unconstrained prose answer.

Validate teacher output against known entities, relations, allowed intents, and policy before execution.

Verified teacher resolutions must become training evidence.

Priority 3: world graph and context.

Implement compact persistent world-state structures for:

* projects
* files
* tools
* procedures
* hosts
* processes where useful
* known concepts
* relevant relationships
* current conversational focus
* previous action
* previous target
* recent references

Keep structures array-first and ID-based.

Use adjacency arrays/offsets rather than pointer-heavy object graphs.

Add efficient entity resolution and aliases.

Support context resolution for:

* it
* that
* this
* its
* they
* same thing
* there
* previous project/action references

Priority 4: dynamic programming and incremental computation.

Implement:

* version counters on mutable entities
* dependency tracking for cached computed results
* memoized reusable answers
* invalidation when dependencies change
* sparse tabulation keyed by state hashes
* reuse of known verification plans
* reuse of project/test/tool discovery
* avoid recomputing unchanged information

Keep this simple and measurable.

Priority 5: reinforcement and learned strategy.

Add compact learned statistics for:

* state/action success
* failures
* latency
* cost
* LLM usage
* confidence
* risk

Prefer integer/count-based or simple Bayesian representations.

Use this to rank strategies such as:

* inspect first
* search code
* run tests
* use deterministic tool
* ask local LLM
* ask external free provider
* ask user

Do not build a large RL framework.

Priority 6: learned procedures.

A repeated verified sequence should be representable as data instead of new Rust code.

Implement a tiny instruction representation for composite procedures, enough for things like:

* call tool
* store result
* compare result
* conditional branch
* jump
* return

Atomic and composite tools must continue through the same policy and execution path.

No separate workflow subsystem.

Priority 7: coding-agent completeness.

Tao should be able to handle the core coding-agent loop:

* locate/resolve a project
* inspect repo structure
* search code
* read relevant files
* run tests
* run lint/typecheck/build where deterministically discoverable
* propose/apply minimal edits
* inspect Git diff
* verify scope
* rerun relevant checks
* report actual evidence
* preserve Git state
* do not commit/push/merge unless explicitly authorized

Reuse good ideas from the Python branch only when they fit Tao’s architecture. Port semantics, not structure.

Priority 8: model routing.

Preserve free/local-first behavior.

Support:

* Ollama
* generic external provider command
* optional free provider wrappers
* optional cloud command only with explicit enablement
* budgets/limits for paid routes if paid routes are enabled later

The model should receive only relevant tools/context.

Keep prompts compact.

Bound retries.

Never allow the model to execute arbitrary actions directly.

Priority 9: output language.

Simple structured results should render deterministically into natural English.

Use an LLM for output only when genuinely useful.

Examples:

```text
all tests passed
→ “All 84 tests passed.”

3 files changed, tests passed
→ “Three files changed, and all tests pass.”
```

Priority 10: repository cleanup.

The Tao branch currently inherits old Python files and an outdated README.

Cleanly decide what belongs to Tao.

Do not delete useful historical code recklessly, but the Tao branch should become internally coherent and clearly Rust-first.

Update:

* README
* architecture docs
* `.gitignore`
* CI
* contributor guidance
* data-format documentation
* local setup instructions
* model-routing instructions

The README must no longer describe SQLite/Python as the Tao architecture.

Testing requirements:

Run and pass all applicable checks before considering work complete:

```text
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Also add targeted tests for all important new behavior.

If integration tests need temporary repositories or files, create them deterministically.

Use real exit codes, diffs, and state checks.

Do not claim success from model output.

Git workflow:

* work only on `tao`
* make small coherent commits
* commit only verified changes
* inspect diffs before committing
* preserve existing Git state
* do not merge `tao` into `main`
* do not push to `main`
* pushing commits to `tao` is authorized
* if CI fails, inspect and fix it
* continue until the branch is in a coherent, tested state

Engineering constraints:

* keep important interfaces typed
* prefer arrays, slices, indexes, bitsets, compact records, adjacency arrays, append logs
* avoid giant enums/classes/frameworks
* avoid deep trait hierarchies unless truly needed
* avoid pointer-heavy internal structures in hot/persistent data
* use explicit little-endian encoding for persistent state
* never raw-dump Rust structs as the file format
* use timeouts for external commands/processes
* propagate explicit errors
* do not silently swallow failures
* keep dependencies minimal and justified
* do not store credentials in `damon.data`
* credentials should remain opaque references handled by a future secure credential boundary
* keep live learned/personal binary state ignored by Git
* keep test seeds/format fixtures tracked

Completion criteria:

Tao should feel like one coherent system, not a collection of experiments.

A successful final state means:

1. Rust builds cleanly.
2. All tests pass.
3. Clippy is clean.
4. `damon.data` is crash-safe, recoverable, versioned, compactable, and portable.
5. natural language produces real semantic candidate graphs.
6. ambiguous language uses probabilistic scoring and bounded fallback.
7. LLM teacher output can be learned from.
8. routine learned requests increasingly avoid LLM usage.
9. context/pronouns work for common conversational commands.
10. deterministic coding operations work end-to-end.
11. verification happens from system evidence.
12. model routing remains local/free-first.
13. Tao documentation matches the actual code.
14. `main` remains untouched.
15. everything is pushed to `tao`.

Keep working through failures rather than stopping at the first obstacle. Make the best technical decisions consistent with the architecture above, prefer simplicity, and verify continuously.
