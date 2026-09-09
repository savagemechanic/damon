use std::{
    cell::Cell,
    env,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    time::Duration,
};

const MAX_PROMPT_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Ollama,
    External,
    Cloud,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelEvent {
    AskingOllama,
    Thinking,
    Processing,
}

#[derive(Debug, Clone)]
pub struct RoutedResponse {
    pub provider: ProviderKind,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ModelRouter {
    pub ollama_model: String,
    pub ollama_host: String,
    pub external_command: Option<String>,
    pub cloud_command: Option<String>,
    pub allow_cloud: bool,
    pub cloud_call_limit: u32,
    cloud_calls: Cell<u32>,
}

impl Default for ModelRouter {
    fn default() -> Self {
        Self {
            ollama_model: env::var("DAMON_OLLAMA_MODEL").unwrap_or_else(|_| "qwen3:8b".into()),
            ollama_host: env::var("DAMON_OLLAMA_HOST").unwrap_or_else(|_| "127.0.0.1:11434".into()),
            external_command: env::var("DAMON_MODEL_COMMAND").ok(),
            cloud_command: env::var("DAMON_CLOUD_COMMAND").ok(),
            allow_cloud: env::var("DAMON_ALLOW_CLOUD")
                .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true")),
            cloud_call_limit: env::var("DAMON_CLOUD_CALL_LIMIT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1),
            cloud_calls: Cell::new(0),
        }
    }
}

impl ModelRouter {
    pub fn infer(&self, prompt: &str) -> Result<RoutedResponse, String> {
        self.infer_validated_with_events(prompt, |_| Ok(()), |_| {})
    }

    pub fn infer_validated(
        &self,
        prompt: &str,
        validate: impl Fn(&str) -> Result<(), String>,
    ) -> Result<RoutedResponse, String> {
        self.infer_validated_with_events(prompt, validate, |_| {})
    }

    pub fn infer_validated_with_events(
        &self,
        prompt: &str,
        validate: impl Fn(&str) -> Result<(), String>,
        mut event: impl FnMut(ModelEvent),
    ) -> Result<RoutedResponse, String> {
        self.infer_with_format(prompt, "\"json\"", validate, &mut event)
    }

    pub fn infer_structured_validated_with_events(
        &self,
        prompt: &str,
        schema: &str,
        validate: impl Fn(&str) -> Result<(), String>,
        mut event: impl FnMut(ModelEvent),
    ) -> Result<RoutedResponse, String> {
        crate::json::parse(schema).map_err(|error| format!("invalid output schema: {error}"))?;
        self.infer_with_format(prompt, schema, validate, &mut event)
    }

    fn infer_with_format(
        &self,
        prompt: &str,
        format: &str,
        validate: impl Fn(&str) -> Result<(), String>,
        event: &mut impl FnMut(ModelEvent),
    ) -> Result<RoutedResponse, String> {
        if prompt.len() > MAX_PROMPT_BYTES {
            return Err("teacher context exceeds 64 KiB".into());
        }
        self.route(validate, |provider| match provider {
            ProviderKind::Ollama => {
                event(ModelEvent::AskingOllama);
                let result = self.ollama_chat(prompt, format, event);
                event(ModelEvent::Processing);
                result
            }
            ProviderKind::External | ProviderKind::Cloud => {
                let command = if provider == ProviderKind::External {
                    &self.external_command
                } else {
                    &self.cloud_command
                };
                let result = crate::process::run(
                    Path::new("."),
                    "sh",
                    &[
                        "-lc",
                        command.as_deref().ok_or("provider is not configured")?,
                    ],
                    Some(prompt),
                    Duration::from_secs(30),
                );
                if result.success {
                    Ok(result.stdout)
                } else {
                    Err(format!(
                        "provider failed (exit {:?}): {}",
                        result.code,
                        result.stderr.trim()
                    ))
                }
            }
        })
    }

