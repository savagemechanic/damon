use damon::{
    data::DamonData,
    language::{self, DeterministicProducer, Interpretation},
    semantic_ir::{self, ResolutionStatus, SemanticProducer},
    semantic_registry as registry,
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
        let path = std::env::temp_dir().join(format!(
            "damon-ir-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn data(&self) -> DamonData {
        DamonData::open(self.0.join("brain.data")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn run_tests_json(entity_slot: u16) -> String {
    format!(
        "{{\"ir_version\":1,\"registry_version\":1,\"candidates\":[{{\"nodes\":[{{\"kind\":\"ACTION\",\"concept\":{},\"value\":0}},{{\"kind\":\"ENTITY\",\"concept\":{},\"value\":{entity_slot}}}],\"edges\":[{{\"source\":0,\"predicate\":{},\"target\":1}}]}}],\"unresolved_spans\":[]}}",
        registry::RUN_TESTS.0,
        registry::PROJECT.0,
        registry::TARGET.0,
    )
}

#[test]
fn strict_json_rejects_schema_concepts_slots_edges_and_missing_arguments() {
    let fixture = Fixture::new();
    let data = fixture.data();
    let request = semantic_ir::request("run the tests in Damon", &data);
    assert_eq!(
        semantic_ir::parse_json(&run_tests_json(0), &request)
            .unwrap()
            .status,
        ResolutionStatus::Resolved
    );

    let extra = run_tests_json(0).replacen(
        "\"ir_version\":1",
        "\"unexpected\":true,\"ir_version\":1",
        1,
    );
    assert!(semantic_ir::parse_json(&extra, &request).is_err());
    assert!(semantic_ir::parse_json(&run_tests_json(99), &request).is_err());

    let invented = run_tests_json(0).replace(
        &registry::RUN_TESTS.0.to_string(),
        &(registry::ACTION_NAMESPACE | 999).to_string(),
    );
    assert!(semantic_ir::parse_json(&invented, &request).is_err());

    let invalid_edge = run_tests_json(0).replace("\"target\":1", "\"target\":99");
    assert!(semantic_ir::parse_json(&invalid_edge, &request).is_err());

    let missing = format!(
        "{{\"ir_version\":1,\"registry_version\":1,\"candidates\":[{{\"nodes\":[{{\"kind\":\"ACTION\",\"concept\":{},\"value\":0}}],\"edges\":[]}}],\"unresolved_spans\":[]}}",
        registry::RUN_TESTS.0
    );
    assert!(semantic_ir::parse_json(&missing, &request).is_err());
}

#[test]
fn canonical_bytes_are_stable_hashable_and_round_trip() {
    let fixture = Fixture::new();
    let data = fixture.data();
    let request = semantic_ir::request("run the tests in Damon", &data);
    let resolution = semantic_ir::parse_json(&run_tests_json(0), &request).unwrap();
    let ir = &resolution.candidates[0].ir;
    let first = semantic_ir::canonical_bytes(ir).unwrap();
    let second = semantic_ir::canonical_bytes(ir).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        semantic_ir::canonical_hash(ir).unwrap(),
        semantic_ir::canonical_hash(ir).unwrap()
    );
    assert_eq!(
        semantic_ir::decode_canonical(&first, &request).unwrap(),
        *ir
    );

    let mut wrong_version = first.clone();
    wrong_version[1] = 2;
    assert!(semantic_ir::decode_canonical(&wrong_version, &request).is_err());
    let mut trailing = first;
    trailing.push(0);
    assert!(semantic_ir::decode_canonical(&trailing, &request).is_err());
}

#[test]
fn deterministic_producer_covers_required_meaning_examples() {
    let fixture = Fixture::new();
    let data = fixture.data();
    for input in [
        "run the tests in Damon",
        "show me what changed",
        "copy parser.rs from Damon to my desktop",
        "find files larger than 10 MB",
        "run the tests and if they pass show me the diff",
    ] {
        let request = semantic_ir::request(input, &data);
        let resolution = DeterministicProducer { data: &data }.resolve(&request);
        assert_eq!(resolution.status, ResolutionStatus::Resolved, "{input}");
        semantic_ir::validate_structural(&resolution.candidates[0].ir, input).unwrap();
        semantic_ir::validate_semantic(&resolution.candidates[0].ir, &request).unwrap();
    }
}

#[test]
fn source_spans_bind_to_user_text_without_invented_paths() {
    let fixture = Fixture::new();
    let data = fixture.data();
    let input = "copy parser.rs from Damon to my desktop";
    let request = semantic_ir::request(input, &data);
    let resolution = DeterministicProducer { data: &data }.resolve(&request);
    let bound = semantic_ir::bind_context(&resolution.candidates[0].ir, &request, &data).unwrap();
    let span = bound
        .entities
        .iter()
        .find_map(|(_, binding)| match binding {
            semantic_ir::EntityBinding::SourceSpan(span) => Some(*span),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        &input[usize::from(span.start)..usize::from(span.end)],
        "parser.rs"
    );
}

#[test]
fn prompt_exposes_meaning_but_not_implementations() {
    let fixture = Fixture::new();
    let data = fixture.data();
    let request = semantic_ir::request("run tests in Damon", &data);
    let full = semantic_ir::prompt(&request, false);
    let compact = semantic_ir::prompt(&request, true);
    for prompt in [&full, &compact] {
        assert!(prompt.contains(&registry::RUN_TESTS.0.to_string()));
        assert!(!prompt.contains("cargo"));
        assert!(!prompt.contains("pytest"));
        assert!(!prompt.contains("ToolId"));
    }
    assert!(compact.len() < full.len());
}

#[test]
fn repeat_rebinds_through_slots_and_ambiguous_projects_are_rejected() {
    let fixture = Fixture::new();
    let mut data = fixture.data();
    let cpython = data.add_entity(damon::world::PROJECT, "cpython", "/tmp/cpython");
    let Interpretation::Resolved(first) = language::understand("run the tests in Damon", &data)
    else {
        panic!()
    };
    damon::learning::observe_verified(
        &mut data,
        language::feature_hash("run the tests in Damon"),
        &first,
        true,
    );
    let Interpretation::Resolved(repeated) =
        language::understand("do the same thing to CPython", &data)
    else {
        panic!()
    };
    assert_eq!(repeated.target, Some(cpython));
    assert!(damon::reference::target("run tests in Damon to CPython", &data).is_err());
}

#[test]
fn multiple_valid_model_candidates_remain_ambiguous() {
    let fixture = Fixture::new();
    let data = fixture.data();
    let request = semantic_ir::request("run the tests in Damon", &data);
    let object: serde_json::Value = serde_json::from_str(&run_tests_json(0)).unwrap();
    let one = object["candidates"][0].clone();
    let json = serde_json::json!({
        "ir_version": 1,
        "registry_version": 1,
        "candidates": [one.clone(), one],
        "unresolved_spans": []
    })
    .to_string();
    assert_eq!(
        semantic_ir::parse_json(&json, &request).unwrap().status,
        ResolutionStatus::Ambiguous
    );
}

#[test]
fn damon_ranks_candidates_from_context_not_model_confidence() {
    let fixture = Fixture::new();
    let mut data = fixture.data();
    let project = data.resolve("damon").unwrap();
    data.world.context.observe(
        project,
        language::INTENT_RUN_TESTS,
        language::feature_hash("run tests"),
    );
    let request = semantic_ir::request("run tests or show diff", &data);
    let candidate = |action: u32| {
        serde_json::json!({
            "nodes": [
                {"kind": "ACTION", "concept": action, "value": 0},
                {"kind": "ENTITY", "concept": registry::PROJECT.0, "value": 0}
            ],
            "edges": [
                {"source": 0, "predicate": registry::TARGET.0, "target": 1}
            ]
        })
    };
    let json = serde_json::json!({
        "ir_version": 1,
        "registry_version": 1,
        "candidates": [candidate(registry::SHOW_DIFF.0), candidate(registry::RUN_TESTS.0)],
        "unresolved_spans": []
    })
    .to_string();
    let resolution = semantic_ir::parse_json(&json, &request).unwrap();
    assert_eq!(resolution.status, ResolutionStatus::Resolved);
    assert_eq!(
        resolution.candidates[0].ir.nodes[0].concept,
        registry::RUN_TESTS.0
    );
}
