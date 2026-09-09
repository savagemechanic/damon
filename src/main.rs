use damon::Damon;
use serde::Serialize;
use std::io::{self, BufRead, Write};

#[derive(Serialize)]
struct ChatResponse<'a> {
    response: &'a str,
}

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
        let input = line?;
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        let response = damon.handle(input);
        serde_json::to_writer(
            &mut stdout,
            &ChatResponse {
                response: &response,
            },
        )?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}
