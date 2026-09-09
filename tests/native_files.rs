use damon::{data::DamonData, Damon};
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
            "damon-native-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn canonical_spans_and_constraints_lower_to_verified_native_file_tools() {
    let fixture = Fixture::new();
    let project_path = fixture.0.join("demo");
    let desktop_path = fixture.0.join("desktop");
    fs::create_dir(&project_path).unwrap();
    fs::create_dir(&desktop_path).unwrap();
    fs::write(project_path.join("parser.rs"), "fn parser() {}\n").unwrap();
    let large = fs::File::create(project_path.join("large.bin")).unwrap();
    large.set_len(10_000_001).unwrap();

    let mut data = DamonData::open(fixture.0.join("brain.data")).unwrap();
    let project = data.register_project("Demo", &project_path).unwrap();
    let desktop = data.resolve("desktop").unwrap();
    data.update_entity(desktop, desktop_path.to_str().unwrap())
        .unwrap();
    data.world.context.focus(project);
    data.save().unwrap();
    let mut runtime = Damon {
        data,
        models: Default::default(),
        policy: Default::default(),
    };

    let copied = runtime.handle("copy parser.rs from Demo to my desktop");
    assert!(copied.contains("Copied and verified"), "{copied}");
    assert_eq!(
        fs::read(desktop_path.join("parser.rs")).unwrap(),
        b"fn parser() {}\n"
    );

    let found = runtime.handle("find files larger than 10 MB");
    assert!(found.contains("large.bin"), "{found}");
    assert!(!found.contains("parser.rs"));
}