    pub fn ollama_models(&self) -> Result<Vec<String>, String> {
        let response = http_request(&self.ollama_host, "GET", "/api/tags", None, |_| {})?;
        let root = crate::json::parse(&response)?;
        let rows = root
            .get("models")
            .and_then(crate::json::Value::as_array)
            .ok_or("Ollama model list is missing models")?;
        let mut models = rows
            .iter()
            .filter_map(|row| row.get("name").and_then(crate::json::Value::as_str))
            .filter(|name| !name.is_empty() && name.len() <= 256)
            .map(str::to_owned)
            .take(256)
            .collect::<Vec<_>>();
        models.sort();
        models.dedup();
        Ok(models)
    }

    pub fn select_ollama_model(&mut self, model: &str) -> Result<(), String> {
        if model.is_empty()
            || model.len() > 256
            || model
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err("invalid Ollama model name".into());
        }
        let models = self.ollama_models()?;
        if !models.iter().any(|available| available == model) {
            return Err("that model is not installed in Ollama".into());
        }
        self.ollama_model = model.to_owned();
        Ok(())
    }

    fn ollama_chat(
        &self,
        prompt: &str,
        format: &str,
        event: &mut impl FnMut(ModelEvent),
    ) -> Result<String, String> {
        match self.ollama_chat_once(prompt, format, true, event) {
            Err(error) if error.contains("HTTP 400") => {
                self.ollama_chat_once(prompt, format, false, event)
            }
            result => result,
        }
    }

    fn ollama_chat_once(
        &self,
        prompt: &str,
        format: &str,
        think: bool,
        event: &mut impl FnMut(ModelEvent),
    ) -> Result<String, String> {
        if self.ollama_model.is_empty() {
            return Err("Ollama is disabled".into());
        }
        let body = format!(
            "{{\"model\":{},\"messages\":[{{\"role\":\"user\",\"content\":{}}}],\"stream\":true,\"format\":{format},\"think\":{think},\"options\":{{\"temperature\":0}}}}",
            crate::json::quoted(&self.ollama_model),
            crate::json::quoted(prompt)
        );
        let mut answer = String::new();
        let mut saw_thinking = false;
        http_request(
            &self.ollama_host,
            "POST",
            "/api/chat",
            Some(&body),
            |line| {
                let Ok(value) = crate::json::parse(line) else {
                    return;
                };
                let Some(message) = value.get("message") else {
                    return;
                };
                if message
                    .get("thinking")
                    .and_then(crate::json::Value::as_str)
                    .is_some_and(|text| !text.is_empty())
                    && !saw_thinking
                {
                    saw_thinking = true;
                    event(ModelEvent::Thinking);
                }
                if let Some(content) = message.get("content").and_then(crate::json::Value::as_str) {
                    if answer.len().saturating_add(content.len()) <= MAX_RESPONSE_BYTES {
                        answer.push_str(content);
                    }
                }
            },
        )?;
        if answer.trim().is_empty() {
            return Err("Ollama returned no answer".into());
        }
        Ok(answer)
    }

