use crate::data::DamonData;
use crate::language::{INTENT_GIT_DIFF, INTENT_GIT_STATUS, INTENT_LIST_FILES, INTENT_RUN_TESTS};
use crate::types::{Action, Effects, MeaningGraph, ToolId, ToolResult};
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

pub const TOOL_GIT_STATUS: ToolId = ToolId(1);
pub const TOOL_GIT_DIFF: ToolId = ToolId(2);
pub const TOOL_TEST: ToolId = ToolId(3);
pub const TOOL_LIST_FILES: ToolId = ToolId(4);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

impl CommandSpec {
    fn encoded(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn decode_test(value: &str) -> Option<Self> {
        match value.split('\n').collect::<Vec<_>>().as_slice() {
            ["cargo", "test", "--quiet"] => Some(Self::new("cargo", &["test", "--quiet"])),
            ["python3", "-m", "pytest", "-q"] => {
                Some(Self::new("python3", &["-m", "pytest", "-q"]))
            }
            ["npm", "test", "--", "--runInBand"] => {
                Some(Self::new("npm", &["test", "--", "--runInBand"]))
            }
            _ => None,
        }
    }

    fn decode_allowed(value: &str) -> Option<Self> {
        Self::decode_test(value).or_else(|| {
            match value.split('\n').collect::<Vec<_>>().as_slice() {
                ["cargo", "fmt", "--check"] => Some(Self::new("cargo", &["fmt", "--check"])),
                ["cargo", "test"] => Some(Self::new("cargo", &["test"])),
                ["cargo", "clippy", "--all-targets", "--", "-D", "warnings"] => Some(Self::new(
                    "cargo",
                    &["clippy", "--all-targets", "--", "-D", "warnings"],
                )),
                ["python3", "-m", "ruff", "check", "."] => {
                    Some(Self::new("python3", &["-m", "ruff", "check", "."]))
                }
                ["python3", "-m", "mypy", "."] => Some(Self::new("python3", &["-m", "mypy", "."])),
                ["npm", "run", script] if matches!(*script, "lint" | "typecheck" | "build") => {
                    Some(Self::new("npm", &["run", script]))
                }
                _ => None,
            }
        })
    }

    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|arg| (*arg).into()).collect(),
        }
    }
}
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
        TOOL_TEST => run_tests(action),
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

fn run_tests(action: &Action) -> ToolResult {
    let cwd = action.args.first().map(String::as_str).unwrap_or(".");
    let command = if action.args.len() > 1 {
        CommandSpec::decode_test(&action.args[1..].join("\n"))
    } else {
        discover_test_command(Path::new(cwd))
    };
    let Some(command) = command else {
        return ToolResult {
            success: false,
            stdout: String::new(),
            stderr: "no supported test runner detected".into(),
            code: None,
        };
    };
    let args = command.args.iter().map(String::as_str).collect::<Vec<_>>();
    run(cwd, &command.program, &args)
}

/// Resolve deterministic project commands before policy/execution. Cached data
/// is parsed back through a fixed allowlist, so a brain entry cannot introduce
/// an executable or argument.
pub fn prepare(action: &mut Action, data: &mut DamonData) -> io::Result<()> {
    if action.tool != TOOL_TEST {
        return Ok(());
    }
    let target = action
        .target
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "test target missing"))?;
    let path = Path::new(
        action
            .args
            .first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "project path missing"))?,
    );
    let state_hash = discovery_state_hash(path)?;
    let key = crate::cache::key_for_project(target);
    let cached = data
        .memo
        .get(crate::cache::TEST_COMMAND, key, state_hash, &data.entities)
        .and_then(CommandSpec::decode_test);
    let command = match cached {
        Some(command) => command,
        None => {
            let Some(command) = discover_test_command(path) else {
                return Ok(());
            };
            data.memo.put(
                crate::cache::TEST_COMMAND,
                key,
                state_hash,
                &[target],
                command.encoded(),
                &data.entities,
            )?;
            command
        }
    };
    action.args.truncate(1);
    action.args.push(command.program);
    action.args.extend(command.args);
    Ok(())
}

