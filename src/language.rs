use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use crate::data::DamonData;
use crate::types::{IntentId, MeaningGraph};

pub const INTENT_GIT_STATUS: IntentId = IntentId(1);
pub const INTENT_GIT_DIFF: IntentId = IntentId(2);
pub const INTENT_RUN_TESTS: IntentId = IntentId(3);
pub const INTENT_LIST_FILES: IntentId = IntentId(4);

#[derive(Debug)] pub enum Interpretation { Resolved(MeaningGraph), Unknown { feature: u64 } }
pub fn normalize(input:&str)->String { input.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ") }
pub fn feature_hash(input:&str)->u64 { let mut h=DefaultHasher::new(); normalize(input).hash(&mut h); h.finish() }

pub fn understand(input:&str,data:&DamonData)->Interpretation {
    let n=normalize(input);
    let target=data.entities.iter().find(|e| n.split(|c:char|!c.is_alphanumeric()&&c!='-'&&c!='_').any(|w|w==e.name.to_ascii_lowercase())).map(|e|e.id);
    let mut scored:Vec<(IntentId,u32)>=Vec::new();
    let mut add=|id:IntentId,score:u32|{if let Some((_,s))=scored.iter_mut().find(|(x,_)|*x==id){*s+=score}else{scored.push((id,score));}};
    if n.contains("test")||n.contains("check the code"){add(INTENT_RUN_TESTS,80);}
    if n.contains("diff")||n.contains("what changed")||n.contains("what did i change")||n.contains("what have i changed"){add(INTENT_GIT_DIFF,90);}
    if n.contains("git status")||n=="status"||n.contains("repo status"){add(INTENT_GIT_STATUS,90);}
    if n.contains("list files")||n.contains("show files"){add(INTENT_LIST_FILES,85);}
    let feature=feature_hash(&n); for (intent,count) in data.language_candidates(feature){add(*intent,count.saturating_mul(20));}
    scored.sort_by_key(|(_,score)|std::cmp::Reverse(*score)); let Some((intent,best))=scored.first().copied() else{return Interpretation::Unknown{feature};};
    let second=scored.get(1).map(|x|x.1).unwrap_or(0); let margin=best.saturating_sub(second); let confidence=(best.saturating_add(margin).min(255)) as u8;
    if confidence<70{return Interpretation::Unknown{feature};} Interpretation::Resolved(MeaningGraph{intent,target,edges:Vec::new(),confidence})
}
#[cfg(test)] mod tests { use super::*; use crate::data::DamonData; #[test] fn resolves_common_intents(){let p=std::env::temp_dir().join(format!("damon-lang-{}.data",std::process::id()));let _=std::fs::remove_file(&p);let d=DamonData::open(&p).unwrap();let Interpretation::Resolved(m)=understand("show me what changed in damon",&d)else{panic!()};assert_eq!(m.intent,INTENT_GIT_DIFF);assert!(m.target.is_some());let _=std::fs::remove_file(p);} }
