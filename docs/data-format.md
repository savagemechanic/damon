# Damon binary state, version 2

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
