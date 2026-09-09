# Semantic graphs and execution

English normalization produces a bounded beam (width four) of compact candidate
node/edge arrays. Integer lexical scores combine with log-count priors; margins
select confident candidates. This is a small count-based approximation, not a
calibrated universal English parser. Two-clause grammar patterns preserve
sequence and conditional edges without enumerating all parses.

Current executable actions are Git status, actual Git diff, discovered tests,
file listing, and files in yesterday's commits attributed to the configured Git
user email. Uncommitted edit dates cannot be reconstructed from Git. Graph nodes
represent actions, project entities, object concepts, and yesterday. Relations
include target, object, time, dependency, and success condition. The relation
vocabulary also reserves source, destination, reference, modifier, result, and
action; unsupported executable uses are rejected, never silently ignored.

```text
run the tests in Damon
show me what changed
show me the files I changed yesterday
run the tests and if they pass show me the diff
```

The planner validates the entire graph, translates each action using registered
tool metadata, and checks policy for the whole plan before execution. Conditional
steps run only when their prerequisite actually succeeds. Tool effects are
recomputed by policy, so a fabricated low-effect action cannot bypass it.

The teacher returns strict node and edge lines, never shell commands or prose:

```text
N action 3
N entity 0
N action 2
E 0 target 1
E 2 target 1
E 2 condition 0
```

This graph runs tests for project zero and runs its diff only on success. Teacher
output is capped at 8 KiB, 32 nodes, and 64 edges. Unknown actions/entities,
invalid endpoints, duplicate edges, cycles/forward dependencies, incompatible
objects, and unsupported relations are rejected. Explicit temporal/conditional
constraints are checked against the request. Negations and unsupported time
constraints currently require clarification rather than an approximation.
An invalid answer falls through to the next enabled provider.

Only verified successful execution retains an exact phrase's entire graph in
`damon.data`. A repeated learned composite preserves all steps and dependencies;
it is never reduced to its first intent. Failed execution removes that exact
graph. At most 4,096 graphs are retained. Reuse still validates the graph and
passes through the same planner/policy/tool path. Tests prove reuse with model
providers disabled and graph persistence across reopening the brain.

Pronoun context, richer world relationships, file copy/edit semantics, learned
procedures, and dependency-aware cached discovery are subsequent implementation
milestones; they are not claimed by this initial graph boundary.
