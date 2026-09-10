# Mechanism crate integration plan

Damon owns meaning, planning, capability resolution, policy, state reduction,
execution, verification, and learning. Crates provide bounded mechanisms at the
edges of those decisions.

```text
typed boundaries -> durable events -> async host -> parallel kernels -> WASM sandbox
```

| Order | Mechanism | Current code path | Change | Cost and acceptance gate |
| --- | --- | --- | --- | --- |
| 1 | Serde | `semantic_ir.rs`, then provider and UI wire values | Replace manual wire decoding with typed, strict deserialization | Low runtime cost. Preserve byte limits and Damon validation. |
| 1 | Schemars | `semantic_ir::json_schema` | Generate the structural schema; Damon injects request-specific concept, predicate, entity-kind, and version constraints | Schema allocation occurs only at the model boundary. Contract tests must cover generated constraints. |
| 2 | Custom event log | `storage.rs`, `data.rs`, state reducers | Extend the current checksummed journal from full images to typed mutation events plus periodic snapshots | High migration risk. Old snapshots and journals must remain readable; replay must reproduce the same state. |
| 3 | Tokio | `main.rs` and the runtime host around one Damon owner | Replace blocking host I/O with bounded command, event, and result channels | Use only `rt`, `macros`, `sync`, `io-std`, and `io-util`. Prove ordering, backpressure, and one terminal result per request. |
| 4 | Rayon | Ordered packet parsing and content hashing only after benchmarks identify CPU-bound batches | Extend selected pure batch kernels; keep planning and state reduction serial | Adds a worker pool. Preserve input order and use measured thresholds that beat serial execution. |
| 5 | Wasmtime | New executor for `ImplementationKind::Generated` | Add a sandboxed implementation mechanism behind the existing tool, policy, and verification boundaries | Heavy dependency; keep features minimal and optional. No WASI or ambient file, network, process, clock, or environment access. Enforce fuel, memory, time, and output limits. |
| 6 | rpds | Temporary speculative plan or policy snapshots, if clone measurements justify it | Extend only short-lived internal snapshots | Never becomes the durable or core state model. Remove it if measurements do not show a benefit. |
| Fallback | redb | No current integration point | Consider only if event-log recovery or indexing measurements fail stated targets | `redb` 4.2 requires Rust 1.90, above Damon's current Rust 1.89 floor. Do not add it now. |
| Excluded | imbl | None | Duplicates the selected `rpds` role | No dependency. |

## Invariants

- Start each migration with a failing behavioral or compatibility test.
- Keep crate-owned types out of the persistent `damon.data` contract.
- Keep the custom event format and Damon reducers authoritative.
- Tokio and Rayon schedule work; they do not choose meaning or policy.
- Generated WASM requests typed host capabilities through Damon's registry,
  policy, execution, and verification path.
- Measure dependency count, binary size, latency, memory, and replay behavior at
  each milestone.

## Baseline

| Measure | Before milestone 1 | After Serde + Schemars |
| --- | ---: | ---: |
| Direct dependencies | `ureq` | `ureq`, `serde`, `serde_json`, `schemars` |
| Debug binary | 10,931,720 bytes | 11,995,480 bytes |
| Semantic response limit | 32 KiB | 32 KiB |

