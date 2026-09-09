use crate::codec::*;
use crate::storage::{self, Store};
use crate::types::{EntityId, IntentId};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

const MAGIC: &[u8; 8] = b"DAMON\0\x06\0";
const STRATEGY: &[u8; 8] = b"DAMON\0\x05\0";
const MEMO: &[u8; 8] = b"DAMON\0\x04\0";
const WORLD: &[u8; 8] = b"DAMON\0\x03\0";
const GRAPHS: &[u8; 8] = b"DAMON\0\x02\0";
const LEGACY: &[u8; 8] = b"DAMON\0\x01\0";

#[derive(Debug, Clone)]
pub struct Entity {
    pub id: EntityId,
    pub kind: u16,
    pub flags: u64,
    pub name: String,
    pub value: String,
    pub version: u32,
}
#[derive(Debug, Clone)]
pub struct Experience {
    pub input_hash: u64,
    pub intent: IntentId,
    pub reward: i16,
    pub confidence: u8,
}
#[derive(Debug)]
pub struct DamonData {
    store: Option<Store>,
    pub entities: Vec<Entity>,
    pub names: HashMap<String, EntityId>,
    pub language_counts: HashMap<u64, Vec<(IntentId, u32)>>,
    pub experiences: Vec<Experience>,
    pub learned_graphs: HashMap<u64, crate::types::MeaningGraph>,
    pub world: crate::world::World,
    pub memo: crate::cache::MemoTable,
    pub strategies: crate::strategy::StrategyTable,
    pub capabilities: crate::capability::CapabilityGraph,
}

