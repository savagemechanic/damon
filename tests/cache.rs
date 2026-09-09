use damon::{cache, data::DamonData, tools};
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
            "damon-cache-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn brain(&self) -> DamonData {
        DamonData::open(self.0.join("brain.data")).unwrap()
    }

    fn project(&self) -> PathBuf {
        let path = self.0.join("project");
        fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn discovery_plan_is_reused_persisted_and_invalidated_by_manifest_state() {
    let fixture = Fixture::new();
    let project_path = fixture.project();
    fs::write(
        project_path.join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.0.0'\n",
    )
    .unwrap();
    let mut data = fixture.brain();
    let project = data.register_project("fixture", &project_path).unwrap();

    let first = tools::cached_verification_plan(&mut data, project).unwrap();
    assert_eq!(first.len(), 3);
    assert_eq!(data.memo.entries.len(), 1);
    assert_eq!(data.memo.entries[0].hits, 0);
    let second = tools::cached_verification_plan(&mut data, project).unwrap();
    assert_eq!(second, first);
    assert_eq!(data.memo.entries[0].hits, 1);
    data.save().unwrap();
    drop(data);

    let mut data = fixture.brain();
    assert_eq!(data.memo.entries.len(), 1);
    let _ = tools::cached_verification_plan(&mut data, project).unwrap();
    assert_eq!(data.memo.entries[0].hits, 2);

    fs::remove_file(project_path.join("Cargo.toml")).unwrap();
    fs::write(
        project_path.join("pyproject.toml"),
        "[tool.pytest.ini_options]\n",
    )
    .unwrap();
    let changed = tools::cached_verification_plan(&mut data, project).unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].program, "python3");
    assert_eq!(data.memo.entries[0].hits, 0);
}

#[test]
fn entity_version_invalidates_every_dependent_entry() {
    let fixture = Fixture::new();
    let project_path = fixture.project();
    fs::write(project_path.join("Cargo.toml"), "[workspace]\n").unwrap();
    let mut data = fixture.brain();
    let project = data.register_project("fixture", &project_path).unwrap();
    tools::cached_verification_plan(&mut data, project).unwrap();
    data.memo
        .put(
            cache::FILE_LIST,
            cache::key_for_project(project),
            7,
            &[project],
            "a.rs".into(),
            &data.entities,
        )
        .unwrap();
    assert_eq!(data.memo.entries.len(), 2);
    data.update_entity(project, fixture.0.to_string_lossy().as_ref())
        .unwrap();
    assert!(data.memo.entries.is_empty());
}

#[test]
fn file_inventory_is_cached_and_populates_file_world_records() {
    let fixture = Fixture::new();
    let project_path = fixture.project();
    fs::write(project_path.join("alpha.rs"), "fn alpha() {}\n").unwrap();
    let mut data = fixture.brain();
    let project = data.register_project("fixture", &project_path).unwrap();
    let mut action =
        tools::action_for_capability(damon::capability::LIST_FILES, project, &data).unwrap();
    tools::prepare(&mut action, &mut data).unwrap();
    assert_eq!(action.args[1], "alpha.rs");
    assert!(data.resolve("fixture:alpha.rs").is_some());
    assert_eq!(data.memo.entries.len(), 1);

    let mut second =
        tools::action_for_capability(damon::capability::LIST_FILES, project, &data).unwrap();
    tools::prepare(&mut second, &mut data).unwrap();
    assert_eq!(data.memo.entries[0].hits, 1);

    fs::write(project_path.join("beta.rs"), "fn beta() {}\n").unwrap();
    let mut changed =
        tools::action_for_capability(damon::capability::LIST_FILES, project, &data).unwrap();
    tools::prepare(&mut changed, &mut data).unwrap();
    assert!(changed.args[1].contains("beta.rs"));
    assert_eq!(data.memo.entries[0].hits, 0);
}
