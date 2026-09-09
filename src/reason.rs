use crate::{
    data::DamonData,
    semantics::{self, Relation},
    tools,
    types::{Action, MeaningGraph},
};

#[derive(Debug)]
pub struct Dependency {
    pub step: usize,
    pub previous: usize,
    pub success_required: bool,
}
#[derive(Debug)]
pub struct Plan {
    pub actions: Vec<Action>,
    pub dependencies: Vec<Dependency>,
}

pub fn plan(meaning: &MeaningGraph, data: &DamonData) -> Result<Plan, String> {
    semantics::validate(meaning, data)?;
    let mut actions = Vec::new();
    let mut indexes = vec![None; meaning.nodes.len()];
    for (index, node) in meaning.nodes.iter().enumerate() {
        if node.kind == crate::graph::NodeKind::Action {
            indexes[index] = Some(actions.len());
            let edge = meaning
                .edges
                .iter()
                .find(|e| e.source == index as u32 && e.relation == Relation::Target as u16)
                .ok_or("action target missing")?;
            let target = crate::types::EntityId(meaning.nodes[edge.target as usize].value);
            actions.push(tools::action_for(
                crate::types::IntentId(node.value),
                target,
                data,
            )?);
        }
    }
    let mut dependencies = Vec::new();
    for edge in &meaning.edges {
        if edge.relation == Relation::Dependency as u16
            || edge.relation == Relation::Condition as u16
        {
            dependencies.push(Dependency {
                step: indexes[edge.source as usize].ok_or("invalid dependent action")?,
                previous: indexes[edge.target as usize].ok_or("invalid prerequisite action")?,
                success_required: edge.relation == Relation::Condition as u16,
            });
        }
    }
    Ok(Plan {
        actions,
        dependencies,
    })
}
pub fn resolve(meaning: &MeaningGraph, data: &DamonData) -> Result<Action, String> {
    let p = plan(meaning, data)?;
    if p.actions.len() != 1 {
        return Err("composite meaning requires a plan".into());
    }
    p.actions
        .into_iter()
        .next()
        .ok_or_else(|| "empty plan".into())
}
