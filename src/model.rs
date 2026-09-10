use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_PROMPT_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(45);
const ZEN_TIMEOUT: Duration = Duration::from_secs(90);
const OPENCODE_VERSION: &str = "1.18.30";
const OPENCODE_ID_RANDOM_LENGTH: usize = 14;
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
static OPENCODE_ID_STATE: Mutex<(u64, u16)> = Mutex::new((0, 0));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Ollama,
    External,
    Cloud,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelEvent {
    AskingOllama,
    AskingCloud,
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
    pub zen_model: String,
    pub zen_api_key: Option<String>,
    pub zen_base_url: String,
    pub zen_session_id: String,
    pub zen_project_id: Option<String>,
    pub allow_cloud: bool,
}

impl Default for ModelRouter {
    fn default() -> Self {
        let zen_api_key = env::var("DAMON_OPENCODE_API_KEY")
            .ok()
            .filter(|key| !key.is_empty());
        Self {
            ollama_model: env::var("DAMON_OLLAMA_MODEL").unwrap_or_default(),
            ollama_host: env::var("DAMON_OLLAMA_HOST").unwrap_or_else(|_| "127.0.0.1:11434".into()),
            external_command: env::var("DAMON_MODEL_COMMAND").ok(),
            zen_model: env::var("DAMON_ZEN_MODEL").unwrap_or_else(|_| "big-pickle".into()),
            allow_cloud: zen_api_key.is_some(),
            zen_api_key,
            zen_session_id: opencode_id("ses", IdDirection::Descending).unwrap_or_default(),
            zen_project_id: opencode_project_id(env::current_dir().ok().as_deref()),
            zen_base_url: if cfg!(debug_assertions) {
                env::var("DAMON_ZEN_BASE_URL")
                    .unwrap_or_else(|_| "https://opencode.ai/zen/v1".into())
            } else {
                "https://opencode.ai/zen/v1".into()
            },
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
            ProviderKind::Cloud => {
                event(ModelEvent::AskingCloud);
                let result = self.zen_chat(prompt);
                event(ModelEvent::Processing);
                result
            }
            ProviderKind::Ollama => {
                event(ModelEvent::AskingOllama);
                let result = self.ollama_chat(prompt, format, event);
                event(ModelEvent::Processing);
                result
            }
            ProviderKind::External => {
                let command = &self.external_command;
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
        let response = http_request(&self.ollama_host, "GET", "/api/tags", None, |_| Ok(()))?;
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

    pub fn set_zen_api_key(&mut self, key: String) -> Result<(), String> {
        if key.len() < 8
            || key.len() > 4096
            || key
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err("invalid OpenCode Zen API key".into());
        }
        self.zen_api_key = Some(key);
        self.allow_cloud = true;
        Ok(())
    }

    pub fn zen_models(&self) -> Result<Vec<String>, String> {
        let text = self.zen_request("GET", "/models", None, false)?;
        zen_models_from_json(&text)
    }

    pub fn select_zen_model(&mut self, model: &str) -> Result<(), String> {
        validate_model_name(model)?;
        if !self
            .zen_models()?
            .iter()
            .any(|available| available == model)
        {
            return Err("that free model is not currently available from OpenCode Zen".into());
        }
        self.zen_model = model.to_owned();
        Ok(())
    }

    fn zen_chat(&self, prompt: &str) -> Result<String, String> {
        self.zen_api_key
            .as_deref()
            .ok_or("OpenCode Zen API key is not configured")?;
        validate_model_name(&self.zen_model)?;
        if !is_free_chat_model(&self.zen_model) {
            return Err("selected OpenCode Zen model is not a supported free chat model".into());
        }
        let body = format!(
            "{{\"model\":{},\"messages\":[{{\"role\":\"user\",\"content\":{}}}],\"temperature\":0,\"stream\":true,\"stream_options\":{{\"include_usage\":true}}}}",
            crate::json::quoted(&self.zen_model),
            crate::json::quoted(prompt)
        );
        self.zen_request("POST", "/chat/completions", Some(&body), true)
    }

    fn zen_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        authenticate: bool,
    ) -> Result<String, String> {
        let url = format!("{}{path}", self.zen_base_url.trim_end_matches('/'));
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(ZEN_TIMEOUT))
            .http_status_as_error(false)
            .build();
        let agent: ureq::Agent = config.into();
        let operation = if authenticate {
            "model request"
        } else {
            "model catalog"
        };
        let response = match (method, body) {
            ("GET", None) => agent.get(&url).call(),
            ("POST", Some(body)) => {
                let key = self
                    .zen_api_key
                    .as_deref()
                    .ok_or("OpenCode Zen API key is not configured")?;
                let request_id = opencode_id("msg", IdDirection::Ascending)?;
                if self.zen_session_id.is_empty() {
                    return Err("cannot generate OpenCode session ID".into());
                }
                let mut request = agent
                    .post(&url)
                    .header("Authorization", &format!("Bearer {key}"))
                    .header("Content-Type", "application/json")
                    .header("User-Agent", &format!("opencode/{OPENCODE_VERSION}"))
                    .header("x-opencode-client", "cli")
                    .header("x-opencode-session", &self.zen_session_id)
                    .header("x-opencode-request", &request_id);
                if let Some(project_id) = &self.zen_project_id {
                    request = request.header("x-opencode-project", project_id);
                }
                request.send(body)
            }
            _ => return Err("unsupported Zen request shape".into()),
        };
        let mut response =
            response.map_err(|error| format!("OpenCode Zen {operation} failed: {error}"))?;
        let status = response.status();
        if status.is_success() && authenticate {
            return zen_answer_from_reader(response.body_mut().as_reader());
        }
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES as u64)
            .read_to_string()
            .map_err(|error| format!("OpenCode Zen response is invalid or too large: {error}"))?;
        if !status.is_success() {
            let detail = zen_error_detail(&text)
                .unwrap_or_else(|| "provider returned no error detail".into());
            return Err(format!(
                "OpenCode Zen {operation} failed (HTTP {}): {detail}",
                status.as_u16()
            ));
        }
        Ok(text)
    }

    fn ollama_chat(
        &self,
        prompt: &str,
        format: &str,
        event: &mut impl FnMut(ModelEvent),
    ) -> Result<String, String> {
        self.ollama_chat_once(prompt, format, true, event)
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
                let value = crate::json::parse(line)
                    .map_err(|error| format!("invalid Ollama stream row: {error}"))?;
                if let Some(error) = value.get("error").and_then(crate::json::Value::as_str) {
                    return Err(format!("Ollama stream error: {error}"));
                }
                let Some(message) = value.get("message") else {
                    return Ok(());
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
                    if answer.len().saturating_add(content.len()) > MAX_RESPONSE_BYTES {
                        return Err("Ollama answer exceeds 256 KiB".into());
                    }
                    answer.push_str(content);
                }
                Ok(())
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
        let provider = [ProviderKind::Cloud, ProviderKind::External, ProviderKind::Ollama]
            .into_iter()
            .find(|provider| match provider {
                ProviderKind::Ollama => !self.ollama_model.is_empty(),
                ProviderKind::External => self.external_command.is_some(),
                ProviderKind::Cloud => self.allow_cloud && self.zen_api_key.is_some(),
            })
            .ok_or(
                "No language teacher is configured. Add an OpenCode Zen API key or explicitly enable another provider.",
            )?;
        let text = run(provider).map_err(|error| format!("{provider:?}: {error}"))?;
        if text.trim().is_empty() {
            return Err(format!("{provider:?}: empty provider response"));
        }
        validate(&text).map_err(|error| format!("{provider:?}: {error}"))?;
        Ok(RoutedResponse { provider, text })
    }
}

