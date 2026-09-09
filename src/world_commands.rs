use crate::{data::DamonData, world};
use std::path::Path;
fn unquote(s: &str) -> &str {
    s.trim()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s.trim())
}
pub fn handle(input: &str, data: &mut DamonData) -> Option<String> {
    let input = input.trim();
    let normalized = input.to_ascii_lowercase();
    if normalized == "what projects do you know" || normalized == "show my projects" {
        return Some(
            data.entities
                .iter()
                .filter(|e| e.kind == world::PROJECT)
                .map(|e| format!("{}: {}", e.name, e.value))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let result: Result<String, String> = if normalized.starts_with("remember project ") {
        (|| {
            let rest = &input[17..];
            let split = rest
                .to_ascii_lowercase()
                .find(" at ")
                .ok_or("say 'remember project NAME at \"/path\"'")?;
            let name = unquote(&rest[..split]);
            let path = unquote(&rest[split + 4..]);
            let id = data
                .register_project(name, Path::new(path))
                .map_err(|e| e.to_string())?;
            data.world.context.focus(id);
            Ok(format!(
                "I know {} at {}.",
                data.entities[id.0 as usize].name, data.entities[id.0 as usize].value
            ))
        })()
    } else if normalized.starts_with("remember alias ") {
        (|| {
            let rest = &input[15..];
            let split = rest
                .to_ascii_lowercase()
                .find(" for ")
                .ok_or("say 'remember alias NAME for PROJECT'")?;
            let alias = unquote(&rest[..split]);
            let name = unquote(&rest[split + 5..]);
            let id = data.resolve(name).ok_or("I do not know that entity")?;
            data.add_alias(alias, id).map_err(|e| e.to_string())?;
            Ok(format!(
                "{alias} refers to {}.",
                data.entities[id.0 as usize].name
            ))
        })()
    } else if normalized.starts_with("focus on ") {
        (|| {
            let name = unquote(&input[9..]);
            let id = data
                .resolve(name)
                .filter(|id| data.entity(*id).is_some_and(|e| e.kind == world::PROJECT))
                .ok_or("I do not know that project")?;
            data.world.context.focus(id);
            Ok(format!(
                "I am focused on {}.",
                data.entities[id.0 as usize].name
            ))
        })()
    } else {
        return None;
    };
    Some(match result {
        Ok(message) => match data.save() {
            Ok(()) => message,
            Err(e) => format!("Could not persist project context: {e}"),
        },
        Err(e) => format!("Project operation failed: {e}"),
    })
}
