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
        .env("DAMON_OLLAMA_MODEL", "")
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
        .write_all(b"show memory status\nwhat wifi network am i connected to\nstart ollma\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 3);
    assert!(responses[0]["response"]
        .as_str()
        .unwrap()
        .starts_with("Memory generation "));
    let wifi = responses[1]["response"].as_str().unwrap();
    assert!(!wifi.contains("teacher"));
    assert!(!wifi.contains("Ollama"));
    let unknown = responses[2]["response"].as_str().unwrap();
    assert!(unknown.contains("Try rephrasing"));
    assert!(!unknown.contains("provider failed"));
    assert!(responses
        .iter()
        .all(|response| !response["response"].as_str().unwrap().contains("ToolId")));
    std::fs::remove_dir_all(directory).unwrap();
}
