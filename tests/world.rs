use damon::{
    data::DamonData,
    language::{self, Interpretation},
    types::{EntityId, IntentId},
    world::{self, Relationship},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "damon-world-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn data(&self) -> DamonData {
        DamonData::open(self.0.join("brain.data")).unwrap()
    }
    fn project(&self, name: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::create_dir(&p).unwrap();
        p
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn resolved(input: &str, data: &DamonData) -> damon::types::MeaningGraph {
    match language::understand(input, data) {
        Interpretation::Resolved(m) => m,
        other => panic!("{input}: {other:?}"),
    }
}
#[test]
fn world_records_aliases_offsets_and_context_survive_reopen() {
    let f = Fixture::new();
    let mut d = f.data();
    let project = d
        .register_project("CPython", &f.project("cpython"))
        .unwrap();
    d.add_alias("python core", project).unwrap();
    let file = d.add_entity(world::FILE, "parser.rs", "src/parser.rs");
    d.link(project, Relationship::Contains, file).unwrap();
    d.link(project, Relationship::Contains, file).unwrap();
    d.world.context.observe(project, IntentId(3), 42);
    d.save().unwrap();
    drop(d);
    let d = f.data();
    assert_eq!(d.resolve("python core"), Some(project));
    assert_eq!(d.world.neighbors(project).len(), 1);
    assert_eq!(d.world.neighbors(project)[0].target, file);
    assert_eq!(d.world.context.focus, Some(project));
    assert_eq!(d.world.context.previous_feature, Some(42));
}
#[test]
fn same_thing_retargets_all_steps_and_keeps_the_condition() {
    let f = Fixture::new();
    let mut d = f.data();
    let other = d
        .register_project("CPython", &f.project("cpython"))
        .unwrap();
    let request = "run tests and if they pass show diff";
    let m = resolved(request, &d);
    damon::learning::observe_verified(&mut d, language::feature_hash(request), &m, true);
    let repeated = resolved("do the same thing to CPython", &d);
    assert_eq!(repeated.target, Some(other));
    let plan = damon::reason::plan(&repeated, &d).unwrap();
    assert_eq!(plan.actions.len(), 2);
    assert!(plan.actions.iter().all(|a| a.target == Some(other)));
    assert!(plan.dependencies[0].success_required);
}
#[test]
fn learned_contextual_request_follows_new_focus() {
    let f = Fixture::new();
    let mut d = f.data();
    let other = d
        .register_project("CPython", &f.project("cpython"))
        .unwrap();
    let m = resolved("show me what changed", &d);
    damon::learning::observe_verified(
        &mut d,
        language::feature_hash("show me what changed"),
        &m,
        true,
    );
    d.world.context.focus(other);
    assert_eq!(resolved("show me what changed", &d).target, Some(other));
    for input in [
        "check it",
        "show its git status",
        "list files there",
        "show files in that",
        "run tests in this",
        "run tests in the previous project",
    ] {
        let m = resolved(input, &d);
        assert_eq!(
            m.target,
            if input.contains("previous") {
                Some(EntityId(0))
            } else {
                Some(other)
            }
        );
    }
}
#[test]
fn unknown_projects_and_missing_context_require_clarification() {
    let f = Fixture::new();
    let mut d = f.data();
    assert!(matches!(
        language::understand("run tests in NeverRegistered", &d),
        Interpretation::Clarify(_)
    ));
    d.world.context = Default::default();
    assert!(matches!(
        language::understand("check it", &d),
        Interpretation::Clarify(_)
    ));
    assert!(matches!(
        language::understand("do the same thing", &d),
        Interpretation::Clarify(_)
    ));
}
#[test]
fn names_and_aliases_cannot_silently_rebind() {
    let f = Fixture::new();
    let mut d = f.data();
    let other = d
        .register_project("CPython", &f.project("cpython"))
        .unwrap();
    assert!(d.add_alias("damon", other).is_err());
    assert!(d.add_alias("it", other).is_err());
    assert!(d.register_project("damon", &f.0).is_err());
    assert!(d.add_alias("unknown", EntityId(99999)).is_err());
    let v = d.entity(other).unwrap().version;
    d.update_entity(other, "new-value").unwrap();
    assert_eq!(d.entity(other).unwrap().version, v + 1);
    let world_version = d.world.version;
    d.update_entity(other, "new-value").unwrap();
    assert_eq!(d.world.version, world_version);
}
#[test]
fn second_clause_inherits_first_project_not_stale_focus() {
    let f = Fixture::new();
    let mut d = f.data();
    let other = d
        .register_project("CPython", &f.project("cpython"))
        .unwrap();
    d.world.context.focus(other);
    let m = resolved("run tests in Damon and if they pass show diff", &d);
    assert!(damon::reason::plan(&m, &d)
        .unwrap()
        .actions
        .iter()
        .all(|a| a.target == Some(EntityId(0))));
}
#[test]
fn natural_project_registration_and_alias_use_are_persistent() {
    let f = Fixture::new();
    let project = f.project("cpython");
    fs::write(project.join("marker"), "fixture").unwrap();
    let mut runtime = damon::Damon {
        data: f.data(),
        models: Default::default(),
        policy: Default::default(),
    };
    assert!(runtime
        .handle(&format!(
            "remember project CPython at \"{}\"",
            project.display()
        ))
        .starts_with("I know cpython"));
    assert!(runtime
        .handle("remember alias py for CPython")
        .contains("refers to"));
    assert!(runtime.handle("list files in py").contains("marker"));
    drop(runtime);
    let d = f.data();
    assert_eq!(d.world.context.focus, d.resolve("py"));
}
#[test]
fn corrupted_relationships_and_context_are_rejected() {
    let f = Fixture::new();
    let mut d = f.data();
    d.world.links.push(world::Link {
        source: EntityId(99999),
        relation: 1,
        target: EntityId(0),
    });
    assert!(d.world.rebuild(d.entities.len()).is_err());
    d.world.links.pop();
    d.world.context.focus = Some(EntityId(99999));
    assert!(d.world.rebuild(d.entities.len()).is_err());
}
#[test]
fn v2_fixture_migrates_to_world_payload() {
    let f = Fixture::new();
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v2-seed.data"),
        f.0.join("brain.data"),
    )
    .unwrap();
    let mut d = f.data();
    assert_eq!(d.resolve("damon"), Some(EntityId(0)));
    assert!(d.world.aliases.is_empty());
    d.add_alias("old project", EntityId(0)).unwrap();
    d.save().unwrap();
    drop(d);
    assert_eq!(f.data().resolve("old project"), Some(EntityId(0)));
}

#[test]
fn invalid_world_cannot_be_committed() {
    let f = Fixture::new();
    let mut d = f.data();
    let generation = d.generation();
    d.world.context.focus = Some(EntityId(99999));
    assert!(d.save().is_err());
    assert_eq!(d.generation(), generation);
    drop(d);
    assert!(f.data().world.context.focus.is_some());
}
