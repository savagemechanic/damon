# Damon binary state, envelope version 2

Live memory defaults to `~/.damon/damon.data`; `DAMON_DATA` overrides it. State
contains private project paths and learned evidence. It is not a credential
store. Never commit live memory, backups, journals, locks, rejected records, or
temporary images. Only synthetic fixtures in `tests/fixtures/` are tracked.

All integers use explicit little-endian encoding. No Rust layouts are dumped.
The snapshot and each append-only journal transaction have this envelope:

| Offset | Field |
| --- | --- |
| 0 | 8 bytes, `DAMONIMG` |
| 8 | u32 format version (2) |
| 12 | u32 flags (0; others rejected) |
| 16 | u64 generation |
| 24 | u32 payload length (maximum 16 MiB) |
| 28 | u32 IEEE CRC-32 over header bytes 0–27 followed by payload |
| 32 | payload |

CRC detects accidental corruption, not malicious tampering. There is no claim
of cryptographic authenticity, encryption, or a security boundary here.

The v2 payload retains the v1 explicit schema: eight bytes `DAMON\0\x01\0`,
u32 entity count, entities (u16 kind, u64 flags, u32 version, name, value),
u32 phrase count, phrase rows (u64 hash, u32 count, pairs of u32 intent and
u32 observations), u32 experience count, experiences (u64 hash, u32 intent,
i16 reward, u8 confidence). Strings are u32 byte length followed by UTF-8.
Entity IDs are array indexes. Phrase rows serialize in ascending hash order.
Trailing bytes and duplicate entity names are invalid. Legacy v1 snapshots
are read directly and become v2 on compaction; the previous image is retained.
The synthetic v1 fixture is an executable migration contract.

## Commit and recovery

One process holds an OS file lock on `damon.data.lock` for the open lifetime.
The OS releases the lock on exit/crash; the lock file itself remains. Rust 1.89
or newer is required for the standard-library locking API. Initial support is
macOS/Linux local filesystems with atomic rename, file sync, and directory sync.
Do not use a shared network filesystem as a brain store.

Mutations in memory become durable on `save`: append a full image transaction,
sync the journal, then sync its directory. Generations increase monotonically.
Full image transactions trade write amplification for simple, independently
verifiable replay, without a second mutation interpreter. A failed/uncertain
write poisons the store until reopen; success must never be inferred from it.

Compaction writes/syncs the previous snapshot to `.prev` through a temporary
file and atomic rename, writes/syncs the new primary snapshot the same way,
then truncates/syncs the journal. Directory entries are synced after replacement.
A crash before truncation can leave already-checkpointed records; replay skips
those generations. Stale `.tmp` files are never replayed.

Startup validates primary and previous images, chooses the newest valid one,
then scans the journal for newer complete transactions. It stops at the first
invalid/truncated record, saves rejected tail bytes to `.rejected`, truncates
that tail, and reports recovery warnings. Later records are never guessed or
salvaged across corruption. Without any valid image, opening fails rather than
silently resetting memory. Previous-snapshot recovery can lose updates after
that snapshot when the primary and journal are unavailable; backups remain useful.

The journal compacts automatically at 8 MiB (a transaction can temporarily take
it up to 24 MiB). Images have a 16 MiB limit. Learning retains at most 4,096 raw
experiences and 8,192 phrases; least-observed phrases are evicted first with a
deterministic tie break. Counts saturate rather than overflow. Frequently used
phrases retain distilled counts after raw experiences expire. Registered world
entities are not silently discarded; image-limit errors require explicit cleanup.

## Natural-language operations

```text
show memory status
compact my memory
back up my memory to "/path/to/backup.data"
export my memory to "/path/to/backup.data"
restore my memory from "/path/to/backup.data"
import my memory from "/path/to/backup.data"
```