#[derive(Clone, Copy)]
enum IdDirection {
    Ascending,
    Descending,
}

fn opencode_id(prefix: &str, direction: IdDirection) -> Result<String, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock cannot create OpenCode request ID: {error}"))?
        .as_millis() as u64;
    let counter = {
        let mut state = OPENCODE_ID_STATE
            .lock()
            .map_err(|_| "OpenCode request ID state is unavailable")?;
        if state.0 != timestamp {
            *state = (timestamp, 0);
        }
        state.1 = state.1.saturating_add(1);
        state.1
    };
    let mut random = [0_u8; OPENCODE_ID_RANDOM_LENGTH];
    getrandom::getrandom(&mut random)
        .map_err(|error| format!("cannot generate OpenCode request ID: {error}"))?;
    Ok(opencode_id_from_parts(
        prefix, direction, timestamp, counter, &random,
    ))
}

fn opencode_id_from_parts(
    prefix: &str,
    direction: IdDirection,
    timestamp: u64,
    counter: u16,
    random: &[u8; OPENCODE_ID_RANDOM_LENGTH],
) -> String {
    let encoded = timestamp.wrapping_mul(0x1000).wrapping_add(counter as u64);
    let encoded = match direction {
        IdDirection::Ascending => encoded,
        IdDirection::Descending => !encoded,
    } & 0x0000_ffff_ffff_ffff;
    let suffix = random
        .iter()
        .map(|byte| BASE62[usize::from(*byte) % BASE62.len()] as char)
        .collect::<String>();
    format!("{prefix}_{encoded:012x}{suffix}")
}

