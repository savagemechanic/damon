use crate::data::DamonData;
use crate::types::{Action, Effects, MeaningGraph, ToolId, ToolResult};
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

pub const TOOL_GIT_STATUS: ToolId = ToolId(1);
pub const TOOL_GIT_DIFF: ToolId = ToolId(2);
pub const TOOL_TEST: ToolId = ToolId(3);
pub const TOOL_LIST_FILES: ToolId = ToolId(4);
pub const TOOL_NETWORK_INTERFACES: ToolId = ToolId(6);
pub const TOOL_NETWORK_ROUTES: ToolId = ToolId(7);
pub const TOOL_NETWORK_NEIGHBORS: ToolId = ToolId(8);
pub const TOOL_NETWORK_DIAGNOSE: ToolId = ToolId(9);
pub const TOOL_LIST_SOCKETS: ToolId = ToolId(10);
pub const TOOL_COPY_FILE: ToolId = ToolId(11);
pub const TOOL_FIND_FILES: ToolId = ToolId(12);

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
    let capability =
        crate::capability::for_intent(intent).ok_or("meaning has no known native capability")?;
    action_for_capability(capability, target, data)
}

pub fn action_for_capability(
    capability: crate::types::CapabilityId,
    target: crate::types::EntityId,
    data: &DamonData,
) -> Result<Action, String> {
    let tool = data
        .capabilities
        .resolve_tool(capability)
        .ok_or("capability has no verified implementation")?;
    let entity = data.entity(target).ok_or("unknown target")?;
    let args = match tool {
        TOOL_GIT_STATUS | TOOL_GIT_DIFF | TOOL_TEST | TOOL_LIST_FILES | ToolId(5)
            if entity.kind == crate::world::PROJECT =>
        {
            vec![entity.value.clone()]
        }
        TOOL_NETWORK_INTERFACES
        | TOOL_NETWORK_ROUTES
        | TOOL_NETWORK_NEIGHBORS
        | TOOL_NETWORK_DIAGNOSE
        | TOOL_LIST_SOCKETS
            if entity.kind == crate::world::HOST =>
        {
            Vec::new()
        }
        _ => return Err("capability target kind is incompatible with implementation".into()),
    };
    Ok(Action {
        capability,
        tool,
        target: Some(target),
        effects: required_effects(tool)?,
        args,
    })
}
pub fn required_effects(tool: ToolId) -> Result<Effects, String> {
    match tool.0 {
        1..=3 | 5 => Ok(Effects::READ.union(Effects::PROCESS)),
        4 => Ok(Effects::READ),
        6 => Ok(Effects::READ),
        7 => Ok(Effects::READ.union(Effects::PROCESS)),
        8 => Ok(Effects::READ.union(Effects::PROCESS)),
        9 => Ok(Effects::READ.union(Effects::NETWORK)),
        10 => Ok(Effects::READ.union(Effects::PROCESS)),
        11 => Ok(Effects::READ.union(Effects::WRITE)),
        12 => Ok(Effects::READ),
        _ => Err("unknown tool".into()),
    }
}
pub fn capability_for_tool(tool: ToolId) -> Option<crate::types::CapabilityId> {
    Some(match tool {
        TOOL_GIT_STATUS => crate::capability::GIT_STATUS,
        TOOL_GIT_DIFF => crate::capability::GIT_DIFF,
        TOOL_TEST => crate::capability::RUN_TESTS,
        TOOL_LIST_FILES => crate::capability::LIST_FILES,
        ToolId(5) => crate::capability::FIND_CHANGED_FILES,
        TOOL_NETWORK_INTERFACES => crate::capability::INSPECT_INTERFACES,
        TOOL_NETWORK_ROUTES => crate::capability::INSPECT_ROUTES,
        TOOL_NETWORK_NEIGHBORS => crate::capability::INSPECT_NEIGHBORS,
        TOOL_NETWORK_DIAGNOSE => crate::capability::DIAGNOSE_NETWORK,
        TOOL_LIST_SOCKETS => crate::capability::LIST_SOCKETS,
        TOOL_COPY_FILE => crate::capability::COPY_FILE,
        TOOL_FIND_FILES => crate::capability::FIND_FILES,
        _ => return None,
    })
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
        TOOL_LIST_FILES => action.args.get(1).map_or_else(
            || list_files(cwd),
            |cached| ToolResult {
                success: true,
                stdout: cached.clone(),
                stderr: String::new(),
                code: Some(0),
            },
        ),
        ToolId(5) => changed_files(cwd),
        TOOL_NETWORK_INTERFACES => inspect_interfaces(),
        TOOL_NETWORK_ROUTES => inspect_routes(),
        TOOL_NETWORK_NEIGHBORS => inspect_neighbors(),
        TOOL_NETWORK_DIAGNOSE => diagnose_network(),
        TOOL_LIST_SOCKETS => inspect_sockets(),
        TOOL_COPY_FILE => copy_file(action),
        TOOL_FIND_FILES => find_files(action),
        _ => ToolResult {
            success: false,
            stdout: String::new(),
            stderr: "unknown tool".into(),
            code: None,
        },
    }
}