Export includes current in-memory state as a portable checksummed snapshot and
refuses existing destinations. Restore validates the entire backup before
committing replacement state, keeps generations increasing, and retains the
previous checkpoint. Backup parent directories must exist. These operations
are parsed locally; no model sees memory contents or chooses backup paths.

## Payload revision 2: learned graphs

New snapshots use payload magic `DAMON\0\x02\0` with the same base fields as
revision 1, then a u32 learned graph count (maximum 4,096). Each graph is u64
phrase hash, u8 confidence, u32 node count (maximum 32), nodes (u8 kind, u32
value), u32 edge count (maximum 64), edges (u32 source index, u16 relation, u32
target index). Node tags are 0 action, 1 entity, 2 concept, 3 time, 4 condition;
unsupported semantic combinations are rejected. Graph summary intent/target
are derived, not redundantly persisted. Graph rows serialize by ascending hash.
Revision 1 payloads, including those inside v2 envelopes, remain readable and
migrate on the next save. See [semantic validation](semantic-engine.md).

## Payload revision 3: world and context

Payload magic `DAMON\0\x03\0` appends world state after the revision-2 graphs:
u64 world version; u32 alias count followed by (string normalized name, u32 entity
ID); u32 relationship count followed by (u32 source ID, u16 relation, u32 target
ID); u32 focus, previous target and previous action (u32::MAX means absent);
u8 previous-feature presence flag and u64 feature; u32 recent-reference count
followed by u32 entity IDs. Limits are 8,192 aliases, 65,536 links, eight recent
references. Relationships serialize by source/relation/target. Adjacency offsets
and hash indexes are derived, validated, and rebuilt on open; no pointers persist.

Entity kinds: 1 project, 2 file, 3 tool, 4 procedure, 5 host, 6 process, 7 concept,
8 directory. Relationships: 1 contains, 2 uses, 3 runs on, 4 implements, 5 related
to, 6 produces. Public mutation methods increment versions when values change.
Alias conflicts and unknown relationship/context endpoints are rejected.
Both prior payload revisions remain readable. `v2-seed.data` is a synthetic
checksummed migration fixture. Legacy relative project paths retain their old
meaning; new project registrations and fresh seeds use canonical absolute paths.

## Payload revision 4: dependency-aware memoization

Payload magic `DAMON\0\x04\0` appends a bounded memo table after world state.
The table stores at most 4,096 flat dependency rows followed by at most 1,024
entries. A dependency is an entity ID and the entity version observed when the
result was computed. An entry contains a one-byte kind, stable u64 lookup key,
stable u64 external-state hash, dependency offset/count, saturating reuse count,
and a value of at most 64 KiB. The `(kind, key)` hash index is derived on load.

Lookup requires both the same external-state hash and unchanged versions for
every dependency. A mismatch removes the stale entry. Updating an entity eagerly
removes all entries that depend on it. Least-used entries are evicted first with
a deterministic kind/key tie break. Discovery hashes use explicit FNV-1a rather
than Rust's process-dependent hashing. Cached command values are decoded through
the deterministic tool allowlist; cache bytes cannot introduce a new executable
or argument. Revision-3 world payloads remain readable and acquire an empty memo
table when next saved.

## Payload revision 5: learned strategy statistics

Payload magic `DAMON\0\x05\0` appends at most 4,096 fixed-size strategy rows.
Each row stores a stable state hash, one-byte strategy ID, saturating success and
failure counts, accumulated latency milliseconds, cost units, model-call count,
confidence total, and risk total. The `(state hash, strategy)` lookup index is
derived on load. Eviction removes the least-observed row with stable hash/ID tie
breaking.

Scores use simple smoothed success ratios and bounded latency, cost, confidence,
and risk terms. Fixed base tiers preserve inspect/search/test/deterministic work
ahead of local models, external free models, clarification, and paid cloud use.
Statistics rank choices only: they cannot grant permission, create arguments, or
serve as verification. Revision-4 memo payloads remain readable and acquire an
empty strategy table when saved.