impl DamonData {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let (store, payload) = Store::open(path.as_ref(), |bytes| Self::decode(bytes).map(|_| ()))?;
        let fresh = payload.is_none();
        let mut data = match payload {
            Some(bytes) => Self::decode(&bytes)?,
            None => Self::empty(),
        };
        data.store = Some(store);
        if fresh {
            data.seed()?;
            data.save()?;
        }
        Ok(data)
    }
    fn empty() -> Self {
        Self {
            store: None,
            entities: Vec::new(),
            names: HashMap::new(),
            language_counts: HashMap::new(),
            experiences: Vec::new(),
            learned_graphs: HashMap::new(),
            world: crate::world::World::default(),
            memo: crate::cache::MemoTable::default(),
            strategies: crate::strategy::StrategyTable::default(),
            capabilities: crate::capability::CapabilityGraph::default(),
        }
    }
    pub fn generation(&self) -> u64 {
        self.store.as_ref().map_or(0, |s| s.generation)
    }
    pub fn recovery_warnings(&self) -> &[String] {
        self.store.as_ref().map_or(&[], |s| s.warnings.as_slice())
    }
    fn seed(&mut self) -> io::Result<()> {
        let cwd = std::env::current_dir()?;
        self.add_entity(
            crate::world::PROJECT,
            "damon",
            cwd.to_str()
                .ok_or_else(|| storage::invalid("project path must be UTF-8"))?,
        );
        let host = self.add_entity(crate::world::HOST, "local host", "");
        self.link(EntityId(0), crate::world::Relationship::RunsOn, host)?;
        for (name, value) in [
            ("git status tool", "1"),
            ("git diff tool", "2"),
            ("test tool", "3"),
            ("file listing tool", "4"),
        ] {
            let tool = self.add_entity(crate::world::TOOL, name, value);
            self.link(EntityId(0), crate::world::Relationship::Uses, tool)?;
        }
        for name in ["tests", "files", "changes"] {
            self.add_entity(crate::world::CONCEPT, name, "");
        }
        self.world.context.focus(EntityId(0));
        Ok(())
    }
    pub fn add_entity(&mut self, kind: u16, name: &str, value: &str) -> EntityId {
        if let Some(id) = self.names.get(&name.to_ascii_lowercase()) {
            return *id;
        }
        let id = EntityId(self.entities.len() as u32);
        self.entities.push(Entity {
            id,
            kind,
            flags: 0,
            name: name.to_string(),
            value: value.to_string(),
            version: 1,
        });
        self.names.insert(name.to_ascii_lowercase(), id);
        id
    }
    pub fn resolve(&self, name: &str) -> Option<EntityId> {
        self.names.get(&name.to_ascii_lowercase()).copied()
    }
    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(id.0 as usize)
    }
    pub fn register_project(&mut self, name: &str, path: &Path) -> io::Result<EntityId> {
        let key = name.trim().to_ascii_lowercase();
        if !crate::world::valid_name(&key) {
            return Err(storage::invalid(
                "project name must be 1-128 characters, at most eight words",
            ));
        }
        let path = fs::canonicalize(path)?;
        if !path.is_dir() {
            return Err(storage::invalid("project path is not a directory"));
        }
        let value = path
            .to_str()
            .ok_or_else(|| storage::invalid("project path must be UTF-8"))?;
        if let Some(id) = self.resolve(&key) {
            let e = self
                .entity(id)
                .ok_or_else(|| storage::invalid("invalid entity"))?;
            if e.kind != crate::world::PROJECT || e.value != value {
                return Err(storage::invalid(
                    "name already identifies a different entity or project path",
                ));
            }
            return Ok(id);
        }
        self.world.changed()?;
        let id = self.add_entity(crate::world::PROJECT, &key, value);
        self.world.rebuild(self.entities.len())?;
        Ok(id)
    }
    pub fn add_alias(&mut self, name: &str, id: EntityId) -> io::Result<()> {
        let key = name.trim().to_ascii_lowercase();
        if !crate::world::valid_name(&key) || self.entity(id).is_none() {
            return Err(storage::invalid("invalid alias or entity"));
        }
        if let Some(existing) = self.resolve(&key) {
            return if existing == id {
                Ok(())
            } else {
                Err(storage::invalid("alias already identifies another entity"))
            };
        }
        if self.world.aliases.len() >= 8192 {
            return Err(storage::invalid("alias limit reached"));
        }
        self.world.changed()?;
        self.world.aliases.push(crate::world::Alias {
            name: key.clone(),
            entity: id,
        });
        self.world.rebuild(self.entities.len())?;
        self.names.insert(key, id);
        Ok(())
    }
    pub fn link(
        &mut self,
        source: EntityId,
        relation: crate::world::Relationship,
        target: EntityId,
    ) -> io::Result<()> {
        if self.entity(source).is_none() || self.entity(target).is_none() {
            return Err(storage::invalid("unknown relationship endpoint"));
        }
        let link = crate::world::Link {
            source,
            relation: relation as u16,
            target,
        };
        if self.world.links.contains(&link) {
            return Ok(());
        }
        if self.world.links.len() >= 65536 {
            return Err(storage::invalid("world relationship limit reached"));
        }
        self.world.changed()?;
        self.world.links.push(link);
        self.world.rebuild(self.entities.len())
    }
    pub fn update_entity(&mut self, id: EntityId, value: &str) -> io::Result<()> {
        let e = self
            .entity(id)
            .ok_or_else(|| storage::invalid("unknown entity"))?;
        if e.value == value {
            return Ok(());
        }
        let version = e
            .version
            .checked_add(1)
            .ok_or_else(|| storage::invalid("entity version exhausted"))?;
        self.world.changed()?;
        self.entities[id.0 as usize].value = value.into();
        self.entities[id.0 as usize].version = version;
        self.memo.invalidate_entity(id);
        Ok(())
    }
    pub fn observe_language(
        &mut self,
        feature: u64,
        intent: IntentId,
        reward: i16,
        confidence: u8,
    ) {
        if !self.language_counts.contains_key(&feature) && self.language_counts.len() >= 8192 {
            // Retain the most useful phrases; hashes break ties deterministically.
            if let Some(key) = self
                .language_counts
                .iter()
                .min_by_key(|(key, row)| {
                    (row.iter().map(|(_, c)| u64::from(*c)).sum::<u64>(), **key)
                })
                .map(|(key, _)| *key)
            {
                self.language_counts.remove(&key);
            }
        }
        let row = self.language_counts.entry(feature).or_default();
        if reward > 0 {
            if let Some((_, count)) = row.iter_mut().find(|(id, _)| *id == intent) {
                *count = count.saturating_add(1);
            } else {
                row.push((intent, 1));
            }
        }
        self.experiences.push(Experience {
            input_hash: feature,
            intent,
            reward,
            confidence,
        });
        if self.experiences.len() > 4096 {
            self.experiences.drain(..1024);
        }
    }
    pub fn remember_meaning(&mut self, feature: u64, meaning: &crate::types::MeaningGraph) {
        if self.learned_graphs.len() >= 4096 && !self.learned_graphs.contains_key(&feature) {
            if let Some(key) = self.learned_graphs.keys().min().copied() {
                self.learned_graphs.remove(&key);
            }
        }
        self.learned_graphs.insert(feature, meaning.clone());
    }
    pub fn language_candidates(&self, feature: u64) -> &[(IntentId, u32)] {
        self.language_counts
            .get(&feature)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    pub fn save(&mut self) -> io::Result<()> {
        let bytes = self.encode()?;
        self.store
            .as_mut()
            .ok_or_else(|| storage::invalid("detached brain"))?
            .commit(&bytes)
    }
    pub fn compact(&mut self) -> io::Result<()> {
        let bytes = self.encode()?;
        self.store
            .as_mut()
            .ok_or_else(|| storage::invalid("detached brain"))?
            .compact(&bytes)
    }
    pub fn export(&self, path: impl AsRef<Path>) -> io::Result<()> {
        self.store
            .as_ref()
            .ok_or_else(|| storage::invalid("detached brain"))?
            .export(path.as_ref(), &self.encode()?)
    }
    pub fn restore(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        use std::io::Read;
        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take(storage::MAX_IMAGE as u64 + 33)
            .read_to_end(&mut bytes)?;
        let (_, payload, n) = storage::unframe(&bytes)?;
        if n != bytes.len() {
            return Err(storage::invalid("backup has trailing bytes"));
        }
        let mut replacement = Self::decode(payload)?;
        // Commit before replacing in-memory state; generations never roll back.
        self.store
            .as_mut()
            .ok_or_else(|| storage::invalid("detached brain"))?
            .compact(payload)?;
        replacement.store = self.store.take();
        *self = replacement;
        Ok(())
    }
    fn encode(&self) -> io::Result<Vec<u8>> {
        let mut f = Vec::new();
        f.write_all(MAGIC)?;
        write_u32(&mut f, self.entities.len() as u32)?;
        for e in &self.entities {
            write_u16(&mut f, e.kind)?;
            write_u64(&mut f, e.flags)?;
            write_u32(&mut f, e.version)?;
            write_string(&mut f, &e.name)?;
            write_string(&mut f, &e.value)?;
        }
        write_u32(&mut f, self.language_counts.len() as u32)?;
        let mut rows: Vec<_> = self.language_counts.iter().collect();
        rows.sort_by_key(|(k, _)| **k);
        for (feature, counts) in rows {
            write_u64(&mut f, *feature)?;
            write_u32(&mut f, counts.len() as u32)?;
            for (intent, count) in counts {
                write_u32(&mut f, intent.0)?;
                write_u32(&mut f, *count)?;
            }
        }
        write_u32(&mut f, self.experiences.len() as u32)?;
        for x in &self.experiences {
            write_u64(&mut f, x.input_hash)?;
            write_u32(&mut f, x.intent.0)?;
            write_i16(&mut f, x.reward)?;
            f.write_all(&[x.confidence])?;
        }
        write_u32(&mut f, self.learned_graphs.len() as u32)?;
        let mut graphs = self.learned_graphs.iter().collect::<Vec<_>>();
        graphs.sort_by_key(|(key, _)| **key);
        for (key, meaning) in graphs {
            crate::semantics::validate(meaning, self).map_err(|e| storage::invalid(&e))?;
            write_u64(&mut f, *key)?;
            f.write_all(&[meaning.confidence])?;
            write_u32(&mut f, meaning.nodes.len() as u32)?;
            for node in &meaning.nodes {
                let kind = match node.kind {
                    crate::graph::NodeKind::Action => 0,
                    crate::graph::NodeKind::Entity => 1,
                    crate::graph::NodeKind::Concept => 2,
                    crate::graph::NodeKind::Time => 3,
                    crate::graph::NodeKind::Condition => 4,
                };
                f.write_all(&[kind])?;
                write_u32(&mut f, node.value)?;
            }
            write_u32(&mut f, meaning.edges.len() as u32)?;
            for edge in &meaning.edges {
                write_u32(&mut f, edge.source)?;
                write_u16(&mut f, edge.relation)?;
                write_u32(&mut f, edge.target)?;
            }
        }
        let mut world = self.world.clone();
        world.rebuild(self.entities.len())?;
        world.encode(&mut f)?;
        self.memo.encode(&mut f)?;
        self.strategies.encode(&mut f)?;
        self.capabilities.encode(&mut f)?;
        if f.len() > storage::MAX_IMAGE {
            return Err(storage::invalid("brain image exceeds size limit"));
        }
        Ok(f)
    }
    fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut r = Reader { bytes, pos: 0 };
        let header = r.take(8)?;
        if header != MAGIC
            && header != STRATEGY
            && header != MEMO
            && header != WORLD
            && header != LEGACY
            && header != GRAPHS
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid damon.data header",
            ));
        }
        let mut data = Self::empty();
        let n = r.u32()? as usize;
        for _ in 0..n {
            let kind = r.u16()?;
            let flags = r.u64()?;
            let version = r.u32()?;
            let name = r.string()?;
            let value = r.string()?;
            let id = EntityId(data.entities.len() as u32);
            data.names.insert(name.to_ascii_lowercase(), id);
            data.entities.push(Entity {
                id,
                kind,
                flags,
                name,
                value,
                version,
            });
        }
        let rows = r.u32()? as usize;
        for _ in 0..rows {
            let feature = r.u64()?;
            let len = r.u32()? as usize;
            let mut counts = Vec::new();
            for _ in 0..len {
                counts.push((IntentId(r.u32()?), r.u32()?));
            }
            data.language_counts.insert(feature, counts);
        }
        let exp = r.u32()? as usize;
        for _ in 0..exp {
            data.experiences.push(Experience {
                input_hash: r.u64()?,
                intent: IntentId(r.u32()?),
                reward: r.i16()?,
                confidence: r.u8()?,
            });
        }
        if header == MAGIC
            || header == STRATEGY
            || header == MEMO
            || header == WORLD
            || header == GRAPHS
        {
            let count = r.u32()?;
            if count > 4096 {
                return Err(storage::invalid("too many learned graphs"));
            }
            for _ in 0..count {
                let key = r.u64()?;
                let confidence = r.u8()?;
                let count = r.u32()?;
                if count > 32 {
                    return Err(storage::invalid("too many graph nodes"));
                }
                let mut nodes = Vec::new();
                for _ in 0..count {
                    let kind = match r.u8()? {
                        0 => crate::graph::NodeKind::Action,
                        1 => crate::graph::NodeKind::Entity,
                        2 => crate::graph::NodeKind::Concept,
                        3 => crate::graph::NodeKind::Time,
                        4 => crate::graph::NodeKind::Condition,
                        _ => return Err(storage::invalid("unknown graph node")),
                    };
                    nodes.push(crate::graph::Node {
                        kind,
                        value: r.u32()?,
                    });
                }
                let count = r.u32()?;
                if count > 64 {
                    return Err(storage::invalid("too many graph edges"));
                }
                let mut edges = Vec::new();
                for _ in 0..count {
                    edges.push(crate::types::MeaningEdge {
                        source: r.u32()?,
                        relation: r.u16()?,
                        target: r.u32()?,
                    });
                }
                let meaning = crate::semantics::from_parts(nodes, edges, confidence)
                    .map_err(|e| storage::invalid(&e))?;
                crate::semantics::validate(&meaning, &data).map_err(|e| storage::invalid(&e))?;
                if data.learned_graphs.insert(key, meaning).is_some() {
                    return Err(storage::invalid("duplicate learned graph"));
                }
            }
        }
        if header == MAGIC || header == STRATEGY || header == MEMO || header == WORLD {
            data.world = crate::world::World::decode(&mut r, data.entities.len())?;
        } else {
            data.world.rebuild(data.entities.len())?;
        }
        if header == MAGIC || header == STRATEGY || header == MEMO {
            data.memo = crate::cache::MemoTable::decode(&mut r, &data.entities)?;
        }
        if header == MAGIC || header == STRATEGY {
            data.strategies = crate::strategy::StrategyTable::decode(&mut r)?;
        }
        if header == MAGIC {
            data.capabilities = crate::capability::CapabilityGraph::decode(&mut r)?;
        }
        for alias in &data.world.aliases {
            if data
                .names
                .insert(alias.name.clone(), alias.entity)
                .is_some()
            {
                return Err(storage::invalid("alias conflicts with a name"));
            }
        }
        if r.pos != bytes.len() {
            return Err(storage::invalid("trailing payload bytes"));
        }
        if data.names.len() != data.entities.len() + data.world.aliases.len() {
            return Err(storage::invalid("duplicate entity names"));
        }
        Ok(data)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_data() {
        let p = std::env::temp_dir().join(format!("damon-test-{}.data", std::process::id()));
        for suffix in ["", ".journal", ".prev", ".lock"] {
            let _ = fs::remove_file(format!("{}{}", p.display(), suffix));
        }
        let mut d = DamonData::open(&p).unwrap();
        let id = d.add_entity(1, "cpython", "/tmp/cpython");
        d.observe_language(123, IntentId(7), 1, 240);
        d.save().unwrap();
        drop(d);
        let d2 = DamonData::open(&p).unwrap();
        assert_eq!(d2.resolve("cpython"), Some(id));
        assert_eq!(d2.language_candidates(123), &[(IntentId(7), 1)]);
        for suffix in ["", ".journal", ".prev", ".lock"] {
            let _ = fs::remove_file(format!("{}{}", p.display(), suffix));
        }
    }

    #[test]
    fn revision_three_world_image_migrates_to_memo_payload() {
        let p =
            std::env::temp_dir().join(format!("damon-v3-migration-{}.data", std::process::id()));
        for suffix in ["", ".journal", ".prev", ".lock"] {
            let _ = fs::remove_file(format!("{}{}", p.display(), suffix));
        }
        let d = DamonData::open(&p).unwrap();
        let (memo_len, strategy_len, capability_len) = tail_lengths(&d);
        drop(d);
        let image = fs::read(&p).unwrap();
        let (generation, payload, _) = storage::unframe(&image).unwrap();
        assert_eq!(&payload[..8], MAGIC);
        let mut old = payload[..payload.len() - memo_len - strategy_len - capability_len].to_vec();
        old[..8].copy_from_slice(WORLD);
        fs::write(&p, storage::frame(generation, &old).unwrap()).unwrap();

        let mut migrated = DamonData::open(&p).unwrap();
        assert!(migrated.memo.entries.is_empty());
        migrated.compact().unwrap();
        drop(migrated);
        let image = fs::read(&p).unwrap();
        let (_, payload, _) = storage::unframe(&image).unwrap();
        assert_eq!(&payload[..8], MAGIC);
        for suffix in ["", ".journal", ".prev", ".lock"] {
            let _ = fs::remove_file(format!("{}{}", p.display(), suffix));
        }
    }

    #[test]
    fn revision_four_memo_image_migrates_to_strategy_payload() {
        let p =
            std::env::temp_dir().join(format!("damon-v4-migration-{}.data", std::process::id()));
        for suffix in ["", ".journal", ".prev", ".lock"] {
            let _ = fs::remove_file(format!("{}{}", p.display(), suffix));
        }
        let d = DamonData::open(&p).unwrap();
        let (_, strategy_len, capability_len) = tail_lengths(&d);
        drop(d);
        let image = fs::read(&p).unwrap();
        let (generation, payload, _) = storage::unframe(&image).unwrap();
        let mut old = payload[..payload.len() - strategy_len - capability_len].to_vec();
        old[..8].copy_from_slice(MEMO);
        fs::write(&p, storage::frame(generation, &old).unwrap()).unwrap();

        let mut migrated = DamonData::open(&p).unwrap();
        assert!(migrated.strategies.stats.is_empty());
        migrated.compact().unwrap();
        drop(migrated);
        let image = fs::read(&p).unwrap();
        let (_, payload, _) = storage::unframe(&image).unwrap();
        assert_eq!(&payload[..8], MAGIC);
        for suffix in ["", ".journal", ".prev", ".lock"] {
            let _ = fs::remove_file(format!("{}{}", p.display(), suffix));
        }
    }

    #[test]
    fn revision_five_strategy_image_migrates_to_capability_payload() {
        let p =
            std::env::temp_dir().join(format!("damon-v5-migration-{}.data", std::process::id()));
        for suffix in ["", ".journal", ".prev", ".lock"] {
            let _ = fs::remove_file(format!("{}{}", p.display(), suffix));
        }
        let d = DamonData::open(&p).unwrap();
        let (_, _, capability_len) = tail_lengths(&d);
        drop(d);
        let image = fs::read(&p).unwrap();
        let (generation, payload, _) = storage::unframe(&image).unwrap();
        let mut old = payload[..payload.len() - capability_len].to_vec();
        old[..8].copy_from_slice(STRATEGY);
        fs::write(&p, storage::frame(generation, &old).unwrap()).unwrap();

        let mut migrated = DamonData::open(&p).unwrap();
        assert_eq!(
            migrated
                .capabilities
                .resolve_tool(crate::capability::RUN_TESTS),
            Some(crate::types::ToolId(3))
        );
        migrated.compact().unwrap();
        drop(migrated);
        let image = fs::read(&p).unwrap();
        let (_, payload, _) = storage::unframe(&image).unwrap();
        assert_eq!(&payload[..8], MAGIC);
        for suffix in ["", ".journal", ".prev", ".lock"] {
            let _ = fs::remove_file(format!("{}{}", p.display(), suffix));
        }
    }

    fn tail_lengths(data: &DamonData) -> (usize, usize, usize) {
        let mut memo = Vec::new();
        data.memo.encode(&mut memo).unwrap();
        let mut strategy = Vec::new();
        data.strategies.encode(&mut strategy).unwrap();
        let mut capability = Vec::new();
        data.capabilities.encode(&mut capability).unwrap();
        (memo.len(), strategy.len(), capability.len())
    }
}