fn inspect_interfaces() -> ToolResult {
    match crate::network::interface::discover() {
        Ok(interfaces) => {
            let mut lines = crate::network::wifi::discover()
                .map(|links| render_wifi_links(&links))
                .unwrap_or_default();
            lines.extend(
                interfaces
                    .iter()
                    .map(|interface| {
                        let kind = format!("{:?}", interface.kind).to_ascii_lowercase();
                        let state = match (interface.state.up, interface.state.running) {
                            (true, true) => "up and running",
                            (true, false) => "up",
                            _ => "down",
                        };
                        let addresses = if interface.addresses.is_empty() {
                            "no addresses observed".to_string()
                        } else {
                            interface
                                .addresses
                                .iter()
                                .map(|address| format!("{}/{}", address.address, address.prefix))
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        let mac = interface.mac.map_or_else(
                            || "MAC not observed".to_string(),
                            |mac| format!("MAC {mac}"),
                        );
                        let mtu = interface
                            .mtu
                            .map_or_else(|| "MTU unknown".to_string(), |mtu| format!("MTU {mtu}"));
                        format!(
                            "{} (index {}, {kind}) is {state}; {addresses}; {mac}; {mtu}.",
                            interface.name, interface.index
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            ToolResult {
                success: true,
                stdout: if lines.is_empty() {
                    "No network interfaces were observed.".into()
                } else {
                    lines.join("\n")
                },
                stderr: String::new(),
                code: Some(0),
            }
        }
        Err(error) => tool_error("Network interface discovery failed", error),
    }
}

fn render_wifi_links(links: &[crate::network::wifi::WifiLink]) -> Vec<String> {
    use crate::network::wifi::{Band, ConnectionState};
    links
        .iter()
        .map(|link| {
            if link.state != ConnectionState::Connected {
                return format!("Wi-Fi interface {} is disconnected.", link.interface_name);
            }
            let network = link.ssid.map_or_else(
                || "a privacy-redacted network".to_string(),
                |ssid| format!("“{ssid}”"),
            );
            let band = match link.band {
                Some(Band::Ghz2) => Some("2.4 GHz"),
                Some(Band::Ghz5) => Some("5 GHz"),
                Some(Band::Ghz6) => Some("6 GHz"),
                Some(Band::Unknown) | None => None,
            };
            let mut details = Vec::new();
            if let Some(band) = band {
                details.push(band.to_string());
            }
            if let Some(channel) = link.channel {
                details.push(format!("channel {channel}"));
            }
            if let Some(signal) = link.signal_dbm {
                details.push(format!("signal {signal} dBm"));
            }
            if let Some(rate) = link.link_rate_mbps {
                details.push(format!("link rate {rate} Mbps"));
            }
            let suffix = if details.is_empty() {
                String::new()
            } else {
                format!(" ({})", details.join(", "))
            };
            format!(
                "Wi-Fi interface {} is connected to {network}{suffix}.",
                link.interface_name
            )
        })
        .collect()
}

fn inspect_routes() -> ToolResult {
    match crate::network::route::discover() {
        Ok(routes) => {
            let Some(route) = crate::network::route::default_gateway(&routes) else {
                return ToolResult {
                    success: true,
                    stdout: "No active default gateway was observed in the routing table.".into(),
                    stderr: String::new(),
                    code: Some(0),
                };
            };
            let name = crate::network::interface::discover()
                .ok()
                .and_then(|interfaces| {
                    interfaces
                        .into_iter()
                        .find(|interface| interface.id == route.interface)
                        .map(|interface| interface.name)
                })
                .unwrap_or_else(|| format!("index {}", route.interface.0));
            ToolResult {
                success: true,
                stdout: format!(
                    "The active default route sends traffic through gateway {} on interface {name}.",
                    route.gateway.expect("default gateway was filtered above")
                ),
                stderr: String::new(),
                code: Some(0),
            }
        }
        Err(error) => tool_error("Routing-table discovery failed", error),
    }
}

fn inspect_neighbors() -> ToolResult {
    match crate::network::neighbor::discover() {
        Ok(neighbors) => {
            let interfaces = crate::network::interface::discover().unwrap_or_default();
            let lines = neighbors
                .iter()
                .map(|neighbor| {
                    let interface = neighbor
                        .interface
                        .and_then(|id| {
                            interfaces
                                .iter()
                                .find(|interface| interface.id == id)
                                .map(|interface| interface.name.clone())
                        })
                        .unwrap_or_else(|| "an unknown interface".into());
                    let mac = neighbor.mac.map_or_else(
                        || "MAC not observed".to_string(),
                        |mac| format!("MAC {mac}"),
                    );
                    let state = format!("{:?}", neighbor.state).to_ascii_lowercase();
                    format!(
                        "{} was observed on {interface} ({mac}, state {state}).",
                        neighbor.address
                    )
                })
                .collect::<Vec<_>>();
            ToolResult {
                success: true,
                stdout: if lines.is_empty() {
                    "The operating system currently has no observed network neighbors.".into()
                } else {
                    lines.join("\n")
                },
                stderr: String::new(),
                code: Some(0),
            }
        }
        Err(error) => tool_error("Neighbor-table discovery failed", error),
    }
}

fn diagnose_network() -> ToolResult {
    let evidence = crate::network::diagnosis::internet();
    let success = evidence.last().is_some_and(|stage| stage.passed);
    let stdout = evidence
        .iter()
        .map(|stage| {
            let name = format!("{:?}", stage.stage).to_ascii_lowercase();
            let status = if stage.passed { "passed" } else { "failed" };
            format!("{name}: {status} — {}", stage.detail)
        })
        .collect::<Vec<_>>()
        .join("\n");
    ToolResult {
        success,
        stdout: if success {
            stdout.clone()
        } else {
            String::new()
        },
        stderr: if success { String::new() } else { stdout },
        code: success.then_some(0),
    }
}

fn inspect_sockets() -> ToolResult {
    match crate::network::inventory::discover() {
        Ok(sockets) => {
            let lines = sockets
                .iter()
                .take(500)
                .map(|socket| {
                    let transport = format!("{:?}", socket.transport).to_ascii_lowercase();
                    let owner = match (&socket.process_name, socket.process_id) {
                        (Some(name), Some(pid)) => format!("{name} (PID {pid})"),
                        (_, Some(pid)) => format!("PID {pid}"),
                        _ => "owner not visible".into(),
                    };
                    let remote = socket.remote.as_ref().map_or_else(
                        || "not connected".into(),
                        crate::network::inventory::render_endpoint,
                    );
                    format!(
                        "{owner}: {transport} {} → {remote} ({})",
                        crate::network::inventory::render_endpoint(&socket.local),
                        socket.state.to_ascii_lowercase()
                    )
                })
                .collect::<Vec<_>>();
            ToolResult {
                success: true,
                stdout: if lines.is_empty() {
                    "No visible TCP or UDP sockets were observed.".into()
                } else {
                    lines.join("\n")
                },
                stderr: String::new(),
                code: Some(0),
            }
        }
        Err(error) => tool_error("Socket inventory failed", error),
    }
}

fn copy_file(action: &Action) -> ToolResult {
    let [root, mention, destination] = action.args.as_slice() else {
        return tool_error(
            "File copy failed",
            "copy requires source root, file, and destination",
        );
    };
    match copy_file_inner(Path::new(root), mention, Path::new(destination)) {
        Ok(path) => ToolResult {
            success: true,
            stdout: format!("Copied and verified {}.", path.display()),
            stderr: String::new(),
            code: Some(0),
        },
        Err(error) => tool_error("File copy failed", error),
    }
}

fn copy_file_inner(
    root: &Path,
    mention: &str,
    destination: &Path,
) -> io::Result<std::path::PathBuf> {
    let relative = Path::new(mention);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the file mention must be a relative path without traversal",
        ));
    }
    let root = fs::canonicalize(root)?;
    let source = fs::canonicalize(root.join(relative))?;
    if !source.starts_with(&root) || !source.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the source is not a file inside the named project",
        ));
    }
    let destination = fs::canonicalize(destination)?;
    if !destination.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the destination is not a directory",
        ));
    }
    let name = source
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "source has no file name"))?;
    let output = destination.join(name);
    let mut input = fs::File::open(&source)?;
    let mut created = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    let copied = io::copy(&mut input, &mut created);
    if let Err(error) = copied {
        drop(created);
        let _ = fs::remove_file(&output);
        return Err(error);
    }
    created.sync_all()?;
    if fs::read(&source)? != fs::read(&output)? {
        let _ = fs::remove_file(&output);
        return Err(io::Error::other("copied bytes did not verify"));
    }
    Ok(output)
}

