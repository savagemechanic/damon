use damon::Damon;
use std::io::{self, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut damon = Damon::open_default()?;
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
