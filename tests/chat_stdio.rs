use std::{
    io::Write,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn gui_bridge_returns_one_json_response_per_request() {
    let directory = std::env::temp_dir().join(format!(
        "damon-chat-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_damon"))
        .arg("--chat-stdio")
        .env("DAMON_DATA", directory.join("brain.data"))
        .current_dir(&directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"show memory status\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let response = line["response"].as_str().unwrap();
    assert!(response.starts_with("Memory generation "));
    assert!(!response.contains("ToolId"));
    std::fs::remove_dir_all(directory).unwrap();
}