fn find_files(action: &Action) -> ToolResult {
    let [root, minimum] = action.args.as_slice() else {
        return tool_error(
            "File search failed",
            "search requires a root and minimum byte size",
        );
    };
    let minimum = match minimum.parse::<u64>() {
        Ok(minimum) => minimum,
        Err(error) => return tool_error("File search failed", error),
    };
    match find_files_larger_than(Path::new(root), minimum) {
        Ok(files) => ToolResult {
            success: true,
            stdout: if files.is_empty() {
                format!("No files larger than {minimum} bytes were found.")
            } else {
                files.join("\n")
            },
            stderr: String::new(),
            code: Some(0),
        },
        Err(error) => tool_error("File search failed", error),
    }
}

fn find_files_larger_than(root: &Path, minimum: u64) -> io::Result<Vec<String>> {
    let root = fs::canonicalize(root)?;
    let mut pending = vec![(root.clone(), 0_u8)];
    let mut visited = 0_usize;
    let mut matches = Vec::new();
    while let Some((directory, depth)) = pending.pop() {
        if depth >= 64 {
            continue;
        }
        for entry in fs::read_dir(directory)? {
            visited += 1;
            if visited > 20_000 {
                return Err(io::Error::other("file search exceeded 20,000 entries"));
            }
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else if metadata.is_file() && metadata.len() > minimum {
                matches.push(
                    entry
                        .path()
                        .strip_prefix(&root)
                        .unwrap_or(&entry.path())
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    matches.sort();
    Ok(matches)
}

fn tool_error(context: &str, error: impl std::fmt::Display) -> ToolResult {
    ToolResult {
        success: false,
        stdout: String::new(),
        stderr: format!("{context}: {error}"),
        code: None,
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
    if action.tool == TOOL_LIST_FILES {
        return prepare_file_inventory(action, data);
    }
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

fn prepare_file_inventory(action: &mut Action, data: &mut DamonData) -> io::Result<()> {
    let target = action
        .target
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file target missing"))?;
    let root = action
        .args
        .first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "project path missing"))?
        .clone();
    let Ok((state_hash, names)) = file_inventory(Path::new(&root)) else {
        // Caching is an optimization. Let the real tool produce the authoritative
        // filesystem error instead of turning discovery into verification.
        return Ok(());
    };
    let key = crate::cache::key_for_project(target);
    let cached = data
        .memo
        .get(crate::cache::FILE_LIST, key, state_hash, &data.entities)
        .map(str::to_string);
    let value = match cached {
        Some(value) => value,
        None => {
            let value = names.join("\n");
            data.memo.put(
                crate::cache::FILE_LIST,
                key,
                state_hash,
                &[target],
                value.clone(),
                &data.entities,
            )?;
            for name in names.iter().take(200) {
                let path = Path::new(&root).join(name);
                let kind = if path.is_dir() {
                    crate::world::DIRECTORY
                } else {
                    crate::world::FILE
                };
                let entity = data.add_entity(
                    kind,
                    &format!(
                        "{}:{name}",
                        data.entity(target).map_or("project", |e| &e.name)
                    ),
                    path.to_str().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "file path is not UTF-8")
                    })?,
                );
                data.link(target, crate::world::Relationship::Contains, entity)?;
            }
            value
        }
    };
    action.args.truncate(1);
    action.args.push(value);
    Ok(())
}

fn file_inventory(path: &Path) -> io::Result<(u64, Vec<String>)> {
    let mut rows = Vec::new();
    for entry in fs::read_dir(path)?.take(4097) {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |duration| {
                duration.as_nanos().min(u128::from(u64::MAX)) as u64
            });
        rows.push((name, metadata.len(), modified, u8::from(metadata.is_dir())));
    }
    if rows.len() > 4096 {
        return Err(io::Error::other("file inventory exceeds 4,096 entries"));
    }
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    let encoded = rows
        .iter()
        .map(|(name, size, modified, directory)| format!("{name}\0{size}\0{modified}\0{directory}"))
        .collect::<Vec<_>>();
    let references = encoded.iter().map(String::as_bytes).collect::<Vec<_>>();
    let hash = crate::cache::hash_bytes(&references);
    Ok((hash, rows.into_iter().map(|row| row.0).take(200).collect()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wifi_state_renders_as_direct_evidence() {
        use crate::network::wifi::{Band, ConnectionState, SecurityMode, Ssid, WifiLink};
        let lines = render_wifi_links(&[WifiLink {
            interface: Some(crate::network::InterfaceId(1)),
            interface_name: "en0".into(),
            state: ConnectionState::Connected,
            ssid: Some(Ssid::from_utf8("Home").unwrap()),
            bssid: None,
            channel: Some(44),
            band: Some(Band::Ghz5),
            signal_dbm: Some(-48),
            noise_dbm: None,
            link_rate_mbps: Some(866),
            security: SecurityMode::Wpa3,
        }]);
        assert_eq!(
            lines,
            ["Wi-Fi interface en0 is connected to “Home” (5 GHz, channel 44, signal -48 dBm, link rate 866 Mbps)."]
        );
    }
}
