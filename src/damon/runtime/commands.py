from __future__ import annotations

import asyncio
import os
import shlex
import signal
import time
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Protocol, TypeAlias


@dataclass(frozen=True, slots=True)
class SecretRef:
    """Opaque reference to a secret resolved outside model-visible context."""

    name: str


class SecretResolver(Protocol):
    async def resolve(self, ref: SecretRef) -> str: ...


EnvValue: TypeAlias = str | SecretRef


@dataclass(frozen=True, slots=True)
class CommandSpec:
    argv: tuple[str, ...]
    capability: str
    mutates: bool = False
    network: bool = False
    privileged: bool = False


@dataclass(slots=True)
class CommandResult:
    argv: tuple[str, ...]
    cwd: Path
    returncode: int
    stdout: str
    stderr: str
    duration_ms: int
    timed_out: bool = False
    output_truncated: bool = False
    spec: CommandSpec | None = None

    @property
    def ok(self) -> bool:
        return self.returncode == 0 and not self.timed_out

    @property
    def command(self) -> str:
        return shlex.join(self.argv)

    def compact(self) -> dict[str, object]:
        return {
            "command": self.command,
            "exit_code": self.returncode,
            "stdout": self.stdout,
            "stderr": self.stderr,
            "duration_ms": self.duration_ms,
            "timed_out": self.timed_out,
            "output_truncated": self.output_truncated,
            "capability": self.spec.capability if self.spec else "process.execute",
        }


_MUTATING_GIT = {"add", "apply", "branch", "checkout", "cherry-pick", "clean", "commit", "merge", "mv", "rebase", "reset", "restore", "rm", "switch", "tag"}
_NETWORK_GIT = {"clone", "fetch", "pull", "push", "ls-remote", "submodule"}
_PRIVILEGED = {"sudo", "su", "doas", "launchctl", "diskutil", "shutdown", "reboot", "halt"}
_NETWORK = {"ssh", "scp", "sftp", "curl", "wget", "nc", "netcat", "rsync"}
_MUTATING = {"rm", "rmdir", "mv", "cp", "mkdir", "touch", "chmod", "chown", "dd", "mkfs"}


def classify_command(argv: Sequence[str]) -> CommandSpec:
    if not argv:
        raise ValueError("argv cannot be empty")
    executable = Path(argv[0]).name
    args = tuple(str(part) for part in argv)

    if executable == "git":
        subcommand = next((part for part in args[1:] if not part.startswith("-")), "")
        network = subcommand in _NETWORK_GIT
        mutates = subcommand in _MUTATING_GIT or subcommand == "push"
        capability = "git.push" if subcommand == "push" else ("git.write" if mutates else "git.read")
        return CommandSpec(args, capability=capability, mutates=mutates, network=network)

    privileged = executable in _PRIVILEGED
    network = executable in _NETWORK
    mutates = executable in _MUTATING
    capability = "privileged" if privileged else "process.execute"
    return CommandSpec(args, capability=capability, mutates=mutates, network=network, privileged=privileged)


@dataclass(slots=True)
class CommandRunner:
    root: Path
    secret_resolver: SecretResolver | None = None
    max_output_chars: int = 12_000
    inherit_env: bool = True
    base_env: Mapping[str, str] = field(default_factory=dict)

    def __post_init__(self) -> None:
        self.root = self.root.resolve()
        if self.max_output_chars < 1:
            raise ValueError("max_output_chars must be >= 1")

    def _resolve_cwd(self, cwd: Path | None) -> Path:
        target = (cwd or self.root).resolve()
        try:
            target.relative_to(self.root)
        except ValueError as exc:
            raise PermissionError(f"command cwd escapes workspace: {target}") from exc
        return target

    async def _build_env(self, env: Mapping[str, EnvValue] | None) -> tuple[dict[str, str], tuple[str, ...]]:
        resolved = dict(os.environ) if self.inherit_env else {}
        resolved.update(self.base_env)
        secrets: list[str] = []
        if not env:
            return resolved, ()
        for key, value in env.items():
            if isinstance(value, SecretRef):
                if self.secret_resolver is None:
                    raise RuntimeError(f"no secret resolver configured for {value.name}")
                secret = await self.secret_resolver.resolve(value)
                resolved[key] = secret
                if secret:
                    secrets.append(secret)
            else:
                resolved[key] = value
        return resolved, tuple(secrets)

    def _bound(self, text: str) -> tuple[str, bool]:
        if len(text) <= self.max_output_chars:
            return text, False
        return text[-self.max_output_chars :], True

    async def run(
        self,
        argv: Sequence[str],
        *,
        cwd: Path | None = None,
        env: Mapping[str, EnvValue] | None = None,
        timeout: float = 60.0,
        stdin: str | bytes | None = None,
    ) -> CommandResult:
        args = tuple(str(part) for part in argv)
        spec = classify_command(args)
        target = self._resolve_cwd(cwd)
        process_env, injected_secrets = await self._build_env(env)
        input_bytes = stdin.encode() if isinstance(stdin, str) else stdin
        start = time.monotonic()

        process = await asyncio.create_subprocess_exec(
            *args,
            cwd=target,
            env=process_env,
            stdin=asyncio.subprocess.PIPE if input_bytes is not None else None,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            start_new_session=True,
        )
        timed_out = False
        try:
            stdout_bytes, stderr_bytes = await asyncio.wait_for(process.communicate(input_bytes), timeout=timeout)
        except TimeoutError:
            timed_out = True
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            stdout_bytes, stderr_bytes = await process.communicate()
        except asyncio.CancelledError:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            await process.wait()
            raise

        stdout_text = stdout_bytes.decode(errors="replace")
        stderr_text = stderr_bytes.decode(errors="replace")
        for secret in injected_secrets:
            stdout_text = stdout_text.replace(secret, "[REDACTED]")
            stderr_text = stderr_text.replace(secret, "[REDACTED]")
        stdout, stdout_truncated = self._bound(stdout_text)
        stderr, stderr_truncated = self._bound(stderr_text)
        duration_ms = int((time.monotonic() - start) * 1000)
        return CommandResult(
            argv=args,
            cwd=target,
            returncode=process.returncode if process.returncode is not None else -1,
            stdout=stdout,
            stderr=stderr,
            duration_ms=duration_ms,
            timed_out=timed_out,
            output_truncated=stdout_truncated or stderr_truncated,
            spec=spec,
        )
