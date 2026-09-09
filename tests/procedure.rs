use damon::{
    capability,
    data::DamonData,
    reason::{Dependency, Plan},
    tools,
};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn verified_composite_procedure_persists_and_reenters_the_normal_plan_path() {
    let directory = std::env::temp_dir().join(format!(
        "damon-procedure-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("brain.data");
    let mut data = DamonData::open(&path).unwrap();
    let target = data.resolve("damon").unwrap();
    data.update_entity(target, directory.to_str().unwrap())
        .unwrap();
    let plan = Plan {
        actions: vec![
            tools::action_for_capability(capability::GIT_STATUS, target, &data).unwrap(),
            tools::action_for_capability(capability::GIT_DIFF, target, &data).unwrap(),
        ],
        dependencies: vec![Dependency {
            step: 1,
            previous: 0,
            success_required: true,
        }],
    };
    data.procedures.remember_verified(42, &plan).unwrap();
    data.save().unwrap();
    drop(data);

    let data = DamonData::open(&path).unwrap();
    let restored = data.procedures.plan(42, target, &data).unwrap().unwrap();
    assert_eq!(restored.actions.len(), 2);
    assert_eq!(restored.dependencies.len(), 1);
    assert!(restored.dependencies[0].success_required);
    assert_eq!(restored.actions[0].capability, capability::GIT_STATUS);
    assert_eq!(restored.actions[1].capability, capability::GIT_DIFF);
    drop(data);
    fs::remove_dir_all(directory).unwrap();
}
