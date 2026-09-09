use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use crate::types::{EntityId, IntentId};

const MAGIC: &[u8; 8] = b"DAMON\0\x01\0";

#[derive(Debug, Clone)]
pub struct Entity { pub id: EntityId, pub kind: u16, pub flags: u64, pub name: String, pub value: String, pub version: u32 }
#[derive(Debug, Clone)]
pub struct Experience { pub input_hash: u64, pub intent: IntentId, pub reward: i16, pub confidence: u8 }
#[derive(Debug)]
pub struct DamonData {
    path: PathBuf,
    pub entities: Vec<Entity>,
    pub names: HashMap<String, EntityId>,
    pub language_counts: HashMap<u64, Vec<(IntentId, u32)>>,
    pub experiences: Vec<Experience>,
}

impl DamonData {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() { let mut data = Self::empty(path); data.seed(); data.save()?; return Ok(data); }
        let mut bytes = Vec::new(); File::open(&path)?.read_to_end(&mut bytes)?; Self::decode(path, &bytes)
    }
    fn empty(path: PathBuf) -> Self { Self { path, entities: Vec::new(), names: HashMap::new(), language_counts: HashMap::new(), experiences: Vec::new() } }
    fn seed(&mut self) { self.add_entity(1, "damon", "."); }
    pub fn add_entity(&mut self, kind: u16, name: &str, value: &str) -> EntityId {
        if let Some(id) = self.names.get(&name.to_ascii_lowercase()) { return *id; }
        let id = EntityId(self.entities.len() as u32);
        self.entities.push(Entity { id, kind, flags: 0, name: name.to_string(), value: value.to_string(), version: 1 });
        self.names.insert(name.to_ascii_lowercase(), id); id
    }
    pub fn resolve(&self, name: &str) -> Option<EntityId> { self.names.get(&name.to_ascii_lowercase()).copied() }
    pub fn entity(&self, id: EntityId) -> Option<&Entity> { self.entities.get(id.0 as usize) }
    pub fn observe_language(&mut self, feature: u64, intent: IntentId, reward: i16, confidence: u8) {
        let row = self.language_counts.entry(feature).or_default();
        if reward > 0 {
            if let Some((_, count)) = row.iter_mut().find(|(id, _)| *id == intent) { *count += 1; } else { row.push((intent, 1)); }
        }
        self.experiences.push(Experience { input_hash: feature, intent, reward, confidence });
        if self.experiences.len() > 4096 { self.experiences.drain(..1024); }
    }
    pub fn language_candidates(&self, feature: u64) -> &[(IntentId, u32)] { self.language_counts.get(&feature).map(Vec::as_slice).unwrap_or(&[]) }
    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() { fs::create_dir_all(parent)?; }
        let tmp = self.path.with_extension("data.tmp"); let mut f = File::create(&tmp)?; f.write_all(MAGIC)?;
        write_u32(&mut f, self.entities.len() as u32)?;
        for e in &self.entities { write_u16(&mut f, e.kind)?; write_u64(&mut f, e.flags)?; write_u32(&mut f, e.version)?; write_string(&mut f, &e.name)?; write_string(&mut f, &e.value)?; }
        write_u32(&mut f, self.language_counts.len() as u32)?;
        let mut rows: Vec<_> = self.language_counts.iter().collect(); rows.sort_by_key(|(k, _)| **k);
        for (feature, counts) in rows { write_u64(&mut f, *feature)?; write_u32(&mut f, counts.len() as u32)?; for (intent, count) in counts { write_u32(&mut f, intent.0)?; write_u32(&mut f, *count)?; } }
        write_u32(&mut f, self.experiences.len() as u32)?;
        for x in &self.experiences { write_u64(&mut f, x.input_hash)?; write_u32(&mut f, x.intent.0)?; write_i16(&mut f, x.reward)?; f.write_all(&[x.confidence])?; }
        f.sync_all()?; fs::rename(tmp, &self.path)?; Ok(())
    }
    fn decode(path: PathBuf, bytes: &[u8]) -> io::Result<Self> {
        let mut r = Reader { bytes, pos: 0 }; if r.take(8)? != MAGIC { return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid damon.data header")); }
        let mut data = Self::empty(path); let n = r.u32()? as usize;
        for _ in 0..n { let kind=r.u16()?; let flags=r.u64()?; let version=r.u32()?; let name=r.string()?; let value=r.string()?; let id=EntityId(data.entities.len() as u32); data.names.insert(name.to_ascii_lowercase(), id); data.entities.push(Entity { id, kind, flags, name, value, version }); }
        let rows=r.u32()? as usize; for _ in 0..rows { let feature=r.u64()?; let len=r.u32()? as usize; let mut counts=Vec::with_capacity(len); for _ in 0..len { counts.push((IntentId(r.u32()?), r.u32()?)); } data.language_counts.insert(feature, counts); }
        let exp=r.u32()? as usize; for _ in 0..exp { data.experiences.push(Experience { input_hash:r.u64()?, intent:IntentId(r.u32()?), reward:r.i16()?, confidence:r.u8()? }); }
        Ok(data)
    }
}
fn write_u16(w:&mut impl Write,v:u16)->io::Result<()> { w.write_all(&v.to_le_bytes()) }
fn write_i16(w:&mut impl Write,v:i16)->io::Result<()> { w.write_all(&v.to_le_bytes()) }
fn write_u32(w:&mut impl Write,v:u32)->io::Result<()> { w.write_all(&v.to_le_bytes()) }
fn write_u64(w:&mut impl Write,v:u64)->io::Result<()> { w.write_all(&v.to_le_bytes()) }
fn write_string(w:&mut impl Write,s:&str)->io::Result<()> { write_u32(w,s.len() as u32)?; w.write_all(s.as_bytes()) }
struct Reader<'a>{bytes:&'a[u8],pos:usize}
impl<'a> Reader<'a>{
    fn take(&mut self,n:usize)->io::Result<&'a[u8]>{if self.pos+n>self.bytes.len(){return Err(io::Error::new(io::ErrorKind::UnexpectedEof,"truncated damon.data"));}let s=&self.bytes[self.pos..self.pos+n];self.pos+=n;Ok(s)}
    fn u8(&mut self)->io::Result<u8>{Ok(self.take(1)?[0])}
    fn u16(&mut self)->io::Result<u16>{Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))}
    fn i16(&mut self)->io::Result<i16>{Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))}
    fn u32(&mut self)->io::Result<u32>{Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))}
    fn u64(&mut self)->io::Result<u64>{Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))}
    fn string(&mut self)->io::Result<String>{let n=self.u32()? as usize;String::from_utf8(self.take(n)?.to_vec()).map_err(|_|io::Error::new(io::ErrorKind::InvalidData,"invalid utf-8"))}
}
#[cfg(test)] mod tests { use super::*; #[test] fn round_trip_data(){ let p=std::env::temp_dir().join(format!("damon-test-{}.data",std::process::id()));let _=fs::remove_file(&p);let mut d=DamonData::open(&p).unwrap();let id=d.add_entity(1,"cpython","/tmp/cpython");d.observe_language(123,IntentId(7),1,240);d.save().unwrap();let d2=DamonData::open(&p).unwrap();assert_eq!(d2.resolve("cpython"),Some(id));assert_eq!(d2.language_candidates(123),&[(IntentId(7),1)]);let _=fs::remove_file(p);} }
