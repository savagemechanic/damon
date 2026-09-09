use crate::{
    codec::{write_u32, write_u64, Reader},
    storage,
    types::{Action, CapabilityId, Effects, EntityId},
};
use std::{io, io::Write};

const MAX_APPROVALS: usize = 4096;
const MAX_SCOPE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalGrant {
    pub capability: CapabilityId,
    pub effects: Effects,
    pub target: Option<EntityId>,
    pub scope_hash: u64,
    pub scope: Vec<u8>,
    pub implementation_version: u32,
    pub revoked: bool,
}

impl ApprovalGrant {
    pub fn from_action(action: &Action) -> Self {
        let scope = canonical_scope(action);
        Self {
            capability: action.capability,
            effects: action.effects,
            target: action.target,
            scope_hash: crate::cache::hash_bytes(&[&scope]),
            scope,
            implementation_version: action.implementation_version,
            revoked: false,
        }
    }

    fn matches(&self, action: &Action, scope: &[u8]) -> bool {
        !self.revoked
            && self.capability == action.capability
            && self.effects == action.effects
            && self.target == action.target
            && self.scope_hash == crate::cache::hash_bytes(&[scope])
            && self.scope == scope
            && self.implementation_version == action.implementation_version
    }
}

#[derive(Clone, Debug, Default)]
pub struct ApprovalTable {
    pub grants: Vec<ApprovalGrant>,
}

impl ApprovalTable {
    pub fn allows(&self, action: &Action) -> bool {
        let scope = canonical_scope(action);
        self.grants
            .iter()
            .any(|grant| grant.matches(action, &scope))
    }

    pub fn approve(&mut self, mut grant: ApprovalGrant) -> io::Result<()> {
        if grant.scope.len() > MAX_SCOPE_BYTES
            || grant.scope_hash != crate::cache::hash_bytes(&[&grant.scope])
        {
            return Err(storage::invalid("invalid approval scope"));
        }
        grant.revoked = false;
        if let Some(existing) = self.grants.iter_mut().find(|existing| {
            existing.capability == grant.capability
                && existing.effects == grant.effects
                && existing.target == grant.target
                && existing.scope_hash == grant.scope_hash
                && existing.scope == grant.scope
                && existing.implementation_version == grant.implementation_version
        }) {
            *existing = grant;
            return Ok(());
        }
        if self.grants.len() >= MAX_APPROVALS {
            return Err(storage::invalid("approval table is full"));
        }
        self.grants.push(grant);
        self.grants.sort_by(compare_grants);
        Ok(())
    }

    pub fn revoke(&mut self, grant: &ApprovalGrant) -> bool {
        let Some(existing) = self.grants.iter_mut().find(|existing| {
            existing.capability == grant.capability
                && existing.effects == grant.effects
                && existing.target == grant.target
                && existing.scope_hash == grant.scope_hash
                && existing.scope == grant.scope
                && existing.implementation_version == grant.implementation_version
        }) else {
            return false;
        };
        existing.revoked = true;
        true
    }

    pub fn revoke_all(&mut self) {
        for grant in &mut self.grants {
            grant.revoked = true;
        }
    }