fn opencode_project_id(start: Option<&Path>) -> Option<String> {
    if let Ok(id) = env::var("DAMON_OPENCODE_PROJECT_ID") {
        if valid_opencode_project_id(&id) {
            return Some(id);
        }
    }
    let mut directory = match start {
        Some(start) => start.to_path_buf(),
        None => return Some("global".into()),
    };
    loop {
        let cache = directory.join(".git/opencode");
        if let Ok(id) = std::fs::read_to_string(cache) {
            let id = id.trim();
            if valid_opencode_project_id(id) {
                return Some(id.to_owned());
            }
        }
        if !directory.pop() {
            return Some("global".into());
        }
    }
}

fn valid_opencode_project_id(id: &str) -> bool {
    id == "global" || (id.len() == 40 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn zen_answer_from_reader(reader: impl Read) -> Result<String, String> {
    let mut answer = String::new();
    let mut reader = BufReader::new(reader.take((MAX_STREAM_BYTES + 1) as u64));
    let mut transferred = 0_usize;
    loop {
        let mut line = String::new();
        let count = reader
            .read_line(&mut line)
            .map_err(|error| format!("cannot read OpenCode Zen stream: {error}"))?;
        if count == 0 {
            break;
        }
        transferred = transferred.saturating_add(count);
        if transferred > MAX_STREAM_BYTES {
            return Err("OpenCode Zen stream exceeds 8 MiB".into());
        }
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        if data.is_empty() {
            continue;
        }
        let row = crate::json::parse(data)
            .map_err(|error| format!("invalid OpenCode Zen stream row: {error}"))?;
        if let Some(detail) = zen_error_detail(data) {
            return Err(format!("OpenCode Zen stream error: {detail}"));
        }
        let content = row
            .get("choices")
            .and_then(crate::json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("content"))
            .and_then(crate::json::Value::as_str);
        if let Some(content) = content {
            if answer.len().saturating_add(content.len()) > MAX_RESPONSE_BYTES {
                return Err("OpenCode Zen answer exceeds 256 KiB".into());
            }
            answer.push_str(content);
        }
    }
    if answer.trim().is_empty() {
        Err("OpenCode Zen returned no answer".into())
    } else {
        Ok(answer)
    }
}

fn validate_model_name(model: &str) -> Result<(), String> {
    if model.is_empty()
        || model.len() > 256
        || model
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        Err("invalid model name".into())
    } else {
        Ok(())
    }
}

fn is_free_chat_model(model: &str) -> bool {
    (model == "big-pickle" || model.ends_with("-free")) && !model.starts_with("muse-spark-")
}

fn zen_models_from_json(text: &str) -> Result<Vec<String>, String> {
    let root = crate::json::parse(text)?;
    let rows = root
        .get("data")
        .and_then(crate::json::Value::as_array)
        .ok_or("OpenCode Zen model list is missing data")?;
    let mut models = rows
        .iter()
        .filter_map(|row| row.get("id").and_then(crate::json::Value::as_str))
        .filter(|model| validate_model_name(model).is_ok() && is_free_chat_model(model))
        .map(str::to_owned)
        .take(256)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    Ok(models)
}

fn zen_error_detail(text: &str) -> Option<String> {
    let root = crate::json::parse(text).ok()?;
    root.get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(crate::json::Value::as_str)
                .or_else(|| error.as_str())
        })
        .or_else(|| root.get("message").and_then(crate::json::Value::as_str))
        .filter(|message| !message.trim().is_empty())
        .map(str::to_owned)
}

