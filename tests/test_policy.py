import pytest

from damon.policy.engine import ApprovalRequired, Policy, PolicyDenied


def test_policy_defaults_are_conservative():
    policy = Policy()
    policy.enforce("filesystem.read")
    policy.enforce("workspace.write")
    with pytest.raises(ApprovalRequired):
        policy.enforce("git.write")
    with pytest.raises(PolicyDenied):
        policy.enforce("git.push")
