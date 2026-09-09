# Native capability direction

Damon resolves English to a capability before choosing an implementation. The
preferred execution order is native Damon primitive, composition, direct system
or library call, direct protocol client, acquisition of the smallest missing
primitive, structured external interface, accessibility, then pixels as the last
resort.

Human-facing applications are not the computer. Finder is filesystem behavior;
Terminal is process execution; Postman is a protocol client; Wireshark is packet
capture and parsing. Applications may be endpoints, data sources, or temporary
teachers, but they should not become permanent control surfaces when Damon can
perform the underlying computation directly.

The capability graph will use compact `CapabilityId` records and relationships
for implementation, composition, required formats, verification, and provenance.
Procedures compose existing capabilities. Primitives implement a genuinely
missing operation. Every implementation declares effects and remains subject to
the same policy path. Generated code must be isolated, tested against real output,
versioned, reversible, and registered only after verification; compilation alone
is not proof.

`damon.data` should retain capability IDs, aliases, implementation metadata,
dependencies, provenance, verification state, versions, outcomes, and preference.
Managed source/build artifacts belong outside the brain. Credentials remain
opaque references and never enter capability metadata.

## Implemented kernel

Tao now persists compact capability, implementation, and dependency arrays.
Implementation records carry kind, effect-bearing capability, tool/procedure
binding, verification state, provenance, and verified outcome counters. Planning
maps semantic intents to capability IDs and resolves a verified implementation
before producing a tool action. Policy rejects a tool that is not the registered
implementation of the declared capability. Native verified implementations beat
application references, and generated implementations do not resolve merely
because they compiled.

The canonical graph covers current Git/file/test tools and reserves
fundamental interface, route, neighbor, socket, packet-capture, TCP, and UDP
capabilities. Interface, route, passive neighbor, socket inventory, staged network
diagnosis, file copy, and bounded file discovery resolve to verified native tools;
the canonical semantic IR never sees those tool bindings. Verified repeated
multi-action plans persist as compact capability instructions and reenter the
same policy and verification path on reuse. Isolated generated-source artifact
management, rollback, and the full primitive-acquisition loop remain future work.
