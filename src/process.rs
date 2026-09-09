//! Bounded subprocess execution shared by deterministic tools and providers.
//! This is lifecycle management, not a sandbox for untrusted project code.
use crate::types::ToolResult;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const MAX_OUTPUT: u64 = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn create() -> io::Result<Self> {
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "damon-process-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other(
            "cannot allocate subprocess scratch directory",
        ))
    }
    fn file(&self, name: &str) -> io::Result<File> {
        OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(self.0.join(name))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn output(file: &File) -> io::Result<String> {
    use std::io::{Seek, SeekFrom};
    let mut file = file.try_clone()?;
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(MAX_OUTPUT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_OUTPUT {
        return Err(io::Error::other("process output exceeded 1 MiB limit"));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
#[cfg(unix)]
fn kill_group(id: u32) -> io::Result<()> {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    // SAFETY: POSIX kill takes scalar values only. Every child is launched into
    // its own process group; a negative ID targets only that group. SIGKILL=9.
    let result = unsafe { kill(-(id as i32), 9) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(3) {
        Ok(())
    } else {
        Err(error)
    }
}
pub fn run(
    cwd: &Path,
    program: &str,
    args: &[&str],
    input: Option<&str>,
    timeout: Duration,
) -> ToolResult {
    match run_inner(cwd, program, args, input, timeout) {
        Ok(result) => result,
        Err(e) => ToolResult {
            success: false,
            stdout: String::new(),
            stderr: e.to_string(),
            code: None,
        },
    }
}
fn run_inner(
    cwd: &Path,
    program: &str,
    args: &[&str],
    input: Option<&str>,
    timeout: Duration,
) -> io::Result<ToolResult> {
    if input.is_some_and(|s| s.len() > 64 * 1024) {
        return Err(io::Error::other("provider prompt exceeds 64 KiB"));
    }
    let scratch = Scratch::create()?;
    let stdout = scratch.file("stdout")?;
    let stderr = scratch.file("stderr")?;
    let stdin = if let Some(input) = input {
        use std::io::{Seek, SeekFrom};
        let mut f = scratch.file("stdin")?;
        f.write_all(input.as_bytes())?;
        f.seek(SeekFrom::Start(0))?;
        Stdio::from(f)
    } else {
        Stdio::null()
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(stdin)
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let started = Instant::now();
    let status = (|| loop {
        if stdout.metadata()?.len() > MAX_OUTPUT || stderr.metadata()?.len() > MAX_OUTPUT {
            return Err(io::Error::other("process output exceeded 1 MiB limit"));
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "process timed out"));
        }
        std::thread::sleep(Duration::from_millis(10));
    })();
    #[cfg(unix)]
    let cleanup = kill_group(child.id());
    #[cfg(not(unix))]
    let cleanup = if status.is_err() {
        child.kill()
    } else {
        Ok(())
    };
    // Reap even on timeout or output/metadata failure.
    let reaped = child.wait();
    cleanup?;
    reaped?;
    let status = status?;
    Ok(ToolResult {
        success: status.success(),
        stdout: output(&stdout)?,
        stderr: output(&stderr)?,
        code: status.code(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stdin_closes_and_exit_status_is_evidence() {
        let result = run(
            Path::new("."),
            "sh",
            &["-c", "cat; echo failure >&2; exit 7"],
            Some("hello"),
            Duration::from_secs(2),
        );
        assert!(!result.success);
        assert_eq!(result.code, Some(7));
        assert_eq!(result.stdout, "hello");
        assert_eq!(result.stderr, "failure\n");
    }
    #[test]
    fn timeout_is_bounded_even_with_descendants() {
        let start = Instant::now();
        let result = run(
            Path::new("."),
            "sh",
            &["-c", "sleep 30 & wait"],
            None,
            Duration::from_millis(50),
        );
        assert!(!result.success);
        assert!(result.stderr.contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn output_limit_stops_noisy_child() {
        let result = run(
            Path::new("."),
            "sh",
            &[
                "-c",
                "while :; do printf '1234567890123456789012345678901234567890'; done",
            ],
            None,
            Duration::from_secs(5),
        );
        assert!(!result.success);
        assert!(result.stderr.contains("output exceeded"));
    }
}
