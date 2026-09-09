use damon::{
    data::DamonData,
    graph::NodeKind,
    language::{self, Interpretation},
    semantics::{self, Relation},
    types::{Action, Effects, ToolId},
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
            "damon-semantics-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn data(&self) -> DamonData {
        let mut d = DamonData::open(self.0.join("brain.data")).unwrap();
        d.entities[0].value = self.0.to_string_lossy().into_owned();
        d
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn lexical_graph_has_real_nodes_and_relations() {
    let f = Fixture::new();
    let d = f.data();
    let Interpretation::Resolved(m) = language::understand("run the tests in Damon", &d) else {
        panic!()
    };
    semantics::validate(&m, &d).unwrap();
    assert!(m
        .nodes
        .iter()
        .any(|n| n.kind == NodeKind::Concept && n.value == 1));
    assert!(m
        .edges
        .iter()
        .any(|e| e.relation == Relation::Object as u16));
    let p = damon::reason::plan(&m, &d).unwrap();
    assert_eq!(p.actions[0].tool, ToolId(3));
}
#[test]
fn conditional_request_preserves_both_actions_and_dependency() {
    let f = Fixture::new();
    let d = f.data();
    let Interpretation::Resolved(m) =
        language::understand("run the tests and if they pass show me the diff", &d)
    else {
        panic!()
    };
    semantics::validate(&m, &d).unwrap();
    let p = damon::reason::plan(&m, &d).unwrap();
    assert_eq!(p.actions.len(), 2);
    assert!(p.dependencies[0].success_required);
    assert_eq!(p.dependencies[0].previous, 0);
    assert_eq!(p.actions[1].tool, ToolId(2));
}
#[test]
fn yesterday_is_a_time_node_not_a_plain_diff() {
    let f = Fixture::new();
    let d = f.data();
    let Interpretation::Resolved(m) =
        language::understand("show me the files I changed yesterday", &d)
    else {
        panic!()
    };
    semantics::validate(&m, &d).unwrap();
    assert_eq!(m.intent, language::INTENT_CHANGED_FILES);
    assert!(m.nodes.iter().any(|n| n.kind == NodeKind::Time));
}

#[test]
fn network_questions_resolve_to_native_host_capabilities() {
    let f = Fixture::new();
    let d = f.data();
    let host = d.resolve("local host").unwrap();
    for (request, intent, tool, capability) in [
        (
            "What network am I connected to?",
            language::INTENT_NETWORK_INTERFACES,
            damon::tools::TOOL_NETWORK_INTERFACES,
            damon::capability::INSPECT_INTERFACES,
        ),
        (
            "What is my default gateway?",
            language::INTENT_DEFAULT_GATEWAY,
            damon::tools::TOOL_NETWORK_ROUTES,
            damon::capability::INSPECT_ROUTES,
        ),
        (
            "Show me the devices my Mac currently knows about.",
            language::INTENT_NETWORK_NEIGHBORS,
            damon::tools::TOOL_NETWORK_NEIGHBORS,
            damon::capability::INSPECT_NEIGHBORS,
        ),
        (
            "Why is the internet not working?",
            language::INTENT_NETWORK_DIAGNOSIS,
            damon::tools::TOOL_NETWORK_DIAGNOSE,
            damon::capability::DIAGNOSE_NETWORK,
        ),
        (
            "What is my Mac talking to?",
            language::INTENT_LIST_SOCKETS,
            damon::tools::TOOL_LIST_SOCKETS,
            damon::capability::LIST_SOCKETS,
        ),
    ] {
        let Interpretation::Resolved(meaning) = language::understand(request, &d) else {
            panic!("network question was not resolved")
        };
        assert_eq!(meaning.intent, intent);
        assert_eq!(meaning.target, Some(host));
        semantics::validate_request(request, &meaning, &d).unwrap();
        let action = damon::reason::resolve(&meaning, &d).unwrap();
        assert_eq!(action.tool, tool);
        assert_eq!(action.capability, capability);
        assert!(action.args.is_empty());
        damon::policy::Policy::default().check(&action).unwrap();
    }
}

#[test]
fn network_observation_does_not_replace_project_focus() {
    let f = Fixture::new();
    let mut d = f.data();
    let project = d.world.context.focus.unwrap();
    let Interpretation::Resolved(meaning) = language::understand("show network interfaces", &d)
    else {
        panic!("network question was not resolved")
    };
    damon::learning::observe_verified(
        &mut d,
        language::feature_hash("show network interfaces"),
        &meaning,
        true,
    );
    assert_eq!(d.world.context.focus, Some(project));
    assert_eq!(d.world.context.previous_target, meaning.target);
    let Interpretation::Resolved(coding) = language::understand("run the tests", &d) else {
        panic!("coding request lost project focus")
    };
    assert_eq!(coding.target, Some(project));
}
#[test]
fn forged_effects_do_not_bypass_policy() {
    let action = Action {
        capability: damon::capability::RUN_TESTS,
        tool: ToolId(3),
        implementation_version: 1,
        target: None,
        effects: Effects::NONE,
        args: vec![],
    };
    assert!(damon::policy::Policy::default().check(&action).is_err());
    assert!(!damon::tools::execute(&action, &Default::default()).success);
}
#[test]
fn verified_composite_graph_survives_reopen_without_losing_steps() {
    let f = Fixture::new();
    let mut d = f.data();
    let Interpretation::Resolved(m) =
        language::understand("list files and if they pass git status", &d)
    else {
        panic!()
    };
    let phrase = "inspect the workspace";
    let key = language::feature_hash(phrase);
    damon::learning::observe_verified(&mut d, key, &m, true);
    d.save().unwrap();
    drop(d);
    let d = DamonData::open(f.0.join("brain.data")).unwrap();
    let Interpretation::Resolved(learned) = language::understand(phrase, &d) else {
        panic!()
    };
    let plan = damon::reason::plan(&learned, &d).unwrap();
    assert_eq!(plan.actions.len(), 2);
    assert!(plan.dependencies[0].success_required);
    assert_eq!(plan.actions[0].tool, ToolId(4));
    assert_eq!(plan.actions[1].tool, ToolId(1));
}
#[test]
fn failed_prerequisite_skips_dependent_tool() {
    let f = Fixture::new();
    // No manifest exists, so test discovery deterministically fails. The diff
    // action would fail differently if it were accidentally executed.
    let mut runtime = damon::Damon {
        data: f.data(),
        models: Default::default(),
        policy: Default::default(),
    };
    let response = runtime.handle("run tests and if they pass show diff");
    assert!(response.contains("no supported test runner"));
    assert!(response.contains("Skipped the dependent action"));
    assert!(!response.contains("not a git repository"));
    assert!(runtime.data.learned_graphs.is_empty());
}
#[test]
fn successful_teacher_graph_is_reused_with_providers_disabled() {
    let f = Fixture::new();
    let mut models = damon::model::ModelRouter::default();
    models.ollama_model.clear();
    models.allow_cloud = false;
    models.external_command = Some(
        "printf '%s' '{\"ir_version\":1,\"registry_version\":3,\"candidates\":[{\"nodes\":[{\"kind\":\"ACTION\",\"concept\":16777220,\"value\":0},{\"kind\":\"ENTITY\",\"concept\":67108865,\"value\":0}],\"edges\":[{\"source\":0,\"predicate\":33554433,\"target\":1}]}],\"unresolved_spans\":[]}'".into(),
    );
    let mut runtime = damon::Damon {
        data: f.data(),
        models,
        policy: Default::default(),
    };
    assert!(runtime
        .handle("inspect the frobnicator")
        .contains("brain.data"));
    runtime.models.external_command = None;
    assert!(runtime
        .handle("inspect the frobnicator")
        .contains("brain.data"));
}

#[test]
fn passing_real_tests_allow_the_real_diff() {
    let f = Fixture::new();
    fs::create_dir(f.0.join("src")).unwrap();
    fs::write(
        f.0.join("Cargo.toml"),
        "[package]\nname=\"damon-fixture\"\nversion=\"0.0.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    fs::write(
        f.0.join("src/lib.rs"),
        "#[test] fn passes() { assert_eq!(2+2,4); }\n",
    )
    .unwrap();
    for args in [vec!["init", "-q"], vec!["add", "src/lib.rs"]] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(&f.0)
            .status()
            .unwrap()
            .success());
    }
    fs::write(
        f.0.join("src/lib.rs"),
        "#[test] fn passes() { assert_eq!(2+2,4); }\n// verified change\n",
    )
    .unwrap();
    let mut runtime = damon::Damon {
        data: f.data(),
        models: Default::default(),
        policy: Default::default(),
    };
    let response = runtime.handle("run the tests and if they pass show me the diff");
    assert!(response.contains("The tests passed."), "{response}");
    assert!(response.contains("+// verified change"), "{response}");
    assert_eq!(runtime.data.learned_graphs.len(), 1);
}