fn http_request(
    host: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    mut line: impl FnMut(&str) -> Result<(), String>,
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
    let success = (200..300).contains(&code);
    let mut accepted_line = |row: &str| {
        if success {
            line(row)
        } else {
            Ok(())
        }
    };
    let bytes = if chunked {
        read_chunked(&mut reader, &mut accepted_line)?
    } else {
        read_sized(&mut reader, content_length, &mut accepted_line)?
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if !success {
        return Err(format!("Ollama returned HTTP {code}: {}", text.trim()));
    }
    Ok(text)
}

fn read_chunked(
    reader: &mut impl BufRead,
    line: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut pending = Vec::new();
    let mut transferred = 0_usize;
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
        transferred = transferred.saturating_add(size);
        if transferred > MAX_STREAM_BYTES {
            return Err("Ollama stream exceeds 8 MiB".into());
        }
        let mut chunk = vec![0_u8; size];
        reader
            .read_exact(&mut chunk)
            .map_err(|error| format!("cannot read Ollama chunk body: {error}"))?;
        let retained = MAX_RESPONSE_BYTES.saturating_sub(output.len()).min(size);
        output.extend_from_slice(&chunk[..retained]);
        pending.extend_from_slice(&chunk);
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
    line: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    if content_length.is_some_and(|length| length > MAX_STREAM_BYTES) {
        return Err("Ollama stream exceeds 8 MiB".into());
    }
    let limit = content_length.unwrap_or(MAX_STREAM_BYTES);
    let mut output = Vec::with_capacity(limit.min(MAX_RESPONSE_BYTES));
    let mut pending = Vec::new();
    let mut transferred = 0_usize;
    let mut block = [0_u8; 8192];
    while transferred < limit {
        let wanted = (limit - transferred).min(block.len());
        let count = reader
            .read(&mut block[..wanted])
            .map_err(|error| format!("cannot read Ollama response: {error}"))?;
        if count == 0 {
            if content_length.is_some() {
                return Err("Ollama response ended early".into());
            }
            break;
        }
        transferred += count;
        let retained = MAX_RESPONSE_BYTES.saturating_sub(output.len()).min(count);
        output.extend_from_slice(&block[..retained]);
        pending.extend_from_slice(&block[..count]);
        emit_lines(&mut pending, line)?;
    }
    emit_tail(&mut pending, line)?;
    Ok(output)
}

fn emit_lines(
    pending: &mut Vec<u8>,
    line: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
        let row = pending.drain(..=end).collect::<Vec<_>>();
        let row = std::str::from_utf8(&row[..row.len() - 1])
            .map_err(|_| "Ollama returned invalid UTF-8")?
            .trim_end_matches('\r');
        if !row.is_empty() {
            line(row)?;
        }
    }
    Ok(())
}