pub fn discover_test_command(path: &Path) -> Option<CommandSpec> {
    if path.join("Cargo.toml").is_file() {
        Some(CommandSpec::new("cargo", &["test", "--quiet"]))
    } else if path.join("pyproject.toml").is_file() || path.join("pytest.ini").is_file() {
        Some(CommandSpec::new("python3", &["-m", "pytest", "-q"]))
    } else if path.join("package.json").is_file() {
        Some(CommandSpec::new("npm", &["test", "--", "--runInBand"]))
    } else {
        None
    }
}

pub fn verification_plan(path: &Path) -> io::Result<Vec<CommandSpec>> {
    let mut plan = Vec::new();
    if path.join("Cargo.toml").is_file() {
        plan.push(CommandSpec::new("cargo", &["fmt", "--check"]));
        plan.push(CommandSpec::new("cargo", &["test"]));
        plan.push(CommandSpec::new(
            "cargo",
            &["clippy", "--all-targets", "--", "-D", "warnings"],
        ));
    } else if path.join("pyproject.toml").is_file() || path.join("pytest.ini").is_file() {
        plan.push(CommandSpec::new("python3", &["-m", "pytest", "-q"]));
        let config = read_bounded_if_present(&path.join("pyproject.toml"))?;
        if config.windows(11).any(|window| window == b"[tool.ruff]") {
            plan.push(CommandSpec::new("python3", &["-m", "ruff", "check", "."]));
        }
        if config.windows(11).any(|window| window == b"[tool.mypy]") {
            plan.push(CommandSpec::new("python3", &["-m", "mypy", "."]));
        }
    } else if path.join("package.json").is_file() {
        let package_bytes = read_bounded_if_present(&path.join("package.json"))?;
        let package = String::from_utf8_lossy(&package_bytes);
        plan.push(CommandSpec::new("npm", &["test", "--", "--runInBand"]));
        for script in ["lint", "typecheck", "build"] {
            if package.contains(&format!("\"{script}\"")) {
                plan.push(CommandSpec::new("npm", &["run", script]));
            }
        }
    }
    Ok(plan)
}

pub fn cached_verification_plan(
    data: &mut DamonData,
    project: crate::types::EntityId,
) -> io::Result<Vec<CommandSpec>> {
    let entity = data
        .entity(project)
        .filter(|entity| entity.kind == crate::world::PROJECT)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown project"))?;
    let path = entity.value.clone();
    let path = Path::new(&path);
    let state_hash = discovery_state_hash(path)?;
    let key = crate::cache::key_for_project(project);
    if let Some(value) = data.memo.get(
        crate::cache::VERIFICATION_PLAN,
        key,
        state_hash,
        &data.entities,
    ) {
        return decode_plan(value).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid cached verification plan",
            )
        });
    }
    let plan = verification_plan(path)?;
    let value = plan
        .iter()
        .map(CommandSpec::encoded)
        .collect::<Vec<_>>()
        .join("\u{1e}");
    data.memo.put(
        crate::cache::VERIFICATION_PLAN,
        key,
        state_hash,
        &[project],
        value,
        &data.entities,
    )?;
    Ok(plan)
}

fn decode_plan(value: &str) -> Option<Vec<CommandSpec>> {
    if value.is_empty() {
        return Some(Vec::new());
    }
    value
        .split('\u{1e}')
        .map(CommandSpec::decode_allowed)
        .collect()
}

pub fn discovery_state_hash(path: &Path) -> io::Result<u64> {
    let names = [
        "Cargo.toml",
        "Cargo.lock",
        "pyproject.toml",
        "pytest.ini",
        "package.json",
        "package-lock.json",
    ];
    let mut parts = Vec::new();
    for name in names {
        let file = path.join(name);
        if file.is_file() {
            parts.push(name.as_bytes().to_vec());
            parts.push(read_bounded_if_present(&file)?);
        }
    }
    let refs = parts.iter().map(Vec::as_slice).collect::<Vec<_>>();
    Ok(crate::cache::hash_bytes(&refs))
}

fn read_bounded_if_present(path: &Path) -> io::Result<Vec<u8>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "discovery manifest exceeds 1 MiB",
        ));
    }
    Ok(bytes)
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
