//! Small dependency-aware memo table for deterministic discovery and results.
//!
//! Entries and dependencies are stored in flat arrays. The hash index is derived
//! on load and is never part of the persistent format.
use crate::{
    codec::{write_string, write_u16, write_u32, write_u64, Reader},
    data::Entity,
    storage,
    types::EntityId,
};
use std::{collections::HashMap, io, io::Write};

pub const TEST_COMMAND: u8 = 1;
pub const VERIFICATION_PLAN: u8 = 2;
pub const FILE_LIST: u8 = 3;
const MAX_ENTRIES: usize = 1024;
const MAX_DEPENDENCIES: usize = 4096;
const MAX_VALUE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dependency {
    pub entity: EntityId,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: u8,
    pub key: u64,
    pub state_hash: u64,
    pub dependency_start: u32,
    pub dependency_len: u16,
    pub hits: u32,
    pub value: String,
}

#[derive(Clone, Debug, Default)]
pub struct MemoTable {
    pub entries: Vec<Entry>,
    pub dependencies: Vec<Dependency>,
    index: HashMap<(u8, u64), usize>,
}

impl MemoTable {
    pub fn get(
        &mut self,
        kind: u8,
        key: u64,
        state_hash: u64,
        entities: &[Entity],
    ) -> Option<&str> {
        let index = *self.index.get(&(kind, key))?;
        if self.entries[index].state_hash != state_hash || !self.dependencies_valid(index, entities)
        {
            self.remove(index);
            return None;
        }
        self.entries[index].hits = self.entries[index].hits.saturating_add(1);
        Some(&self.entries[index].value)
    }

