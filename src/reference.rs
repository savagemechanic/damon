use crate::{
    data::DamonData,
    graph::NodeKind,
    types::{EntityId, MeaningGraph},
    world,
};

pub fn project_mentions(input: &str, data: &DamonData) -> Vec<EntityId> {
    let text = input.to_ascii_lowercase();
    let words = text
        .split(|c: char| !c.is_alphanumeric() && !"-_.".contains(c))
        .filter(|w| !w.is_empty())
        .take(256)
        .collect::<Vec<_>>();
    let mut matches = Vec::new();
    for start in 0..words.len() {
        for end in (start + 1..=(start + 8).min(words.len())).rev() {
            if let Some(id) = data
                .resolve(&words[start..end].join(" "))
                .filter(|id| data.entity(*id).is_some_and(|e| e.kind == world::PROJECT))
            {
                if !matches.contains(&id) {
                    matches.push(id);
                }
                break;
            }
        }
    }
    matches
}
pub fn target(input: &str, data: &DamonData) -> Result<Option<EntityId>, String> {
    let text = crate::language::normalize(input);
    let mentions = project_mentions(&text, data);
    if mentions.len() > 1 {
        return Err("Which project should this action use?".into());
    }
    if let Some(id) = mentions.first() {
        return Ok(Some(*id));
    }
    // A named but unknown project must never silently fall back to Damon.
    for separator in [" in ", " to "] {
        if let Some((_, name)) = text.rsplit_once(separator) {
            if !matches!(
                name,
                "it" | "that"
                    | "this"
                    | "there"
                    | "the project"
                    | "this project"
                    | "the previous project"
            ) {
                return Err(format!(
                    "I do not know project {name:?}. Say 'remember project NAME at \"/path\"'."
                ));
            }
        }
    }
    let focus = if text.contains("previous project") {
        data.world.context.previous_target
    } else {
        data.world.context.focus
    };
    if focus.is_none() && has_reference(&text) {
        return Err("Which project does that refer to?".into());
    }
    Ok(focus.or_else(|| data.resolve("damon")))
}
pub fn has_reference(text: &str) -> bool {
    text.split_whitespace().any(|w| {
        matches!(
            w.trim_matches(|c: char| !c.is_alphanumeric()),
            "it" | "that" | "this" | "its" | "they" | "there"
        )
    }) || text.contains("same thing")
        || text.contains("previous project")
}
pub fn retarget(meaning: &MeaningGraph, target: EntityId) -> MeaningGraph {
    let mut result = meaning.clone();
    if let Some(previous) = meaning.target {
        for node in &mut result.nodes {
            if node.kind == NodeKind::Entity && node.value == previous.0 {
                node.value = target.0;
            }
        }
    }
    result.target = Some(target);
    result
}
pub fn repeat(input: &str, data: &DamonData) -> Result<MeaningGraph, String> {
    let feature = data
        .world
        .context
        .previous_feature
        .ok_or("There is no verified previous action to repeat.")?;
    let meaning = data
        .learned_graphs
        .get(&feature)
        .ok_or("The previous procedure is no longer retained. Please name the action.")?;
    if meaning.intent.0 > 5 {
        return Ok(meaning.clone());
    }
    let target = target(input, data)?.ok_or("Which project should I use?")?;
    if meaning
        .nodes
        .iter()
        .any(|n| n.kind == NodeKind::Entity && Some(EntityId(n.value)) != meaning.target)
    {
        return Err(
            "The previous action used multiple projects. Please specify each target.".into(),
        );
    }
    Ok(retarget(meaning, target))
}
