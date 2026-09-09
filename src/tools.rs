use crate::data::DamonData;
use crate::language::{INTENT_GIT_DIFF, INTENT_GIT_STATUS, INTENT_LIST_FILES, INTENT_RUN_TESTS};
use crate::types::{Action, Effects, MeaningGraph, ToolId, ToolResult};
use std::path::Path;
use std::time::Duration;

pub const TOOL_GIT_STATUS: ToolId = ToolId(1);
pub const TOOL_GIT_DIFF: ToolId = ToolId(2);
pub const TOOL_TEST: ToolId = ToolId(3);
pub const TOOL_LIST_FILES: ToolId = ToolId(4);
pub fn action_from_meaning(m: &MeaningGraph, data: &DamonData) -> Result<Action, String> {
    crate::reason::resolve(m, data)
}
pub fn action_for(
    intent: crate::types::IntentId,
    target: crate::types::EntityId,
    data: &DamonData,
) -> Result<Action, String> {
    let entity = data
        .entity(target)
        .filter(|e| e.kind == 1)
        .ok_or("unknown project")?;
    let tool = match intent {
        INTENT_GIT_STATUS => TOOL_GIT_STATUS,
        INTENT_GIT_DIFF => TOOL_GIT_DIFF,
        INTENT_RUN_TESTS => TOOL_TEST,
        INTENT_LIST_FILES => TOOL_LIST_FILES,
        crate::language::INTENT_CHANGED_FILES => ToolId(5),
        _ => return Err("meaning has no deterministic tool".into()),
    };
    Ok(Action {
        tool,
        target: Some(target),
        effects: required_effects(tool)?,
        args: vec![entity.value.clone()],
    })
}
pub fn required_effects(tool: ToolId) -> Result<Effects, String> {
    match tool.0 {
        1..=3 | 5 => Ok(Effects::READ.union(Effects::PROCESS)),
        4 => Ok(Effects::READ),
        _ => Err("unknown tool".into()),
    }
}
pub fn execute(action: &Action, policy: &crate::policy::Policy) -> ToolResult {
    if let Err(e) = policy.check(action) {
        return ToolResult {
            success: false,
            stdout: String::new(),
            stderr: format!("policy blocked operation: {e}"),
            code: None,
        };
    }
    let cwd = action.args.first().map(String::as_str).unwrap_or(".");
    match action.tool {
        TOOL_GIT_STATUS => run(cwd, "git", &["status", "--short", "--branch"]),
        TOOL_GIT_DIFF => run(
            cwd,
            "git",
            &["--no-pager", "diff", "--no-ext-diff", "--", "."],
        ),
        TOOL_TEST => run_tests(cwd),
        TOOL_LIST_FILES => list_files(cwd),
        ToolId(5) => changed_files(cwd),
        _ => ToolResult {
            success: false,
            stdout: String::new(),
            stderr: "unknown tool".into(),
            code: None,
        },
    }
}
fn run(cwd: &str, program: &str, args: &[&str]) -> ToolResult {
    crate::process::run(
        Path::new(cwd),
        program,
        args,
        None,
        Duration::from_secs(120),
    )
}

fn run_tests(cwd: &str) -> ToolResult {
    let p = Path::new(cwd);
    if p.join("Cargo.toml").exists() {
        return run(cwd, "cargo", &["test", "--quiet"]);
    }
    if p.join("pyproject.toml").exists() || p.join("pytest.ini").exists() {
        return run(cwd, "python3", &["-m", "pytest", "-q"]);
    }
    if p.join("package.json").exists() {
        return run(cwd, "npm", &["test", "--", "--runInBand"]);
    }
    ToolResult {
        success: false,
        stdout: String::new(),
        stderr: "no supported test runner detected".into(),
        code: None,
    }
}
fn list_files(cwd: &str) -> ToolResult {
    let mut names = Vec::new();
    match std::fs::read_dir(cwd) {
        Ok(rd) => {
            for entry in rd.take(200) {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        return ToolResult {
                            success: false,
                            stdout: String::new(),
                            stderr: e.to_string(),
                            code: None,
                        }
                    }
                };
                names.push(entry.file_name().to_string_lossy().to_string());
            }
            names.sort();
            ToolResult {
                success: true,
                stdout: names.join("\n"),
                stderr: String::new(),
                code: Some(0),
            }
        }
        Err(e) => ToolResult {
            success: false,
            stdout: String::new(),
            stderr: e.to_string(),
            code: None,
        },
    }
}

fn changed_files(cwd: &str) -> ToolResult {
    let email = run(cwd, "git", &["config", "user.email"]);
    if !email.success || email.stdout.trim().is_empty() {
        return ToolResult {
            success: false,
            stdout: String::new(),
            stderr: "configure your Git user.email to resolve 'I changed'".into(),
            code: None,
        };
    }
    let author = email
        .stdout
        .trim()
        .chars()
        .flat_map(|c| {
            if ".[]\\*^$".contains(c) {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect::<String>();
    let mut result = run(
        cwd,
        "git",
        &[
            "--no-pager",
            "log",
            "--since=yesterday 00:00",
            "--until=today 00:00",
            "--format=",
            "--name-only",
            &format!("--author={author}"),
            "--",
        ],
    );
    if result.success {
        let mut names = result
            .stdout
            .lines()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        result.stdout=format!("Files in yesterday's commits by {}:\n{}\nGit does not record when uncommitted edits were made.",email.stdout.trim(),if names.is_empty(){"None.".into()} else {names.join("\n")});
    }
    result
}
