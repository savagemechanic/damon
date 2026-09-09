use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn isolated_directory(label: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "damon-chat-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    directory
}

#[test]
fn gui_bridge_returns_one_json_response_per_request() {
    let directory = isolated_directory("responses");
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
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("secret-test-key"));
    let events = stdout
        .lines()
        .map(|line| damon::json::parse(line).unwrap())
        .collect::<Vec<_>>();
    let responses = events
        .iter()
        .filter(|event| event.get("type").and_then(damon::json::Value::as_str) == Some("response"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 3);
    assert!(responses[0]
        .get("response")
        .and_then(damon::json::Value::as_str)
        .unwrap()
        .starts_with("Memory generation "));
    let wifi = responses[1]
        .get("response")
        .and_then(damon::json::Value::as_str)
        .unwrap();
    assert!(!wifi.contains("teacher"));
    assert!(!wifi.contains("Ollama"));
    let unknown = responses[2]
        .get("response")
        .and_then(damon::json::Value::as_str)
        .unwrap();
    assert!(unknown.contains("Try rephrasing"));
    assert!(!unknown.contains("provider failed"));
    assert!(responses.iter().all(|response| !response
        .get("response")
        .and_then(damon::json::Value::as_str)
        .unwrap()
        .contains("ToolId")));
    let states = events
        .iter()
        .filter_map(|event| event.get("state").and_then(damon::json::Value::as_str))
        .collect::<Vec<_>>();
    assert!(states.contains(&"processing"));
    assert!(states.contains(&"ready"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gui_bridge_accepts_a_secret_and_selects_a_free_zen_model_without_echoing_it() {
    let directory = isolated_directory("models");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut block = [0_u8; 512];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut block).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&block[..count]);
            }
            let body = r#"{"object":"list","data":[{"id":"paid-model"},{"id":"big-pickle"},{"id":"smart-free"}]}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_damon"))
        .arg("--chat-stdio")
        .env("DAMON_DATA", directory.join("brain.data"))
        .env("DAMON_ZEN_BASE_URL", format!("http://{address}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            b"{\"type\":\"set_api_key\",\"key\":\"secret-test-key\"}\n{\"type\":\"list_models\"}\n{\"type\":\"select_model\",\"model\":\"smart-free\"}\n",
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    server.join().unwrap();
    assert!(output.status.success());
    let events = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| damon::json::parse(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        events[0].get("type").and_then(damon::json::Value::as_str),
        Some("api_key")
    );
    assert_eq!(
        events[1].get("type").and_then(damon::json::Value::as_str),
        Some("models")
    );
    assert_eq!(
        events[2].get("type").and_then(damon::json::Value::as_str),
        Some("model_selected")
    );
    assert_eq!(
        events[2].get("ok").and_then(damon::json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        events[2].get("model").and_then(damon::json::Value::as_str),
        Some("smart-free")
    );
    assert!(!String::from_utf8(output.stderr)
        .unwrap()
        .contains("secret-test-key"));
    assert!(!std::fs::read(directory.join("brain.data"))
        .unwrap()
        .windows(b"secret-test-key".len())
        .any(|bytes| bytes == b"secret-test-key"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn one_meaning_request_makes_at_most_two_provider_calls() {
    let directory = isolated_directory("attempts");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicU64::new(0));
    let observed = Arc::clone(&calls);
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut block = [0_u8; 512];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut block).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&block[..count]);
            }
            observed.fetch_add(1, Ordering::Relaxed);
            let body = r#"{"choices":[{"message":{"content":"{}"}}]}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_damon"))
        .arg("--chat-stdio")
        .env("DAMON_DATA", directory.join("brain.data"))
        .env("DAMON_OPENCODE_API_KEY", "secret-test-key")
        .env("DAMON_ZEN_BASE_URL", format!("http://{address}"))
        .env("DAMON_OLLAMA_MODEL", "")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"invent an entirely unknown operation\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    server.join().unwrap();
    assert!(output.status.success());
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let states = String::from_utf8(output.stdout).unwrap();
    assert_eq!(states.matches("\"state\":\"asking_cloud\"").count(), 2);
    assert_eq!(states.matches("\"type\":\"response\"").count(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}