    fn route(
        &self,
        validate: impl Fn(&str) -> Result<(), String>,
        mut run: impl FnMut(ProviderKind) -> Result<String, String>,
    ) -> Result<RoutedResponse, String> {
        let mut failures = Vec::new();
        for provider in [
            ProviderKind::Ollama,
            ProviderKind::External,
            ProviderKind::Cloud,
        ] {
            let enabled = match provider {
                ProviderKind::Ollama => !self.ollama_model.is_empty(),
                ProviderKind::External => self.external_command.is_some(),
                ProviderKind::Cloud => {
                    self.allow_cloud
                        && self.cloud_command.is_some()
                        && self.cloud_calls.get() < self.cloud_call_limit
                }
            };
            if !enabled {
                continue;
            }
            if provider == ProviderKind::Cloud {
                self.cloud_calls.set(self.cloud_calls.get() + 1);
            }
            match run(provider).and_then(|text| {
                if text.trim().is_empty() {
                    return Err("empty provider response".into());
                }
                validate(&text)?;
                Ok(text)
            }) {
                Ok(text) => return Ok(RoutedResponse { provider, text }),
                Err(error) => failures.push(format!("{provider:?}: {error}")),
            }
        }
        Err(format!(
            "No valid teacher response. {}. Cloud requires explicit enablement and an available call budget.",
            failures.join("; ")
        ))
    }
}

fn http_request(
    host: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    mut line: impl FnMut(&str),
) -> Result<String, String> {
    let host = host
        .strip_prefix("http://")
        .unwrap_or(host)
        .trim_end_matches('/');
    if host.is_empty() || host.contains('/') || host.contains('\r') || host.contains('\n') {
        return Err("invalid Ollama host".into());
    }
    let address = host
        .to_socket_addrs()
        .map_err(|error| format!("cannot resolve Ollama host: {error}"))?
        .next()
        .ok_or("Ollama host has no address")?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))
        .map_err(|error| format!("cannot connect to Ollama: {error}"))?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(TIMEOUT)))
        .map_err(|error| format!("cannot set Ollama timeout: {error}"))?;
    let body = body.unwrap_or("");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .map_err(|error| format!("cannot write Ollama request: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("cannot flush Ollama request: {error}"))?;

    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader
        .read_line(&mut status)
        .map_err(|error| format!("cannot read Ollama status: {error}"))?;
    let code = status
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or("invalid Ollama HTTP status")?;
    let mut chunked = false;
    let mut content_length = None;
    loop {
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|error| format!("cannot read Ollama header: {error}"))?;
        if header == "\r\n" || header == "\n" {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err("invalid Ollama HTTP header".into());
        };
        if name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let bytes = if chunked {
        read_chunked(&mut reader, &mut line)?
    } else {
        read_sized(&mut reader, content_length, &mut line)?
    };
    let text = String::from_utf8(bytes).map_err(|_| "Ollama returned invalid UTF-8")?;
    if !(200..300).contains(&code) {
        return Err(format!("Ollama returned HTTP {code}: {}", text.trim()));
    }
    Ok(text)
}

fn read_chunked(reader: &mut impl BufRead, line: &mut impl FnMut(&str)) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut pending = Vec::new();
    loop {
        let mut size_line = String::new();
        reader
            .read_line(&mut size_line)
            .map_err(|error| format!("cannot read Ollama chunk: {error}"))?;
        let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or(""), 16)
            .map_err(|_| "invalid Ollama chunk size")?;
        if size == 0 {
            break;
        }
        if output.len().saturating_add(size) > MAX_RESPONSE_BYTES {
            return Err("Ollama response exceeds 256 KiB".into());
        }
        let start = output.len();
        output.resize(start + size, 0);
        reader
            .read_exact(&mut output[start..])
            .map_err(|error| format!("cannot read Ollama chunk body: {error}"))?;
        pending.extend_from_slice(&output[start..]);
        emit_lines(&mut pending, line)?;
        let mut ending = [0_u8; 2];
        reader
            .read_exact(&mut ending)
            .map_err(|error| format!("cannot read Ollama chunk ending: {error}"))?;
        if ending != *b"\r\n" {
            return Err("invalid Ollama chunk ending".into());
        }
    }
    emit_tail(&mut pending, line)?;
    Ok(output)
}

fn read_sized(
    reader: &mut impl Read,
    content_length: Option<usize>,
    line: &mut impl FnMut(&str),
) -> Result<Vec<u8>, String> {
    let limit = content_length.unwrap_or(MAX_RESPONSE_BYTES);
    if limit > MAX_RESPONSE_BYTES {
        return Err("Ollama response exceeds 256 KiB".into());
    }
    let mut output = Vec::with_capacity(limit);
    if content_length.is_some() {
        output.resize(limit, 0);
        reader
            .read_exact(&mut output)
            .map_err(|error| format!("cannot read Ollama response: {error}"))?;
    } else {
        reader
            .take((limit + 1) as u64)
            .read_to_end(&mut output)
            .map_err(|error| format!("cannot read Ollama response: {error}"))?;
        if output.len() > limit {
            return Err("Ollama response exceeds 256 KiB".into());
        }
    }
    let text = std::str::from_utf8(&output).map_err(|_| "Ollama returned invalid UTF-8")?;
    for row in text.lines().filter(|row| !row.trim().is_empty()) {
        line(row);
    }
    Ok(output)
}

