//! Persistent world records and conversation state, indexed by compact IDs.
use crate::{
    codec::*,
    storage,
    types::{EntityId, IntentId},
};
use std::io::{self, Write};

pub const PROJECT: u16 = 1;
pub const FILE: u16 = 2;
pub const TOOL: u16 = 3;
pub const PROCEDURE: u16 = 4;
pub const HOST: u16 = 5;
pub const PROCESS: u16 = 6;
pub const CONCEPT: u16 = 7;
pub const DIRECTORY: u16 = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias {
    pub name: String,
    pub entity: EntityId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub source: EntityId,
    pub relation: u16,
    pub target: EntityId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Relationship {
    Contains = 1,
    Uses = 2,
    RunsOn = 3,
    Implements = 4,
    RelatedTo = 5,
    Produces = 6,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub focus: Option<EntityId>,
    pub previous_target: Option<EntityId>,
    pub previous_action: Option<IntentId>,
    pub previous_feature: Option<u64>,
    pub recent: Vec<EntityId>,
}
impl Context {
    pub fn focus(&mut self, id: EntityId) {
        self.focus = Some(id);
        self.recent.retain(|v| *v != id);
        self.recent.insert(0, id);
        self.recent.truncate(8);
    }
    pub fn observe(&mut self, target: EntityId, action: IntentId, feature: u64) {
        self.previous_target = Some(target);
        self.previous_action = Some(action);
        self.previous_feature = Some(feature);
        self.focus(target);
    }
}
#[derive(Clone, Debug, Default)]
pub struct World {
    pub aliases: Vec<Alias>,
    pub links: Vec<Link>,
    offsets: Vec<u32>,
    pub context: Context,
    pub version: u64,
}
impl World {
    pub fn changed(&mut self) -> io::Result<()> {
        self.version = self
            .version
            .checked_add(1)
            .ok_or_else(|| storage::invalid("world version exhausted"))?;
        Ok(())
    }
    pub fn rebuild(&mut self, count: usize) -> io::Result<()> {
        if self.aliases.len() > 8192 || self.links.len() > 65536 {
            return Err(storage::invalid("world record limit exceeded"));
        }
        if self
            .aliases
            .iter()
            .any(|a| a.entity.0 as usize >= count || !valid_name(&a.name))
        {
            return Err(storage::invalid("invalid alias"));
        }
        self.aliases.sort_by(|a, b| a.name.cmp(&b.name));
        if self.aliases.windows(2).any(|v| v[0].name == v[1].name) {
            return Err(storage::invalid("duplicate alias"));
        }
        self.links
            .sort_by_key(|e| (e.source.0, e.relation, e.target.0));
        if self.links.windows(2).any(|v| v[0] == v[1]) {
            return Err(storage::invalid("duplicate world relationship"));
        }
        if self.links.iter().any(|e| {
            e.source.0 as usize >= count
                || e.target.0 as usize >= count
                || !(1..=6).contains(&e.relation)
        }) {
            return Err(storage::invalid("invalid world relationship"));
        }
        self.offsets = vec![0; count + 1];
        for link in &self.links {
            self.offsets[link.source.0 as usize + 1] += 1;
        }
        for i in 1..self.offsets.len() {
            self.offsets[i] += self.offsets[i - 1];
        }
        let c = &self.context;
        if c.recent.len() > 8
            || c.recent
                .iter()
                .chain(c.focus.iter())
                .chain(c.previous_target.iter())
                .any(|id| id.0 as usize >= count)
        {
            return Err(storage::invalid("invalid conversation entity reference"));
        }
        if c.previous_action.is_some_and(|id| id.0 == 0) {
            return Err(storage::invalid("invalid previous action"));
        }
        Ok(())
    }
    pub fn neighbors(&self, id: EntityId) -> &[Link] {
        let index = id.0 as usize;
        if index + 1 >= self.offsets.len() {
            return &[];
        }
        &self.links[self.offsets[index] as usize..self.offsets[index + 1] as usize]
    }
    pub(crate) fn encode(&self, w: &mut impl Write) -> io::Result<()> {
        write_u64(w, self.version)?;
        write_u32(w, self.aliases.len() as u32)?;
        for alias in &self.aliases {
            write_string(w, &alias.name)?;
            write_u32(w, alias.entity.0)?;
        }
        write_u32(w, self.links.len() as u32)?;
        for link in &self.links {
            write_u32(w, link.source.0)?;
            write_u16(w, link.relation)?;
            write_u32(w, link.target.0)?;
        }
        let c = &self.context;
        write_u32(w, c.focus.map_or(u32::MAX, |id| id.0))?;
        write_u32(w, c.previous_target.map_or(u32::MAX, |id| id.0))?;
        write_u32(w, c.previous_action.map_or(u32::MAX, |id| id.0))?;
        w.write_all(&[u8::from(c.previous_feature.is_some())])?;
        write_u64(w, c.previous_feature.unwrap_or(0))?;
        write_u32(w, c.recent.len() as u32)?;
        for id in &c.recent {
            write_u32(w, id.0)?;
        }
        Ok(())
    }
    pub(crate) fn decode(r: &mut Reader<'_>, count: usize) -> io::Result<Self> {
        let version = r.u64()?;
        let n = r.u32()?;
        if n > 8192 {
            return Err(storage::invalid("too many aliases"));
        }
        let mut aliases = Vec::new();
        for _ in 0..n {
            aliases.push(Alias {
                name: r.string()?,
                entity: EntityId(r.u32()?),
            });
        }
        let n = r.u32()?;
        if n > 65536 {
            return Err(storage::invalid("too many world links"));
        }
        let mut links = Vec::new();
        for _ in 0..n {
            links.push(Link {
                source: EntityId(r.u32()?),
                relation: r.u16()?,
                target: EntityId(r.u32()?),
            });
        }
        let optional = |v| if v == u32::MAX { None } else { Some(v) };
        let focus = optional(r.u32()?).map(EntityId);
        let previous_target = optional(r.u32()?).map(EntityId);
        let previous_action = optional(r.u32()?).map(IntentId);
        let present = r.u8()?;
        if present > 1 {
            return Err(storage::invalid("invalid optional feature flag"));
        }
        let feature = r.u64()?;
        let previous_feature = if present == 1 { Some(feature) } else { None };
        let n = r.u32()?;
        if n > 8 {
            return Err(storage::invalid("too many recent references"));
        }
        let mut recent = Vec::new();
        for _ in 0..n {
            recent.push(EntityId(r.u32()?));
        }
        let mut world = Self {
            aliases,
            links,
            offsets: Vec::new(),
            context: Context {
                focus,
                previous_target,
                previous_action,
                previous_feature,
                recent,
            },
            version,
        };
        world.rebuild(count)?;
        Ok(world)
    }
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.split_whitespace().count() <= 8
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || " -_.".contains(c))
        && !matches!(name, "it" | "that" | "this" | "its" | "they" | "there")
}
