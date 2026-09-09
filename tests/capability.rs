use damon::{
    capability::{self, ImplementationKind, NewImplementation, Provenance, Verification},
    data::DamonData,
    types::ToolId,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "damon-capability-{}-{}.data",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn capability_graph_and_provenance_survive_reopen() {
    let path = path();
    let mut data = DamonData::open(&path).unwrap();
    let reference = data
        .capabilities
        .add_implementation(
            NewImplementation {
                capability: capability::INSPECT_NEIGHBORS,
                kind: ImplementationKind::ApplicationReference,
                tool: Some(ToolId(900)),
                procedure: None,
                verification: Verification::Verified,
                provenance: Provenance::ApplicationReference,
            },
            &[],
        )
        .unwrap();
    data.capabilities.observe(reference, true).unwrap();
    data.save().unwrap();
    drop(data);

    let data = DamonData::open(&path).unwrap();
    let implementation = data
        .capabilities
        .resolve(capability::INSPECT_NEIGHBORS)
        .unwrap();
    assert_eq!(
        implementation.kind,
        ImplementationKind::ApplicationReference
    );
    assert_eq!(implementation.provenance, Provenance::ApplicationReference);
    assert_eq!(implementation.successes, 1);
    drop(data);
    for suffix in ["", ".journal", ".prev", ".lock"] {
        let _ = fs::remove_file(format!("{}{}", path.display(), suffix));
    }
}

#[test]
fn planning_resolves_semantics_through_capability_to_native_tool() {
    let path = path();
    let data = DamonData::open(&path).unwrap();
    let meaning = match damon::language::understand("run tests in Damon", &data) {
        damon::language::Interpretation::Resolved(meaning) => meaning,
        other => panic!("unexpected interpretation: {other:?}"),
    };
    let action = &damon::reason::plan(&meaning, &data).unwrap().actions[0];
    assert_eq!(action.capability, capability::RUN_TESTS);
    assert_eq!(action.tool, ToolId(3));
    drop(data);
    for suffix in ["", ".journal", ".prev", ".lock"] {
        let _ = fs::remove_file(format!("{}{}", path.display(), suffix));
    }
}