fn emit_tail(
    pending: &mut Vec<u8>,
    line: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    if !pending.is_empty() {
        let row = std::str::from_utf8(pending).map_err(|_| "Ollama returned invalid UTF-8")?;
        if !row.trim().is_empty() {
            line(row)?;
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
            external_command: None,
            zen_model: "big-pickle".into(),
            zen_api_key: None,
            zen_base_url: "https://opencode.ai/zen/v1".into(),
            zen_session_id: "ses_ffffffffffff00000000000000".into(),
            zen_project_id: Some("675c45d5f7220f30d01d91400e4574b8461820e4".into()),
            allow_cloud: false,
        }
    }

    fn read_request(socket: &mut TcpStream) {
        let _ = read_request_bytes(socket);
    }

    fn read_request_bytes(socket: &mut TcpStream) -> Vec<u8> {
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
                return request;
            }
        }
    }

    #[test]
    fn one_inference_calls_only_the_highest_priority_enabled_provider() {
        let mut model = router("127.0.0.1:1".into());
        model.external_command = Some("free".into());
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
        assert_eq!(calls, vec![ProviderKind::External]);
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
    fn long_reasoning_stream_does_not_consume_the_answer_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            read_request(&mut socket);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"
            )
            .unwrap();
            let thought = "x".repeat(4096);
            for _ in 0..80 {
                let row =
                    format!("{{\"message\":{{\"thinking\":\"{thought}\"}},\"done\":false}}\n");
                write!(socket, "{:x}\r\n{}\r\n", row.len(), row).unwrap();
            }
            let answer = "{\"message\":{\"content\":\"{\\\"ok\\\":true}\"},\"done\":true}\n";
            write!(socket, "{:x}\r\n{}\r\n0\r\n\r\n", answer.len(), answer).unwrap();
        });
        let model = router(address.to_string());
        let response = model
            .ollama_chat_once("prompt", "\"json\"", true, &mut |_| {})
            .unwrap();
        assert_eq!(response, r#"{"ok":true}"#);
        server.join().unwrap();
    }

    #[test]
    fn stream_error_is_reported_instead_of_becoming_an_empty_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            read_request(&mut socket);
            let row = "{\"error\":\"model runner stopped\"}\n";
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
                row.len(),
                row
            )
            .unwrap();
        });
        let model = router(address.to_string());
        let error = model
            .ollama_chat_once("prompt", "\"json\"", true, &mut |_| {})
            .unwrap_err();
        assert!(error.contains("model runner stopped"));
        server.join().unwrap();
    }

    #[test]
    fn provider_adapter_never_retries_internally() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let request = read_request_bytes(&mut socket);
            let body = std::str::from_utf8(&request).unwrap();
            assert!(body.contains("\"think\":true"));
            let error = "{\"error\":\"thinking is not supported\"}";
            write!(
                socket,
                "HTTP/1.1 500 Error\r\nContent-Length: {}\r\n\r\n{}",
                error.len(),
                error
            )
            .unwrap();
        });
        let model = router(address.to_string());
        assert!(model
            .ollama_chat("prompt", "\"json\"", &mut |_| {})
            .is_err());
        server.join().unwrap();
    }

    #[test]
    fn default_configuration_disables_ollama() {
        std::env::remove_var("DAMON_OLLAMA_MODEL");
        assert!(ModelRouter::default().ollama_model.is_empty());
    }

    #[test]
    fn zen_catalog_keeps_only_free_chat_completion_models() {
        let catalog = r#"{"object":"list","data":[
            {"id":"gpt-5.6-sol"},
            {"id":"big-pickle"},
            {"id":"mimo-v2.5-free"},
            {"id":"muse-spark-1.3-contributor-free"},
            {"id":"nemotron-3-ultra-free"}
        ]}"#;
        assert_eq!(
            zen_models_from_json(catalog).unwrap(),
            vec!["big-pickle", "mimo-v2.5-free", "nemotron-3-ultra-free"]
        );
    }

    #[test]
    fn zen_chat_uses_bearer_key_and_extracts_one_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let request = read_request_bytes(&mut socket);
            let request = std::str::from_utf8(&request).unwrap();
            assert!(request.starts_with("POST /chat/completions HTTP/1.1"));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer secret-test-key"));
            assert!(request.contains("user-agent: opencode/1.18.30"));
            assert!(request.contains("x-opencode-client: cli"));
            assert!(request.contains("x-opencode-session: ses_ffffffffffff00000000000000"));
            assert!(request.contains("x-opencode-request: msg_"));
            assert!(
                request.contains("x-opencode-project: 675c45d5f7220f30d01d91400e4574b8461820e4")
            );
            assert!(request.contains("\"model\":\"big-pickle\""));
            assert!(request.contains("\"stream\":true"));
            assert!(request.contains("\"stream_options\":{\"include_usage\":true}"));
            let answer = concat!(
                ": keep-alive\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"ok\\\":\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"true}\"}}]}\n\n",
                "data: [DONE]\n\n",
                "data: {\"choices\":[],\"cost\":\"0\"}\n\n"
            );
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                answer.len(),
                answer
            )
            .unwrap();
        });
        let mut model = router("127.0.0.1:1".into());
        model.zen_api_key = Some("secret-test-key".into());
        model.zen_model = "big-pickle".into();
        model.zen_base_url = format!("http://{address}");
        let response = model.zen_chat("prompt").unwrap();
        assert_eq!(response, r#"{"ok":true}"#);
        server.join().unwrap();
    }

    #[test]
    fn opencode_ids_match_the_source_format_and_direction() {
        let random = [0_u8; OPENCODE_ID_RANDOM_LENGTH];
        assert_eq!(
            opencode_id_from_parts("msg", IdDirection::Ascending, 1, 1, &random),
            "msg_00000000100100000000000000"
        );
        assert_eq!(
            opencode_id_from_parts("ses", IdDirection::Descending, 1, 1, &random),
            "ses_ffffffffeffe00000000000000"
        );
    }

    #[test]
    fn zen_stream_parser_uses_content_and_ignores_reasoning_and_cost() {
        let stream = concat!(
            ": keep-alive\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\",\"content\":null}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n",
            "data: [DONE]\n\n",
            "data: {\"choices\":[],\"cost\":\"0\"}\n\n"
        );
        assert_eq!(zen_answer_from_reader(stream.as_bytes()).unwrap(), "OK");
    }

    #[test]
    fn zen_http_error_preserves_observed_provider_detail() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            read_request(&mut socket);
            let body = r#"{"type":"MissingSessionID","error":{"type":"MissingSessionID","message":"Error from provider (Console): OpenCode's free tier can only be used in OpenCode"}}"#;
            write!(
                socket,
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let mut model = router("127.0.0.1:1".into());
        model.zen_api_key = Some("secret-test-key".into());
        model.zen_model = "big-pickle".into();
        model.zen_base_url = format!("http://{address}");
        let error = model.zen_chat("prompt").unwrap_err();
        assert!(error.contains("HTTP 400"), "{error}");
        assert!(
            error.contains("OpenCode's free tier can only be used in OpenCode"),
            "{error}"
        );
        server.join().unwrap();
    }

    #[test]
    fn zen_http_error_preserves_observed_unavailable_model_detail() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            read_request(&mut socket);
            let body = r#"{"error":{"type":"server_error","message":"Error from provider (Console): Upstream request failed: Model is unavailable."}}"#;
            write!(
                socket,
                "HTTP/1.1 500 Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let mut model = router("127.0.0.1:1".into());
        model.zen_api_key = Some("secret-test-key".into());
        model.zen_model = "deepseek-v4-flash-free".into();
        model.zen_base_url = format!("http://{address}");
        let error = model.zen_chat("prompt").unwrap_err();
        assert!(error.contains("HTTP 500"), "{error}");
        assert!(error.contains("Model is unavailable"), "{error}");
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
}
