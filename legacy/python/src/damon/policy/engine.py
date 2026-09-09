from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum


class Decision(StrEnum):
    ALLOW = "allow"
    ASK = "ask"
    DENY = "deny"


class PolicyDenied(PermissionError):
    pass


class ApprovalRequired(PermissionError):
    pass


@dataclass(slots=True)
class Policy:
    rules: dict[str, Decision] = field(default_factory=lambda: {
        "safe": Decision.ALLOW,
        "filesystem.read": Decision.ALLOW,
        "workspace.write": Decision.ALLOW,
        "filesystem.write": Decision.ASK,
        "process.execute": Decision.ALLOW,
        "git.read": Decision.ALLOW,
        "git.write": Decision.ASK,
        "git.push": Decision.DENY,
        "privileged": Decision.DENY,
    })

    def decision(self, permission: str) -> Decision:
        return self.rules.get(permission, Decision.ASK)

    def enforce(self, permission: str, *, approved: bool = False) -> None:
        decision = self.decision(permission)
        if decision is Decision.DENY:
            raise PolicyDenied(f"permission denied by policy: {permission}")
        if decision is Decision.ASK and not approved:
            raise ApprovalRequired(f"approval required: {permission}")
