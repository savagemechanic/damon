use damon::{data::DamonData, types::IntentId};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "damon-storage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("damon.data")
    }
    fn side(&self, suffix: &str) -> PathBuf {
        self.0.join(format!("damon.data{suffix}"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn teach(d: &mut DamonData, key: u64) {
    d.observe_language(key, IntentId(3), 1, 200);
    d.save().unwrap();
}
#[test]
fn durable_journal_replay_and_exclusive_writer() {
    let f = Fixture::new();
    let mut d = DamonData::open(f.path()).unwrap();
    assert!(DamonData::open(f.path()).is_err());
    teach(&mut d, 42);
    let generation = d.generation();
    drop(d);
    let d = DamonData::open(f.path()).unwrap();
    assert_eq!(d.generation(), generation);
    assert_eq!(d.language_candidates(42), &[(IntentId(3), 1)]);
}
#[test]
fn every_truncated_record_boundary_recovers_prior_commit() {
    let f = Fixture::new();
    let mut d = DamonData::open(f.path()).unwrap();
    teach(&mut d, 42);
    let committed = fs::read(f.side(".journal")).unwrap();
    teach(&mut d, 43);
    let complete = fs::read(f.side(".journal")).unwrap();
    drop(d);
    for end in committed.len()..complete.len() {
        fs::write(f.side(".journal"), &complete[..end]).unwrap();
        let d = DamonData::open(f.path()).unwrap();
        assert_eq!(d.language_candidates(42), &[(IntentId(3), 1)]);
        assert!(d.language_candidates(43).is_empty());
        assert_eq!(
            fs::metadata(f.side(".journal")).unwrap().len(),
            committed.len() as u64
        );
    }
}
#[test]
fn corrupted_record_is_rejected_and_subsequent_commits_replay() {
    let f = Fixture::new();
    let mut d = DamonData::open(f.path()).unwrap();
    teach(&mut d, 1);
    let boundary = fs::metadata(f.side(".journal")).unwrap().len() as usize;
    teach(&mut d, 2);
    drop(d);
    let mut bytes = fs::read(f.side(".journal")).unwrap();
    bytes[boundary + 35] ^= 0x80;
    fs::write(f.side(".journal"), bytes).unwrap();
    let mut d = DamonData::open(f.path()).unwrap();
    assert!(!d.recovery_warnings().is_empty());
    assert!(d.language_candidates(2).is_empty());
    teach(&mut d, 3);
    drop(d);
    let d = DamonData::open(f.path()).unwrap();
    assert_eq!(d.language_candidates(3), &[(IntentId(3), 1)]);
}
#[test]
fn previous_snapshot_recovers_corrupted_primary() {
    let f = Fixture::new();
    let mut d = DamonData::open(f.path()).unwrap();
    teach(&mut d, 1);
    d.compact().unwrap();
    let previous = d.generation();
    teach(&mut d, 2);
    d.compact().unwrap();
    drop(d);
    fs::write(f.path(), b"corrupt").unwrap();
    let d = DamonData::open(f.path()).unwrap();
    assert_eq!(d.generation(), previous);
    assert_eq!(d.language_candidates(1), &[(IntentId(3), 1)]);
    assert!(!d.recovery_warnings().is_empty());
}
#[test]
fn no_valid_state_is_an_error_not_a_reset() {
    let f = Fixture::new();
    fs::write(f.path(), b"broken").unwrap();
    assert!(DamonData::open(f.path()).is_err());
    assert_eq!(fs::read(f.path()).unwrap(), b"broken");
}
#[test]
fn interrupted_snapshot_and_duplicate_journal_are_idempotent() {
    let f = Fixture::new();
    let mut d = DamonData::open(f.path()).unwrap();
    teach(&mut d, 7);
    let journal = fs::read(f.side(".journal")).unwrap();
    d.compact().unwrap();
    let generation = d.generation();
    drop(d);
    fs::write(f.side(".journal"), journal).unwrap();
    fs::write(f.side(".tmp"), b"incomplete").unwrap();
    let d = DamonData::open(f.path()).unwrap();
    assert_eq!(d.generation(), generation);
    assert_eq!(d.language_candidates(7), &[(IntentId(3), 1)]);
}
#[test]
fn portable_backup_restore_preserves_monotonic_generation() {
    let f = Fixture::new();
    let mut d = DamonData::open(f.path()).unwrap();
    teach(&mut d, 11);
    let backup = f.0.join("backup");
    d.export(&backup).unwrap();
    assert!(d.export(&backup).is_err());
    teach(&mut d, 12);
    let generation = d.generation();
    d.restore(&backup).unwrap();
    assert!(d.generation() > generation);
    assert!(d.language_candidates(12).is_empty());
    assert_eq!(d.language_candidates(11), &[(IntentId(3), 1)]);
    let other = Fixture::new();
    fs::copy(backup, other.path()).unwrap();
    let imported = DamonData::open(other.path()).unwrap();
    assert_eq!(imported.language_candidates(11), d.language_candidates(11));
}
#[test]
fn corrupt_backup_does_not_change_live_state() {
    let f = Fixture::new();
    let mut d = DamonData::open(f.path()).unwrap();
    teach(&mut d, 21);
    let p = f.0.join("backup");
    d.export(&p).unwrap();
    let mut bytes = fs::read(&p).unwrap();
    bytes[16] ^= 1;
    fs::write(&p, bytes).unwrap();
    let generation = d.generation();
    assert!(d.restore(&p).is_err());
    assert_eq!(d.generation(), generation);
    assert_eq!(d.language_candidates(21), &[(IntentId(3), 1)]);
}
#[test]
fn migrates_tracked_v1_fixture() {
    let f = Fixture::new();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1-seed.data"),
        f.path(),
    )
    .unwrap();
    let mut d = DamonData::open(f.path()).unwrap();
    assert!(d.resolve("damon").is_some());
    d.compact().unwrap();
    assert_eq!(&fs::read(f.path()).unwrap()[..8], b"DAMONIMG");
    drop(d);
    assert!(DamonData::open(f.path())
        .unwrap()
        .resolve("damon")
        .is_some());
}
#[test]
fn repeated_learning_and_unique_phrase_retention_are_bounded() {
    let f = Fixture::new();
    let mut d = DamonData::open(f.path()).unwrap();
    for _ in 0..10_000 {
        d.observe_language(1, IntentId(3), 1, 200);
    }
    for key in 2..10_000 {
        d.observe_language(key, IntentId(3), 1, 200);
    }
    assert!(d.experiences.len() <= 4096);
    assert!(d.language_counts.len() <= 8192);
    assert_eq!(d.language_candidates(1), &[(IntentId(3), 10_000)]);
    d.compact().unwrap();
    assert!(fs::metadata(f.path()).unwrap().len() < 512_000);
    assert_eq!(fs::metadata(f.side(".journal")).unwrap().len(), 0);
    drop(d);
    let d = DamonData::open(f.path()).unwrap();
    assert_eq!(d.language_candidates(1), &[(IntentId(3), 10_000)]);
}
#[test]
fn hostile_lengths_fail_without_large_allocation() {
    let f = Fixture::new();
    let mut bytes = b"DAMON\0\x01\0".to_vec();
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    fs::write(f.path(), bytes).unwrap();
    assert!(DamonData::open(f.path()).is_err());
}

#[test]
fn unrecoverable_initial_journal_is_never_silently_reset() {
    let f = Fixture::new();
    fs::write(f.side(".journal"), b"broken initial transaction").unwrap();
    for _ in 0..2 {
        assert!(DamonData::open(f.path()).is_err());
    }
    assert!(!f.path().exists());
}

#[test]
fn natural_language_memory_operations_use_real_files() {
    let f = Fixture::new();
    let mut runtime = damon::Damon {
        data: DamonData::open(f.path()).unwrap(),
        models: Default::default(),
        policy: Default::default(),
    };
    let backup = f.0.join("Memory Backup.data");
    assert_eq!(
        runtime.handle(&format!("back up my memory to \"{}\"", backup.display())),
        "Memory backup saved."
    );
    assert!(backup.exists());
    assert!(runtime
        .handle("compact my memory")
        .starts_with("Memory compacted"));
    assert_eq!(
        runtime.handle(&format!("restore my memory from \"{}\"", backup.display())),
        "Memory restored from a verified backup."
    );
    assert!(runtime
        .handle("show memory status")
        .contains("Memory generation"));
}

#[test]
fn teacher_answers_are_not_learned_when_execution_fails() {
    let f = Fixture::new();
    let mut models = damon::model::ModelRouter::default();
    models.ollama_model.clear();
    models.external_command = Some("printf 'N action 1\\nN entity 0\\nE 0 target 1\\n'".into());
    models.allow_cloud = false;
    let mut data = DamonData::open(f.path()).unwrap();
    data.entities[0].value = f.0.join("not-a-repository").to_string_lossy().into_owned();
    let mut runtime = damon::Damon {
        data,
        models,
        policy: Default::default(),
    };
    let response = runtime.handle("inspect the frobnicator");
    assert!(response.contains("failed"));
    assert!(runtime
        .data
        .language_candidates(damon::language::feature_hash("inspect the frobnicator"))
        .is_empty());
    assert_eq!(runtime.data.experiences.last().unwrap().reward, -1);
}

#[test]
fn corrupt_legacy_primary_cannot_replace_known_good_previous() {
    let f = Fixture::new();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1-seed.data");
    fs::copy(fixture, f.side(".prev")).unwrap();
    fs::write(f.path(), b"DAMON\0\x01\0truncated").unwrap();
    let mut d = DamonData::open(f.path()).unwrap();
    d.compact().unwrap();
    drop(d);
    fs::write(f.path(), b"corrupt again").unwrap();
    assert!(DamonData::open(f.path())
        .unwrap()
        .resolve("damon")
        .is_some());
}
