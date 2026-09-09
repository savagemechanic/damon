# Tao implementation goal

The objective is a coherent, tested local-first Rust Damon runtime. This is an
ongoing implementation; the following status describes delivered code, not the
full completion criteria.

Delivered milestones:

- Formatted/buildable baseline; fixed the inherited ambiguity-test failure.
- Versioned binary snapshots and image journal; checksums, generation counters,
  exclusive writer, crash replay, previous image, atomic compaction, migration,
  backups/restore, retention limits, and corruption/truncation tests.
- Bounded provider/tool processes, local/free/cloud routing gates and call caps;
  successful execution is required before positive learning.
- Node-preserving semantic graphs, structured validated teachers, conditional
  plans, real diff/test evidence, yesterday's committed files, and persistent
  exact-graph learning with reuse tested without providers.

- Persistent world kinds, aliases, adjacency relationships, entity versions,
  project registration, focus, previous action/target, recent references, pronouns,
  and whole-procedure retargeting with safeguards against stale project binding.
- Persistent dependency-aware memo tables with entity-version and manifest-state
  invalidation, deterministic eviction, cached test discovery, and reusable
  lint/typecheck/build verification plans.
- Persistent count-based strategy statistics for success, failure, latency, cost,
  model usage, confidence, and risk; deterministic tiered ranking; and runtime
  updates from actual tool exit status and validated teacher execution.
- Canonical model-independent semantic IR with four node kinds, stable namespaced
  concepts, bounded request exposure, context-local entity slots, source spans,
  strict JSON, three-stage validation/binding, DAG composition, deterministic
  canonical CBOR/hashing, shared producer contract and prompt assets.
- Structured execution ResultIR and deterministic English rendering; models state
  meaning only while capability resolution, policy, execution, and verification
  remain Damon-owned.
- Native interface, routing-table, and passive neighbor discovery exposed through
  semantic network questions and verified capabilities on Linux and macOS.
- Provenance-aware topology derivation across machines, interfaces, addresses,
  subnets, hosts, MACs, routers, and routes, with compact selective persistence
  that rejects raw volatile observations.
- Typed macOS Wi-Fi link state from bounded structured system inspection, with
  explicit unknowns for privacy-redacted fields and no permission bypass.
- Native DNS resolution, TCP/UDP socket and process-owner inventory, and staged
  evidence-based network diagnosis.
- Dependency-aware file inventory, file/directory world records, canonical COPY
  and size-constrained FIND lowering, verified native copy, and bounded search.
- Persistent verified call/store/compare/branch/jump/return procedures that reuse
  the normal capability, policy, execution, and verification path.
- Native SwiftUI conversation window, private line-delimited runtime protocol,
  Universal 2 application bundle, DMG packaging, and GitHub release automation.
- Standard-library-first Rust core with established `rustls` HTTPS at the cloud
  boundary, bounded JSON, OpenCode Zen free-model selection, optional Ollama,
  one bounded semantic repair, and truthful runtime state events.
- Exact remembered policy approvals bound to capability, effects, entity target,
  canonical scope bytes, and implementation version; durable revocation and
  plain-language denial explanations are tested across restart.

Remaining implementation priorities:

The active architecture expansion makes fundamental networking a low-level Tao
capability. Build from bytes/interfaces/IP/routes/packets/sockets/connections to
protocols and application clients. Native capability resolution replaces generic
application automation; accessibility and pixels are escape hatches only. See
`network-architecture.md` and `capabilities.md`.

1. Extend references from projects and files to richer operation-result artifacts.
2. Expand the canonical semantic registry only from demonstrated needs and
   improve entity/span resolution and calibrated evidence.
3. Extend memoization to more verified coding results where external-state
   fingerprints can prove freshness.
4. Apply learned rankings to more choice points while preserving hard local-first,
   policy, dependency, and verification constraints.
5. Finish the general coding loop: locate/register projects, inspect/search/read files,
   discover lint/typecheck/build, propose/apply minimal edits, verify scope,
   rerun relevant checks, and preserve Git state without implicit commits.
6. Add explicitly enabled, bounded packet capture and higher protocol interpreters.
7. Improve deterministic English rendering and compact relevant teacher context.
8. Harden boundary/failure cases continuously, keep format migration/fixtures and
   architecture docs current, pass local and CI checks, and push only `tao`.

Architectural constraints remain Rust, arrays and compact IDs, hashes for lookup,
trees for structure, graphs for relationships, Bayesian/count-based uncertainty,
dynamic programming for reuse, simple reinforcement from outcomes, deterministic
tools, and LLMs as fallback teachers. No SQLite, ORM, agent framework, or vector
store. Cloud remains explicitly opt-in; learned personal state stays ignored.
