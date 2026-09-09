# Canonical semantic interface v1

The stable boundary is: **models produce meaning; Damon produces execution**.
Every semantic producer receives the same bounded `SemanticRequest` and returns a
`SemanticResolution`. The deterministic rule engine, verified learned mappings,
Ollama, external free models, and optional cloud models do not select tools,
commands, libraries, protocols, applications, effects, or policy.

```text
English -> candidate IR -> validate -> bind -> rank -> canonical meaning
        -> capability -> plan -> policy -> execute -> ResultIR -> English
```

## Meaning-only IR

IR version 1 has four node kinds: `ACTION`, `ENTITY`, `VALUE`, and `CONSTRAINT`.
Graphs are bounded arrays of at most 32 nodes and 64 typed edges; a response has
at most three candidates. The stable registry reserves high-byte namespaces:

| Prefix | Namespace |
| --- | --- |
| `0x01` | action |
| `0x02` | predicate |
| `0x03` | property |
| `0x04` | entity kind |
| `0x05` | operator |
| `0x06` | value type |

Registry version 3 includes coding and network meanings plus `COPY`, `FIND`,
`TARGET`, `OBJECT`, `SOURCE`, `DESTINATION`, `TIME`, `AFTER`, `ON_SUCCESS`,
`ON_FAILURE`, `REQUIRES`, size/greater-than, bytes, and the required entity
kinds. IDs are permanent; names are metadata.

Models see context-local slots such as `0 Damon` or `1 CPython`, never persistent
entity IDs. Unknown user text is represented by validated UTF-8 byte spans into
the original request. Thus `parser.rs` can be bound from the user's exact text,
but a model cannot invent a path, host, credential, command, or tool argument.
Abstract semantic entities such as a file set are registry concepts, not strings.

Structural validation checks versions, bounds, node kinds, indexes, duplicate or
self edges, strict JSON fields, and source spans. Semantic validation checks the
exposed concept subset, action arguments, entity-kind compatibility, constraint
types, and the small acyclic composition vocabulary. Only `AFTER`, `ON_SUCCESS`,
`ON_FAILURE`, and `REQUIRES` compose v1 graphs. There are no loops, jumps, eval,
shell, tool calls, threads, or exception handlers in model-facing IR.

## Producer contract and prompts

Prompt assembly selects only relevant actions, predicates, entity kinds, slots,
focus, and previous action. When no word cue is strong enough, the complete v1
action set is still only twelve IDs and is exposed so unfamiliar phrasing is not
blocked before a model can interpret it. All providers share the checked-in templates at
`prompts/semantic-ir-v1.txt` and `prompts/semantic-ir-v1-compact.txt`. The full
template carries the strict JSON shape for providers without schema enforcement;
the compact template is intended for constrained local decoding. Unknown fields,
invented concepts, invented slots, invalid spans, and unsupported meanings fail
closed. A rejected meaning gets at most one fresh repair attempt. A provider
adapter cannot retry internally, so one meaning request makes at most two model
calls. There is no unbounded repair loop.

OpenCode Zen is the default optional teacher when the user supplies a key. Damon
uses `ureq` and `rustls` for bounded HTTPS, fetches Zen's current model catalog,
and exposes only free models supported by the initial chat-completions adapter.
The API key is stored in macOS Keychain and passed privately to the child runtime;
it is never stored in `damon.data`. The model picker stores only the model ID.
Ollama is disabled by default but remains an explicitly configured fallback. Its
adapter reports `thinking` only after receiving a non-empty thinking field.

Models do not supply confidence. Damon scores schema validity, bound-entity
quality, conversation context, learned counts, and candidate margin with integers.
Multiple candidates remain `AMBIGUOUS` unless evidence establishes a sufficient
margin. Confidence and policy authorization are independent.

## Canonical bytes, learning, and execution

Validated candidate IR serializes deterministically as canonical CBOR arrays:

```text
[ir_version, registry_version, nodes, edges]
```

Map ordering cannot affect the bytes. Decoding rejects non-canonical integers,
trailing data, version mismatches, unknown concepts, invalid slots, and malformed
graphs. Stable FNV-1a hashing supports cache keys and deduplication.

After context binding, executable meanings lower to capability IDs. Capability
resolution chooses a verified native/composed/system/protocol implementation;
policy recomputes effects before execution. A semantic producer cannot grant a
capability or select its implementation. Successful verified execution feeds
phrase/action counts and exact graph reuse; failures remove the exact learned
mapping.

Execution first produces `ResultIr`: status, observations, changes, checks,
artifacts, and diagnostics. Routine facts render deterministically—for example,
84 passed and zero failed becomes “All 84 tests passed.” A future explanatory
model may receive those facts, but cannot change them.
