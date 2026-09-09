use damon::{
    data::DamonData,
    strategy::{self, Outcome},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "damon-strategy-{}-{}.data",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn outcome(success: bool, latency_ms: u64) -> Outcome {
    Outcome {
        success,
        latency_ms,
        cost_units: 0,
        used_model: false,
        confidence: if success { 230 } else { 90 },
        risk: 10,
    }
}

#[test]
fn strategy_statistics_survive_reopen_and_rank_verified_history() {
    let path = path();
    let mut data = DamonData::open(&path).unwrap();
    for _ in 0..6 {
        data.strategies
            .observe(99, strategy::SEARCH, outcome(true, 5))
            .unwrap();
        data.strategies
            .observe(99, strategy::TEST, outcome(false, 2000))
            .unwrap();
    }
    data.save().unwrap();
    drop(data);

    let data = DamonData::open(&path).unwrap();
    assert_eq!(
        data.strategies
            .stat(99, strategy::SEARCH)
            .unwrap()
            .successes,
        6
    );
    assert_eq!(
        data.strategies
            .rank(99, &[strategy::TEST, strategy::SEARCH])[0]
            .0,
        strategy::SEARCH
    );
    drop(data);
    for suffix in ["", ".journal", ".prev", ".lock"] {
        let _ = fs::remove_file(format!("{}{}", path.display(), suffix));
    }
}

#[test]
fn real_runtime_records_tool_outcome_evidence() {
    let path = path();
    let data = DamonData::open(&path).unwrap();
    let mut runtime = damon::Damon {
        data,
        models: Default::default(),
        policy: Default::default(),
    };
    let feature = damon::language::feature_hash("list files in damon");
    assert!(!runtime.handle("list files in damon").contains("failed"));
    let stat = runtime
        .data
        .strategies
        .stat(feature, strategy::INSPECT)
        .unwrap();
    assert_eq!(stat.successes, 1);
    assert_eq!(stat.failures, 0);
    assert_eq!(stat.model_calls, 0);
    drop(runtime);
    for suffix in ["", ".journal", ".prev", ".lock"] {
        let _ = fs::remove_file(format!("{}{}", path.display(), suffix));
    }
}
