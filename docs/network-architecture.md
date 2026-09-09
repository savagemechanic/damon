# Fundamental networking architecture

To Damon, a network is machines exchanging arrays of bytes according to
protocols. The web is one possible application above that foundation.

```text
bytes
  -> link and interfaces
  -> local networks and neighbors
  -> IP addresses and routes
  -> transport sockets and connections
  -> protocol state machines
  -> application clients
```

HTTP, APIs, browsers, router administration, and security tools must consume
this core rather than define it. Damon uses kernel/driver facilities for actual
hardware and transport. It does not reimplement Wi-Fi firmware, TCP/IP, or
cryptography.

## Current foundation

`src/network` now provides:

- fixed-width typed MAC, IPv4, IPv6, port-adjacent transport, interface, socket,
  connection, and network IDs;
- normalized CIDR values, membership arithmetic, and deterministic longest-prefix
  route choice with metric tie breaking;
- direct Unix `getifaddrs` interface/address discovery, with Linux sysfs MTU and
  counters and explicit unknown values where the OS does not expose data yet;
- structured Linux `/proc/net/route` and bounded macOS routing-table discovery,
  default-gateway selection, and native English questions for interfaces/routes;
- passive Linux ARP and bounded macOS ARP/IPv6 neighbor-table discovery with
  address, MAC, interface, state, evidence source, and observation timestamp;
- byte-owned packets whose parsed layers and payload are offsets/views over one
  original buffer;
- bounded Ethernet dispatch into ARP, IPv4, IPv6, TCP, UDP, and ICMP metadata;
- bounded TCP connect/listen/accept/send/receive/close and UDP bind/send/receive;
- packet-to-connection aggregation, counters, basic TCP state, and conservative
  protocol guesses.

Packet parsing never assumes that all layers exist. Unknown EtherTypes and IP
protocols remain valid lower-layer observations. Truncation, inconsistent
lengths, unsupported ARP layouts, and excess parsing depth fail explicitly.

## Safety and lifetime rules

Passive, system-known state comes before active probing. `not observed` never
means `does not exist`. Active discovery and packet capture will be separate,
policy-visible effects. Capture must be explicitly enabled, filtered, bounded by
time/output, and never persist full payloads by default.

Socket operations require a nonzero timeout of at most 120 seconds and cap each
send/receive at 1 MiB. These are library primitives, not authorization: exposed
runtime actions must still declare `NETWORK`, `PROCESS`, `READ`, `WRITE`,
`CREDENTIAL`, `PRIVILEGED`, or `DESTRUCTIVE` effects as applicable.

## State classes

| Lifetime | Examples | Default treatment |
| --- | --- | --- |
| Volatile | packet bytes, socket buffers, current TCP state | memory only |
| Short-lived | DNS answers, neighbors, active connections | timestamped cache |
| Versioned | interfaces, addresses, routes | invalidate derived decisions |
| Long-lived learned | router identity, host aliases, normal services | compact metadata in `damon.data` |

The brain remains metadata and knowledge, not a packet archive or binary blob
store. Provenance-aware graph records will distinguish observed, inferred,
configured, and learned relationships with confidence and freshness.

## Remaining integration order

1. macOS Wi-Fi metadata within OS privacy boundaries and richer IPv6 neighbor
   state where directly available.
2. Socket/process inventory and owner resolution.
3. Explicit bounded packet observation behind policy.
4. Network graph observations and compact selective persistence.
5. DNS resolution/protocol parsing, DHCP observations, and staged diagnostics.
6. Expand natural-language route/interface meanings into staged diagnosis.
7. TLS metadata, HTTP, SSH, router management, and security as compositions over
   lower layers.

No higher-layer client may become the architectural shortcut around this order.