## Payload revision 6: native capability graph

Payload magic `DAMON\0\x06\0` appends bounded arrays for capabilities,
capability dependencies, and implementations. A capability has a compact ID,
canonical name, effects mask, and version. An implementation identifies its
capability, ordered kind (native through pixels), exactly one tool or procedure,
verification state, provenance, a dependency slice, and saturating verified
success/failure counts. Limits are 4,096 capabilities, 8,192 implementations,
and 16,384 dependency IDs.

Only `Verified` implementations resolve for execution. `Compiled` generated code
is deliberately insufficient. Resolution prefers native implementations before
compositions, system/library calls, protocols, generated code, structured
external interfaces, application references, accessibility, and pixels. This
ranking does not bypass effects policy or execution verification. Revision-5
strategy payloads remain readable and receive the canonical built-in capability
graph on migration.

## Payload revision 7: semantic registry contract

Payload magic `DAMON\0\x07\0` appends a u16 semantic-registry version after the
capability graph. The logical semantic IR is independent of this physical brain
layout; memory retains the registry version so learned mappings cannot silently
change meaning. Revision 6 remains readable. On migration Damon restores missing
built-in local-host/Desktop entities and verified native interface/route
implementations, then writes revision 7 on the next save or compaction.

Registry version 2 adds the stable passive-neighbor action ID without changing IR
version 1; older registry metadata migrates forward because IDs are never recycled.
Registry version 3 adds stable socket-inventory, network-diagnosis, copy, and
constraint-based find actions and predicates. Older registry metadata migrates
forward without changing existing IDs.
Model-facing entity slots and source spans are request-local and are never stored
as persistent-ID authority. Canonical IR uses deterministic CBOR for hashing and
interchange; `damon.data` continues to store learned bound graph templates and
count arrays in its explicit compact schema rather than embedding model JSON.

## Payload revision 8: durable network knowledge

Payload magic `DAMON\0\x08\0` appends a compact network topology after the
semantic-registry version. It stores at most 16,384 typed node identities and
65,536 sorted relationship facts. Identities encode machine, interface, address,
CIDR network, host, router, MAC, and transport-service values directly as
fixed-width bytes. Facts
encode compact node indexes, relation and provenance tags, integer confidence,
and first/last observation times.

The persistence boundary rejects `Observed` facts. Callers must first promote
verified stable knowledge to `Inferred`, `Configured`, or `Learned`; conversion
also prunes transient-only nodes. This prevents passive snapshots, socket buffers,
packet payloads, and current connections from silently becoming history. Revision
7 remains readable and starts with an empty durable topology before the next save.

## Payload revision 9: verified learned procedures

Payload magic `DAMON\0\x09\0` appends a bounded learned-procedure table after
durable network knowledge. Procedures are compact call/store/compare/branch/jump/
return instruction arrays compiled only from verified multi-action plans. Each
call names a capability rather than a shell command or executable. On reuse the
procedure is reconstructed through the normal capability-resolution, policy,
execution, and verification path.

Revision 8 remains readable and starts with an empty procedure table. The
migration suite exercises every payload revision through revision 9, including
the revision-8 topology boundary.

## Payload revision 10: exact policy approvals

Payload magic `DAMON\0\x0a\0` appends at most 4,096 sorted approval records after
the learned-procedure table. Each record stores a capability ID, effects bits,
optional target entity ID, stable scope hash, at most 64 KiB of canonical scope
bytes, capability implementation version, and revocation bit. Scope covers the
selected tool, target, argument count, and every length-prefixed argument.

An approval matches only the identical capability, effects, target, scope bytes,
and implementation version. The hash is only a lookup hint. Broader scope,
stronger effects, or a changed implementation requires another explicit confirmation. Model output cannot add
an approval. Revoked records remain non-authorizing. Revision 9 remains readable
and starts with an empty approval table before the next save.
