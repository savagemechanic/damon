use damon::Damon;
use std::io::{self, BufRead, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut damon = Damon::open_default()?;
    for warning in damon.data.recovery_warnings() {
        eprintln!("Memory recovery: {warning}");
    }
    if std::env::args()
        .skip(1)
        .any(|argument| argument == "--chat-stdio")
    {
        return chat_stdio(&mut damon);
    }
    println!("Damon is ready. Speak naturally. Type 'exit' to stop.");
    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut input = String::new();
        if stdin.read_line(&mut input)? == 0 {
            break;
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if matches!(input, "exit" | "quit" | "goodbye") {
            break;
        }
        println!("{}", damon.handle(input));
    }
    Ok(())
}

fn chat_stdio(damon: &mut Damon) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match chat_request(line) {
            Ok(ChatRequest::Message(input)) => {
                let response = damon.handle_with_events(&input, |state| {
                    let _ = write_event(
                        &mut stdout,
                        &format!(
                            "{{\"type\":\"state\",\"state\":{}}}",
                            damon::json::quoted(state.name())
                        ),
                    );
                });
                write_event(
                    &mut stdout,
                    &format!(
                        "{{\"type\":\"response\",\"response\":{}}}",
                        damon::json::quoted(&response)
                    ),
                )?;
            }
            Ok(ChatRequest::ListModels) => match damon.models.ollama_models() {
                Ok(models) => write_models(&mut stdout, &models, &damon.models.ollama_model)?,
                Err(error) => write_event(
                    &mut stdout,
                    &format!(
                        "{{\"type\":\"model_error\",\"message\":{}}}",
                        damon::json::quoted(&error)
                    ),
                )?,
            },
            Ok(ChatRequest::SelectModel(model)) => {
                let result = damon.models.select_ollama_model(&model);
                let (ok, message) = match result {
                    Ok(()) => (true, format!("Using {model}.")),
                    Err(error) => (false, error),
                };
                write_event(
                    &mut stdout,
                    &format!(
                        "{{\"type\":\"model_selected\",\"ok\":{ok},\"model\":{},\"message\":{}}}",
                        damon::json::quoted(&damon.models.ollama_model),
                        damon::json::quoted(&message)
                    ),
                )?;
            }
            Err(error) => write_event(
                &mut stdout,
                &format!(
                    "{{\"type\":\"response\",\"response\":{}}}",
                    damon::json::quoted(&format!("I could not read that request: {error}"))
                ),
            )?,
        }
    }
    Ok(())
}

enum ChatRequest {
    Message(String),
    ListModels,
    SelectModel(String),
}

fn chat_request(line: &str) -> Result<ChatRequest, String> {
    if !line.starts_with('{') {
        return Ok(ChatRequest::Message(line.to_owned()));
    }
    let value = damon::json::parse(line)?;
    value.fields_exact(&["type", "text", "model"])?;
    let kind = value
        .get("type")
        .and_then(damon::json::Value::as_str)
        .ok_or("request type must be a string")?;
    match kind {
        "request" => value
            .get("text")
            .and_then(damon::json::Value::as_str)
            .filter(|text| !text.trim().is_empty() && text.len() <= 8192)
            .map(|text| ChatRequest::Message(text.to_owned()))
            .ok_or_else(|| "request text is missing or too large".into()),
        "list_models" => Ok(ChatRequest::ListModels),
        "select_model" => value
            .get("model")
            .and_then(damon::json::Value::as_str)
            .map(|model| ChatRequest::SelectModel(model.to_owned()))
            .ok_or_else(|| "model name is missing".into()),
        _ => Err("unknown request type".into()),
    }
}

fn write_event(output: &mut impl Write, event: &str) -> io::Result<()> {
    output.write_all(event.as_bytes())?;
    output.write_all(b"\n")?;
    output.flush()
}

fn write_models(output: &mut impl Write, models: &[String], selected: &str) -> io::Result<()> {
    let names = models
        .iter()
        .map(|model| damon::json::quoted(model))
        .collect::<Vec<_>>()
        .join(",");
    write_event(
        output,
        &format!(
            "{{\"type\":\"models\",\"models\":[{names}],\"selected\":{}}}",
            damon::json::quoted(selected)
        ),
    )
}
