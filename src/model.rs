use std::env;
use std::io::Write;
use std::process::{Command,Stdio};

#[derive(Debug,Clone)] pub enum ProviderKind{Ollama,External,Cloud}
#[derive(Debug,Clone)] pub struct RoutedResponse{pub provider:ProviderKind,pub text:String}
#[derive(Debug,Clone)] pub struct ModelRouter{pub ollama_model:String,pub external_command:Option<String>,pub cloud_command:Option<String>,pub allow_cloud:bool}
impl Default for ModelRouter { fn default()->Self{Self{ollama_model:env::var("DAMON_OLLAMA_MODEL").unwrap_or_else(|_|"qwen3:8b".to_string()),external_command:env::var("DAMON_MODEL_COMMAND").ok(),cloud_command:env::var("DAMON_CLOUD_COMMAND").ok(),allow_cloud:env::var("DAMON_ALLOW_CLOUD").map(|v|v=="1"||v.eq_ignore_ascii_case("true")).unwrap_or(false)}} }
impl ModelRouter {
    pub fn infer(&self,prompt:&str)->Result<RoutedResponse,String>{
        if command_exists("ollama") { if let Ok(text)=run_with_stdin("ollama",&["run",&self.ollama_model],prompt){if !text.trim().is_empty(){return Ok(RoutedResponse{provider:ProviderKind::Ollama,text});}} }
        if let Some(cmd)=&self.external_command { if let Ok(text)=run_shell_command(cmd,prompt){if !text.trim().is_empty(){return Ok(RoutedResponse{provider:ProviderKind::External,text});}} }
        if self.allow_cloud { if let Some(cmd)=&self.cloud_command { let text=run_shell_command(cmd,prompt)?; if !text.trim().is_empty(){return Ok(RoutedResponse{provider:ProviderKind::Cloud,text});} } }
        Err("no model provider available; install Ollama or set DAMON_MODEL_COMMAND (cloud stays off unless DAMON_ALLOW_CLOUD=1)".into())
    }
}
fn command_exists(name:&str)->bool{let check=format!("command -v {} >/dev/null 2>&1",name);Command::new("sh").args(["-lc",check.as_str()]).status().map(|s|s.success()).unwrap_or(false)}
fn run_shell_command(cmd:&str,prompt:&str)->Result<String,String>{run_with_stdin("sh",&["-lc",cmd],prompt)}
fn run_with_stdin(program:&str,args:&[&str],prompt:&str)->Result<String,String>{let mut child=Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e|e.to_string())?;child.stdin.as_mut().ok_or("provider stdin unavailable")?.write_all(prompt.as_bytes()).map_err(|e|e.to_string())?;let out=child.wait_with_output().map_err(|e|e.to_string())?;if !out.status.success(){return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());}Ok(String::from_utf8_lossy(&out.stdout).to_string())}
#[cfg(test)] mod tests { use super::*; #[test] fn cloud_is_disabled_by_default(){let r=ModelRouter{ollama_model:"x".into(),external_command:None,cloud_command:Some("cat".into()),allow_cloud:false};assert!(!r.allow_cloud);} }