    pub(crate) fn validate(
        &self,
        capabilities: &crate::capability::CapabilityGraph,
        entity_count: usize,
    ) -> io::Result<()> {
        for grant in &self.grants {
            let capability = capabilities
                .capability(grant.capability)
                .ok_or_else(|| storage::invalid("approval references unknown capability"))?;
            if grant.scope.len() > MAX_SCOPE_BYTES
                || grant.scope_hash != crate::cache::hash_bytes(&[&grant.scope])
                || grant.effects != capability.effects
                || grant
                    .target
                    .is_some_and(|target| target.0 as usize >= entity_count)
            {
                return Err(storage::invalid(
                    "approval is incompatible with current data",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn encode(&self, writer: &mut impl Write) -> io::Result<()> {
        if self.grants.len() > MAX_APPROVALS {
            return Err(storage::invalid("approval table exceeds limit"));
        }
        write_u32(writer, self.grants.len() as u32)?;
        for grant in &self.grants {
            write_u32(writer, grant.capability.0)?;
            write_u32(writer, grant.effects.0)?;
            write_u32(writer, grant.target.map_or(u32::MAX, |target| target.0))?;
            write_u64(writer, grant.scope_hash)?;
            write_u32(writer, grant.scope.len() as u32)?;
            writer.write_all(&grant.scope)?;
            write_u32(writer, grant.implementation_version)?;
            writer.write_all(&[u8::from(grant.revoked)])?;
        }
        Ok(())
    }

    pub(crate) fn decode(reader: &mut Reader<'_>) -> io::Result<Self> {
        let count = reader.u32()? as usize;
        if count > MAX_APPROVALS {
            return Err(storage::invalid("approval table exceeds limit"));
        }
        let mut table = Self::default();
        for _ in 0..count {
            let capability = CapabilityId(reader.u32()?);
            let effects = Effects(reader.u32()?);
            let target = match reader.u32()? {
                u32::MAX => None,
                value => Some(EntityId(value)),
            };
            let scope_hash = reader.u64()?;
            let scope_len = reader.u32()? as usize;
            if scope_len > MAX_SCOPE_BYTES {
                return Err(storage::invalid("approval scope exceeds limit"));
            }
            let scope = reader.take(scope_len)?.to_vec();
            let implementation_version = reader.u32()?;
            let revoked = match reader.u8()? {
                0 => false,
                1 => true,
                _ => return Err(storage::invalid("invalid approval state")),
            };
            if capability.0 == 0
                || effects == Effects::NONE
                || implementation_version == 0
                || scope_hash != crate::cache::hash_bytes(&[&scope])
            {
                return Err(storage::invalid("invalid approval grant"));
            }
            table.grants.push(ApprovalGrant {
                capability,
                effects,
                target,
                scope_hash,
                scope,
                implementation_version,
                revoked,
            });
        }
        if !table
            .grants
            .windows(2)
            .all(|pair| compare_grants(&pair[0], &pair[1]).is_lt())
        {
            return Err(storage::invalid("approval grants are not canonical"));
        }
        Ok(table)
    }
}

fn grant_key(grant: &ApprovalGrant) -> (u32, u32, u32, u64, u32) {
    (
        grant.capability.0,
        grant.effects.0,
        grant.target.map_or(u32::MAX, |target| target.0),
        grant.scope_hash,
        grant.implementation_version,
    )
}

fn compare_grants(left: &ApprovalGrant, right: &ApprovalGrant) -> std::cmp::Ordering {
    grant_key(left)
        .cmp(&grant_key(right))
        .then_with(|| left.scope.cmp(&right.scope))
}

fn canonical_scope(action: &Action) -> Vec<u8> {
    let mut canonical = Vec::new();
    canonical.extend_from_slice(&action.tool.0.to_le_bytes());
    canonical.extend_from_slice(
        &action
            .target
            .map_or(u32::MAX, |target| target.0)
            .to_le_bytes(),
    );
    canonical.extend_from_slice(&(action.args.len() as u32).to_le_bytes());
    for argument in &action.args {
        canonical.extend_from_slice(&(argument.len() as u32).to_le_bytes());
        canonical.extend_from_slice(argument.as_bytes());
    }
    canonical
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub allowed: Effects,
    pub approvals: ApprovalTable,
    pending: Option<PendingApproval>,
}

#[derive(Clone, Debug)]
pub struct PendingApproval {
    pub action: Action,
    pub input: String,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            allowed: Effects::READ
                .union(Effects::PROCESS)
                .union(Effects::NETWORK)
                .union(Effects::WRITE),
            approvals: ApprovalTable::default(),
            pending: None,
        }
    }
}
impl Policy {
    pub fn check(&self, action: &Action) -> Result<(), String> {
        if crate::tools::capability_for_tool(action.tool) != Some(action.capability) {
            return Err("tool is not the registered implementation of this capability".into());
        }
        let required = crate::tools::required_effects(action.tool)?;
        if action.effects != required {
            return Err("action effects do not match the registered tool".into());
        }
        if self.allowed.contains(required) || self.approvals.allows(action) {
            Ok(())
        } else {
            Err(format!(
                "This action needs {} access for its exact target, which is not approved yet. Say “do it” to approve this exact capability, scope, effects, and implementation version. A wider or changed action will ask again.",
                effect_names(required, self.allowed)
            ))
        }
    }

    pub fn approve(&mut self, action: &Action) -> io::Result<()> {
        self.approvals.approve(ApprovalGrant::from_action(action))
    }

    pub fn remember_pending(&mut self, action: &Action, input: &str) {
        self.pending = Some(PendingApproval {
            action: action.clone(),
            input: input.to_owned(),
        });
    }

    pub fn take_pending(&mut self) -> Option<PendingApproval> {
        self.pending.take()
    }

    pub fn clear_pending(&mut self) {
        self.pending = None;
    }
}

fn effect_names(required: Effects, allowed: Effects) -> String {
    let mut names = Vec::new();
    for (effect, name) in [
        (Effects::READ, "read"),
        (Effects::WRITE, "write"),
        (Effects::NETWORK, "network"),
        (Effects::PROCESS, "process"),
        (Effects::CREDENTIAL, "credential"),
        (Effects::DESTRUCTIVE, "destructive"),
        (Effects::PRIVILEGED, "privileged"),
    ] {
        if required.contains(effect) && !allowed.contains(effect) {
            names.push(name);
        }
    }
    if names.is_empty() {
        "additional".into()
    } else {
        names.join(" and ")
    }
}

#[cfg(test)]
mod approval_contract_tests {
    use super::*;
    use crate::types::{CapabilityId, EntityId, ToolId};

    fn action(target: u32, path: &str, version: u32) -> Action {
        Action {
            capability: CapabilityId(14),
            tool: ToolId(11),
            target: Some(EntityId(target)),
            effects: Effects::READ.union(Effects::WRITE),
            args: vec![path.into(), "copy.txt".into()],
            implementation_version: version,
        }
    }

    #[test]
    fn approval_matches_only_the_exact_capability_scope_effects_and_version() {
        let exact = action(3, "/project", 1);
        let grant = ApprovalGrant::from_action(&exact);
        let mut approvals = ApprovalTable::default();
        approvals.approve(grant).unwrap();

        assert!(approvals.allows(&exact));
        assert!(!approvals.allows(&action(4, "/project", 1)));
        assert!(!approvals.allows(&action(3, "/other", 1)));
        assert!(!approvals.allows(&action(3, "/project", 2)));
        let mut changed = exact.clone();
        changed.capability = CapabilityId(15);
        assert!(!approvals.allows(&changed));
        let mut changed = exact.clone();
        changed.effects = Effects::READ;
        assert!(!approvals.allows(&changed));
        let mut changed = exact.clone();
        changed.tool = ToolId(12);
        assert!(!approvals.allows(&changed));
        let mut changed = exact.clone();
        changed.args.push("extra".into());
        assert!(!approvals.allows(&changed));
    }

    #[test]
    fn revoked_approval_never_allows_execution() {
        let exact = action(3, "/project", 1);
        let grant = ApprovalGrant::from_action(&exact);
        let mut approvals = ApprovalTable::default();
        approvals.approve(grant.clone()).unwrap();
        assert!(approvals.revoke(&grant));
        assert!(!approvals.allows(&exact));
    }

    #[test]
    fn matching_hash_cannot_hide_different_scope_bytes() {
        let exact = action(3, "/project", 1);
        let mut grant = ApprovalGrant::from_action(&exact);
        grant.scope[0] ^= 1;
        let mut approvals = ApprovalTable::default();
        approvals.grants.push(grant);

        assert!(!approvals.allows(&exact));
    }

    #[test]
    fn malformed_scope_fingerprint_is_rejected_before_storage() {
        let exact = action(3, "/project", 1);
        let mut grant = ApprovalGrant::from_action(&exact);
        grant.scope.push(0);

        assert!(ApprovalTable::default().approve(grant).is_err());
    }

    #[test]
    fn denial_explains_effects_in_words() {
        let policy = Policy {
            allowed: Effects::READ,
            ..Policy::default()
        };
        let error = policy.check(&action(3, "/project", 1)).unwrap_err();
        assert!(error.contains("write"));
        assert!(error.contains("Say “do it”"));
        assert!(!error.contains("mask"));
    }
}