fn emit_lines(pending: &mut Vec<u8>, line: &mut impl FnMut(&str)) -> Result<(), String> {
    while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
        let row = pending.drain(..=end).collect::<Vec<_>>();
        let row = std::str::from_utf8(&row[..row.len() - 1])
            .map_err(|_| "Ollama returned invalid UTF-8")?
            .trim_end_matches('\r');
        if !row.is_empty() {
            line(row);
        }
    }
    Ok(())
}

fn emit_tail(pending: &mut Vec<u8>, line: &mut impl FnMut(&str)) -> Result<(), String> {
    if !pending.is_empty() {
        let row = std::str::from_utf8(pending).map_err(|_| "Ollama returned invalid UTF-8")?;
        if !row.trim().is_empty() {
            line(row);
        }
        pending.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    fn router(host: String) -> ModelRouter {
        ModelRouter {
            ollama_model: "local".into(),
            ollama_host: host,
            external_command: Some("free".into()),
            cloud_command: Some("paid".into()),
            allow_cloud: false,
            cloud_call_limit: 1,
            cloud_calls: Cell::new(0),
        }
    }

    fn read_request(socket: &mut TcpStream) {
        let mut request = Vec::new();
        let mut block = [0_u8; 512];
        loop {
            let count = socket.read(&mut block).unwrap();
            assert!(count > 0);
            request.extend_from_slice(&block[..count]);
            let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
                continue;
            };
            let headers = std::str::from_utf8(&request[..header_end]).unwrap();
            let length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if request.len() >= header_end + 4 + length {
                return;
            }
        }
    }

    #[test]
    fn disabled_cloud_is_never_invoked() {
        let model = router("127.0.0.1:1".into());
        let mut calls = vec![];
        assert!(model
            .route(
                |_| Ok(()),
                |provider| {
                    calls.push(provider);
                    Err("offline".into())
                }
            )
            .is_err());
        assert_eq!(calls, vec![ProviderKind::Ollama, ProviderKind::External]);
    }

    #[test]
    fn local_stream_reports_real_thinking_and_content() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            read_request(&mut socket);
            let rows = [
                "{\"message\":{\"thinking\":\"checking\"},\"done\":false}\n",
                "{\"message\":{\"content\":\"{\\\"ok\\\":true}\"},\"done\":false}\n",
                "{\"message\":{\"content\":\"\"},\"done\":true}\n",
            ];
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"
            )
            .unwrap();
            for row in rows {
                write!(socket, "{:x}\r\n{}\r\n", row.len(), row).unwrap();
            }
            write!(socket, "0\r\n\r\n").unwrap();
        });
        let model = router(address.to_string());
        let mut events = Vec::new();
        let answer = model
            .infer_validated_with_events("prompt", |_| Ok(()), |event| events.push(event))
            .unwrap();
        assert_eq!(answer.text, r#"{"ok":true}"#);
        assert_eq!(
            events,
            vec![
                ModelEvent::AskingOllama,
                ModelEvent::Thinking,
                ModelEvent::Processing
            ]
        );
        server.join().unwrap();
    }

    #[test]
    fn model_list_is_sorted_and_selection_is_checked() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                read_request(&mut socket);
                let body = r#"{"models":[{"name":"z:1"},{"name":"a:2"}]}"#;
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let mut model = router(address.to_string());
        assert_eq!(model.ollama_models().unwrap(), vec!["a:2", "z:1"]);
        model.select_ollama_model("a:2").unwrap();
        assert_eq!(model.ollama_model, "a:2");
        server.join().unwrap();
    }

    #[test]
    fn paid_attempts_have_a_session_limit_even_on_failure() {
        let mut model = router("127.0.0.1:1".into());
        model.allow_cloud = true;
        let mut paid = 0;
        for _ in 0..3 {
            let _ = model.route(
                |_| Ok(()),
                |provider| {
                    if provider == ProviderKind::Cloud {
                        paid += 1;
                    }
                    Err("offline".into())
                },
            );
        }
        assert_eq!(paid, 1);
    }
}