    pub fn put(
        &mut self,
        kind: u8,
        key: u64,
        state_hash: u64,
        dependencies: &[EntityId],
        value: String,
        entities: &[Entity],
    ) -> io::Result<()> {
        if !matches!(kind, TEST_COMMAND | VERIFICATION_PLAN | FILE_LIST) {
            return Err(storage::invalid("unknown memo kind"));
        }
        if value.len() > MAX_VALUE_BYTES {
            return Err(storage::invalid("memo value is too large"));
        }
        let mut deps = Vec::with_capacity(dependencies.len());
        for id in dependencies {
            let entity = entities
                .get(id.0 as usize)
                .ok_or_else(|| storage::invalid("unknown memo dependency"))?;
            deps.push(Dependency {
                entity: *id,
                version: entity.version,
            });
        }
        deps.sort_by_key(|d| d.entity.0);
        deps.dedup_by_key(|d| d.entity.0);
        if deps.len() > u16::MAX as usize {
            return Err(storage::invalid("too many memo dependencies"));
        }
        if let Some(index) = self.index.get(&(kind, key)).copied() {
            self.remove(index);
        }
        while self.entries.len() >= MAX_ENTRIES
            || self.dependencies.len() + deps.len() > MAX_DEPENDENCIES
        {
            let Some(index) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| (entry.hits, entry.kind, entry.key))
                .map(|(index, _)| index)
            else {
                return Err(storage::invalid("memo dependency limit reached"));
            };
            self.remove(index);
        }
        let dependency_start = self.dependencies.len() as u32;
        let dependency_len = deps.len() as u16;
        self.dependencies.extend(deps);
        self.entries.push(Entry {
            kind,
            key,
            state_hash,
            dependency_start,
            dependency_len,
            hits: 0,
            value,
        });
        self.rebuild_index();
        Ok(())
    }

    pub fn invalidate_entity(&mut self, entity: EntityId) {
        let mut keep = Vec::with_capacity(self.entries.len());
        for index in 0..self.entries.len() {
            if !self
                .dependencies_for(index)
                .iter()
                .any(|d| d.entity == entity)
            {
                keep.push(index);
            }
        }
        self.retain(&keep);
    }

    fn dependencies_for(&self, index: usize) -> &[Dependency] {
        let entry = &self.entries[index];
        let start = entry.dependency_start as usize;
        &self.dependencies[start..start + entry.dependency_len as usize]
    }

    fn dependencies_valid(&self, index: usize, entities: &[Entity]) -> bool {
        self.dependencies_for(index).iter().all(|dependency| {
            entities
                .get(dependency.entity.0 as usize)
                .is_some_and(|entity| entity.version == dependency.version)
        })
    }

    fn remove(&mut self, index: usize) {
        let keep = (0..self.entries.len())
            .filter(|candidate| *candidate != index)
            .collect::<Vec<_>>();
        self.retain(&keep);
    }

    fn retain(&mut self, keep: &[usize]) {
        let mut entries = Vec::with_capacity(keep.len());
        let mut dependencies = Vec::new();
        for old_index in keep {
            let old = &self.entries[*old_index];
            let start = dependencies.len() as u32;
            let deps = self.dependencies_for(*old_index);
            dependencies.extend_from_slice(deps);
            let mut entry = old.clone();
            entry.dependency_start = start;
            entries.push(entry);
        }
        self.entries = entries;
        self.dependencies = dependencies;
        self.rebuild_index();
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        for (index, entry) in self.entries.iter().enumerate() {
            self.index.insert((entry.kind, entry.key), index);
        }
    }

    pub(crate) fn encode(&self, writer: &mut impl Write) -> io::Result<()> {
        write_u32(writer, self.dependencies.len() as u32)?;
        for dependency in &self.dependencies {
            write_u32(writer, dependency.entity.0)?;
            write_u32(writer, dependency.version)?;
        }
        write_u32(writer, self.entries.len() as u32)?;
        let mut indexes = (0..self.entries.len()).collect::<Vec<_>>();
        indexes.sort_by_key(|index| {
            let entry = &self.entries[*index];
            (entry.kind, entry.key)
        });
        for index in indexes {
            let entry = &self.entries[index];
            writer.write_all(&[entry.kind])?;
            write_u64(writer, entry.key)?;
            write_u64(writer, entry.state_hash)?;
            write_u32(writer, entry.dependency_start)?;
            write_u16(writer, entry.dependency_len)?;
            write_u32(writer, entry.hits)?;
            write_string(writer, &entry.value)?;
        }
        Ok(())
    }

    pub(crate) fn decode(reader: &mut Reader<'_>, entities: &[Entity]) -> io::Result<Self> {
        let dependency_count = reader.u32()? as usize;
        if dependency_count > MAX_DEPENDENCIES {
            return Err(storage::invalid("too many memo dependencies"));
        }
        let mut dependencies = Vec::with_capacity(dependency_count);
        for _ in 0..dependency_count {
            dependencies.push(Dependency {
                entity: EntityId(reader.u32()?),
                version: reader.u32()?,
            });
        }
        let entry_count = reader.u32()? as usize;
        if entry_count > MAX_ENTRIES {
            return Err(storage::invalid("too many memo entries"));
        }
        let mut entries = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let entry = Entry {
                kind: reader.u8()?,
                key: reader.u64()?,
                state_hash: reader.u64()?,
                dependency_start: reader.u32()?,
                dependency_len: reader.u16()?,
                hits: reader.u32()?,
                value: reader.string()?,
            };
            if !matches!(entry.kind, TEST_COMMAND | VERIFICATION_PLAN | FILE_LIST)
                || entry.value.len() > MAX_VALUE_BYTES
                || entry.dependency_start as usize > dependencies.len()
                || entry.dependency_len as usize
                    > dependencies.len() - entry.dependency_start as usize
            {
                return Err(storage::invalid("invalid memo entry"));
            }
            entries.push(entry);
        }
        let mut table = Self {
            entries,
            dependencies,
            index: HashMap::new(),
        };
        table.rebuild_index();
        if table.index.len() != table.entries.len() {
            return Err(storage::invalid("duplicate memo record"));
        }
        // Cache data is derived. Stale dependencies invalidate an entry rather
        // than making the authoritative brain image unreadable.
        let keep = (0..table.entries.len())
            .filter(|index| table.dependencies_valid(*index, entities))
            .collect::<Vec<_>>();
        table.retain(&keep);
        Ok(table)
    }
}

pub fn key_for_project(project: EntityId) -> u64 {
    u64::from(project.0)
}

pub fn hash_bytes(parts: &[&[u8]]) -> u64 {
    // Explicit FNV-1a is stable across processes and Rust releases.
    let mut hash = 0xcbf29ce484222325_u64;
    for part in parts {
        for byte in *part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
