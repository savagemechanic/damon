use std::env;
use std::io;
use std::path::PathBuf;
use crate::data::DamonData;
use crate::language::{self,Interpretation};
use crate::model::{ModelRouter,ProviderKind};
use crate::policy::Policy;
use crate::tools;
use crate::types::{MeaningGraph,ToolResult};

pub struct Damon{pub data:DamonData,pub models:ModelRouter,pub policy:Policy}
impl Damon{
    pub fn open_default()->io::Result<Self>{let path=env::var_os("DAMON_DATA").map(PathBuf::from).unwrap_or_else(default_data_path);Ok(Self{data:DamonData::open(path)?,models:ModelRouter::default(),policy:Policy::default()})}
    pub fn handle(&mut self,input:&str)->String{
        let feature=language::feature_hash(input);
        let meaning=match language::understand(input,&self.data){Interpretation::Resolved(m)=>m,Interpretation::Unknown{..}=>match self.ask_teacher(input){Ok(m)=>m,Err(e)=>return format!("I don't know how to do that yet. {e}")}};
        let action=match tools::action_from_meaning(&meaning,&self.data){Ok(a)=>a,Err(e)=>return e}; if let Err(e)=self.policy.check(&action){return format!("I understood the request, but policy blocked it: {e}");}
        let result=tools::execute(&action);let reward=if result.success{1}else{-1};self.data.observe_language(feature,meaning.intent,reward,meaning.confidence);let _=self.data.save();render_result(result)
    }
    fn ask_teacher(&self,input:&str)->Result<MeaningGraph,String>{
        let prompt=format!("You are Damon's language teacher. Map the user's English request to exactly one token and output only that token. Allowed tokens: GIT_STATUS, GIT_DIFF, RUN_TESTS, LIST_FILES. If none fit, output UNKNOWN. User: {input}");
        let response=self.models.infer(&prompt)?;let token=response.text.trim().lines().next().unwrap_or("").trim();let intent=match token{"GIT_STATUS"=>language::INTENT_GIT_STATUS,"GIT_DIFF"=>language::INTENT_GIT_DIFF,"RUN_TESTS"=>language::INTENT_RUN_TESTS,"LIST_FILES"=>language::INTENT_LIST_FILES,_=>return Err(format!("teacher returned {token:?}"))};let confidence=match response.provider{ProviderKind::Ollama=>190,ProviderKind::External=>185,ProviderKind::Cloud=>180};Ok(MeaningGraph{intent,target:self.data.resolve("damon"),edges:Vec::new(),confidence})
    }
}
fn render_result(result:ToolResult)->String{let out=result.stdout.trim();let err=result.stderr.trim();if result.success{if out.is_empty(){"Done. The operation completed successfully.".into()}else{out.to_string()}}else if !err.is_empty(){format!("The operation failed{}: {}",result.code.map(|c|format!(" with exit code {c}")).unwrap_or_default(),err)}else{"The operation failed.".into()}}
fn default_data_path()->PathBuf{if let Some(home)=env::var_os("HOME"){return PathBuf::from(home).join(".damon").join("damon.data");}PathBuf::from("damon.data")}
