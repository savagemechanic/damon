//! Compact network relationships. Live observations and durable knowledge use
//! the same graph shape, but only configured, inferred, or learned facts cross
//! the `damon.data` boundary.
use super::{address::Cidr, address::IpAddress, address::MacAddress, InterfaceId};
use crate::{codec::*, storage};
use std::{
    collections::HashSet,
    io::{self, Write},
};

const MAX_NODES: usize = 16_384;
const MAX_FACTS: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Identity {
    Machine,
    Interface(InterfaceId),
    Address(IpAddress),
    Network(Cidr),
    Host(IpAddress),
    Router(IpAddress),
    Mac(MacAddress),
    Service {
        address: IpAddress,
        transport: TransportProtocol,
        port: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TransportProtocol {
    Tcp = 6,
    Udp = 17,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Relation {
    HasInterface = 1,
    HasAddress = 2,
    MemberOf = 3,
    HasHost = 4,
    HasMac = 5,
    UsesGateway = 6,
    GatewayFor = 7,
    RoutesTo = 8,
    ConnectsTo = 9,
    Exposes = 10,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Provenance {
    Observed = 1,
    Inferred = 2,
    Learned = 3,
    Configured = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fact {
    pub source: NodeId,
    pub relation: Relation,
    pub target: NodeId,
    pub provenance: Provenance,
    pub confidence: u8,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Topology {
    pub nodes: Vec<Identity>,
    pub facts: Vec<Fact>,
}

impl Topology {
    pub fn node(&mut self, identity: Identity) -> io::Result<NodeId> {
        if let Some(index) = self
            .nodes
            .iter()
            .position(|candidate| *candidate == identity)
        {
            return Ok(NodeId(index as u32));
        }
        if self.nodes.len() >= MAX_NODES {
            return Err(storage::invalid("network topology node limit reached"));
        }
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(identity);
        Ok(id)
    }

    pub fn record(
        &mut self,
        source: Identity,
        relation: Relation,
        target: Identity,
        evidence: Evidence,
    ) -> io::Result<()> {
        if evidence.confidence == 0 || evidence.first_seen_ms > evidence.last_seen_ms {
            return Err(storage::invalid("invalid network evidence"));
        }
        if !legal_relation(source, relation, target) {
            return Err(storage::invalid("incompatible network relationship"));
        }
        let source = self.node(source)?;
        let target = self.node(target)?;
        if let Some(fact) = self.facts.iter_mut().find(|fact| {
            fact.source == source && fact.relation == relation && fact.target == target
        }) {
            fact.first_seen_ms = fact.first_seen_ms.min(evidence.first_seen_ms);
            fact.last_seen_ms = fact.last_seen_ms.max(evidence.last_seen_ms);
            fact.confidence = fact.confidence.max(evidence.confidence);
            fact.provenance = fact.provenance.max(evidence.provenance);
            return Ok(());
        }
        if self.facts.len() >= MAX_FACTS {
            return Err(storage::invalid("network topology fact limit reached"));
        }
        self.facts.push(Fact {
            source,
            relation,
            target,
            provenance: evidence.provenance,
            confidence: evidence.confidence,
            first_seen_ms: evidence.first_seen_ms,
            last_seen_ms: evidence.last_seen_ms,
        });
        self.facts
            .sort_by_key(|fact| (fact.source, fact.relation, fact.target));
        Ok(())
    }

    pub fn facts_from(&self, source: NodeId) -> impl Iterator<Item = &Fact> {
        self.facts.iter().filter(move |fact| fact.source == source)
    }

    /// Derive raw graph facts from one bounded operating-system snapshot.
    pub fn observe_snapshot(
        interfaces: &[super::interface::NetworkInterface],
        neighbors: &[super::neighbor::Neighbor],
        routes: &[super::route::Route],
        observed_at_ms: u64,
    ) -> io::Result<Self> {
        let observed = |confidence| Evidence {
            provenance: Provenance::Observed,
            confidence,
            first_seen_ms: observed_at_ms,
            last_seen_ms: observed_at_ms,
        };
        let mut graph = Self::default();
        for interface in interfaces {
            graph.record(
                Identity::Machine,
                Relation::HasInterface,
                Identity::Interface(interface.id),
                observed(255),
            )?;
            for assigned in &interface.addresses {
                graph.record(
                    Identity::Interface(interface.id),
                    Relation::HasAddress,
                    Identity::Address(assigned.address),
                    observed(255),
                )?;
                graph.record(
                    Identity::Machine,
                    Relation::MemberOf,
                    Identity::Network(
                        Cidr::new(assigned.address, assigned.prefix)
                            .map_err(|error| storage::invalid(error.0))?,
                    ),
                    observed(250),
                )?;
            }
        }
        for neighbor in neighbors {
            let host = Identity::Host(neighbor.address);
            graph.record(
                host,
                Relation::HasAddress,
                Identity::Address(neighbor.address),
                observed(230),
            )?;
            if let Some(mac) = neighbor.mac {
                graph.record(host, Relation::HasMac, Identity::Mac(mac), observed(230))?;
            }
            if let Some(interface) = neighbor.interface {
                for network in interfaces
                    .iter()
                    .find(|candidate| candidate.id == interface)
                    .into_iter()
                    .flat_map(|candidate| &candidate.addresses)
                    .filter_map(|assigned| Cidr::new(assigned.address, assigned.prefix).ok())
                    .filter(|network| network.contains(neighbor.address))
                {
                    graph.record(
                        Identity::Network(network),
                        Relation::HasHost,
                        host,
                        observed(220),
                    )?;
                }
            }
        }
        for route in routes
            .iter()
            .filter(|route| route.flags & super::route::ROUTE_UP != 0)
        {
            if let Some(gateway) = route.gateway {
                let router = Identity::Router(gateway);
                graph.record(
                    router,
                    Relation::HasAddress,
                    Identity::Address(gateway),
                    observed(255),
                )?;
                graph.record(
                    router,
                    Relation::RoutesTo,
                    Identity::Network(route.destination),
                    observed(245),
                )?;
                if route.destination.prefix() == 0 {
                    graph.record(
                        Identity::Machine,
                        Relation::UsesGateway,
                        router,
                        observed(255),
                    )?;
                }
            }
        }
        Ok(graph)
    }

    /// Removes volatile observations and unreachable nodes before persistence.
    pub fn durable(&self) -> io::Result<Self> {
        let mut result = Self::default();
        for fact in self
            .facts
            .iter()
            .filter(|fact| fact.provenance != Provenance::Observed)
        {
            let source = *self
                .nodes
                .get(fact.source.0 as usize)
                .ok_or_else(|| storage::invalid("unknown network fact source"))?;
            let target = *self
                .nodes
                .get(fact.target.0 as usize)
                .ok_or_else(|| storage::invalid("unknown network fact target"))?;
            result.record(
                source,
                fact.relation,
                target,
                Evidence {
                    provenance: fact.provenance,
                    confidence: fact.confidence,
                    first_seen_ms: fact.first_seen_ms,
                    last_seen_ms: fact.last_seen_ms,
                },
            )?;
        }
        Ok(result)
    }

    pub(crate) fn encode(&self, writer: &mut impl Write) -> io::Result<()> {
        if self
            .facts
            .iter()
            .any(|fact| fact.provenance == Provenance::Observed)
        {
            return Err(storage::invalid(
                "volatile network observations cannot be persisted",
            ));
        }
        self.validate()?;
        write_u32(writer, self.nodes.len() as u32)?;
        for identity in &self.nodes {
            encode_identity(writer, *identity)?;
        }
        write_u32(writer, self.facts.len() as u32)?;
        for fact in &self.facts {
            write_u32(writer, fact.source.0)?;
            writer.write_all(&[fact.relation as u8, fact.provenance as u8, fact.confidence])?;
            write_u32(writer, fact.target.0)?;
            write_u64(writer, fact.first_seen_ms)?;
            write_u64(writer, fact.last_seen_ms)?;
        }
        Ok(())
    }

    pub(crate) fn decode(reader: &mut Reader<'_>) -> io::Result<Self> {
        let node_count = reader.u32()? as usize;
        if node_count > MAX_NODES {
            return Err(storage::invalid("too many network topology nodes"));
        }
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            nodes.push(decode_identity(reader)?);
        }
        let fact_count = reader.u32()? as usize;
        if fact_count > MAX_FACTS {
            return Err(storage::invalid("too many network topology facts"));
        }
        let mut facts = Vec::with_capacity(fact_count);
        for _ in 0..fact_count {
            let source = NodeId(reader.u32()?);
            let relation = decode_relation(reader.u8()?)?;
            let provenance = match reader.u8()? {
                2 => Provenance::Inferred,
                3 => Provenance::Learned,
                4 => Provenance::Configured,
                _ => return Err(storage::invalid("invalid durable network provenance")),
            };
            let confidence = reader.u8()?;
            let target = NodeId(reader.u32()?);
            facts.push(Fact {
                source,
                relation,
                target,
                provenance,
                confidence,
                first_seen_ms: reader.u64()?,
                last_seen_ms: reader.u64()?,
            });
        }
        let result = Self { nodes, facts };
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> io::Result<()> {
        if self.nodes.len() > MAX_NODES || self.facts.len() > MAX_FACTS {
            return Err(storage::invalid("network topology limit exceeded"));
        }
        let mut unique = HashSet::with_capacity(self.nodes.len());
        if self.nodes.iter().any(|node| !unique.insert(*node)) {
            return Err(storage::invalid("duplicate network topology node"));
        }
        let mut previous = None;
        for fact in &self.facts {
            let key = (fact.source, fact.relation, fact.target);
            if fact.source.0 as usize >= self.nodes.len()
                || fact.target.0 as usize >= self.nodes.len()
                || fact.confidence == 0
                || fact.first_seen_ms > fact.last_seen_ms
                || previous.is_some_and(|candidate| candidate >= key)
            {
                return Err(storage::invalid("invalid network topology fact"));
            }
            if !legal_relation(
                self.nodes[fact.source.0 as usize],
                fact.relation,
                self.nodes[fact.target.0 as usize],
            ) {
                return Err(storage::invalid("incompatible network relationship"));
            }
            previous = Some(key);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub provenance: Provenance,
    pub confidence: u8,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
}

fn encode_identity(writer: &mut impl Write, identity: Identity) -> io::Result<()> {
    match identity {
        Identity::Machine => writer.write_all(&[1]),
        Identity::Interface(id) => {
            writer.write_all(&[2])?;
            write_u32(writer, id.0)
        }
        Identity::Address(address) => {
            writer.write_all(&[3])?;
            encode_ip(writer, address)
        }
        Identity::Network(cidr) => {
            writer.write_all(&[4])?;
            encode_ip(writer, cidr.network())?;
            writer.write_all(&[cidr.prefix()])
        }
        Identity::Host(address) => {
            writer.write_all(&[5])?;
            encode_ip(writer, address)
        }
        Identity::Router(address) => {
            writer.write_all(&[6])?;
            encode_ip(writer, address)
        }
        Identity::Mac(address) => {
            writer.write_all(&[7])?;
            writer.write_all(&address.octets())
        }
        Identity::Service {
            address,
            transport,
            port,
        } => {
            writer.write_all(&[8])?;
            encode_ip(writer, address)?;
            writer.write_all(&[transport as u8])?;
            write_u16(writer, port)
        }
    }
}

fn decode_identity(reader: &mut Reader<'_>) -> io::Result<Identity> {
    Ok(match reader.u8()? {
        1 => Identity::Machine,
        2 => Identity::Interface(InterfaceId(reader.u32()?)),
        3 => Identity::Address(decode_ip(reader)?),
        4 => {
            let address = decode_ip(reader)?;
            Identity::Network(
                Cidr::new(address, reader.u8()?).map_err(|error| storage::invalid(error.0))?,
            )
        }
        5 => Identity::Host(decode_ip(reader)?),
        6 => Identity::Router(decode_ip(reader)?),
        7 => {
            let mut bytes = [0; 6];
            bytes.copy_from_slice(reader.take(6)?);
            Identity::Mac(MacAddress::new(bytes))
        }
        8 => {
            let address = decode_ip(reader)?;
            let transport = match reader.u8()? {
                6 => TransportProtocol::Tcp,
                17 => TransportProtocol::Udp,
                _ => return Err(storage::invalid("unknown service transport")),
            };
            Identity::Service {
                address,
                transport,
                port: reader.u16()?,
            }
        }
        _ => return Err(storage::invalid("unknown network topology node")),
    })
}

fn encode_ip(writer: &mut impl Write, address: IpAddress) -> io::Result<()> {
    match address {
        IpAddress::V4(address) => {
            writer.write_all(&[4])?;
            writer.write_all(&address.octets())
        }
        IpAddress::V6(address) => {
            writer.write_all(&[6])?;
            writer.write_all(&address.octets())
        }
    }
}

fn decode_ip(reader: &mut Reader<'_>) -> io::Result<IpAddress> {
    Ok(match reader.u8()? {
        4 => {
            let mut bytes = [0; 4];
            bytes.copy_from_slice(reader.take(4)?);
            IpAddress::V4(super::address::Ipv4Address::new(bytes))
        }
        6 => {
            let mut bytes = [0; 16];
            bytes.copy_from_slice(reader.take(16)?);
            IpAddress::V6(super::address::Ipv6Address::new(bytes))
        }
        _ => return Err(storage::invalid("unknown network address family")),
    })
}

fn decode_relation(value: u8) -> io::Result<Relation> {
    Ok(match value {
        1 => Relation::HasInterface,
        2 => Relation::HasAddress,
        3 => Relation::MemberOf,
        4 => Relation::HasHost,
        5 => Relation::HasMac,
        6 => Relation::UsesGateway,
        7 => Relation::GatewayFor,
        8 => Relation::RoutesTo,
        9 => Relation::ConnectsTo,
        10 => Relation::Exposes,
        _ => return Err(storage::invalid("unknown network relation")),
    })
}

fn legal_relation(source: Identity, relation: Relation, target: Identity) -> bool {
    matches!(
        (source, relation, target),
        (
            Identity::Machine,
            Relation::HasInterface,
            Identity::Interface(_)
        ) | (
            Identity::Interface(_) | Identity::Host(_) | Identity::Router(_),
            Relation::HasAddress,
            Identity::Address(_)
        ) | (
            Identity::Machine | Identity::Host(_),
            Relation::MemberOf,
            Identity::Network(_)
        ) | (Identity::Network(_), Relation::HasHost, Identity::Host(_))
            | (
                Identity::Interface(_) | Identity::Host(_) | Identity::Router(_),
                Relation::HasMac,
                Identity::Mac(_)
            )
            | (
                Identity::Machine,
                Relation::UsesGateway,
                Identity::Router(_)
            )
            | (
                Identity::Router(_),
                Relation::GatewayFor,
                Identity::Network(_)
            )
            | (
                Identity::Router(_),
                Relation::RoutesTo,
                Identity::Network(_)
            )
            | (
                Identity::Machine | Identity::Host(_),
                Relation::ConnectsTo,
                Identity::Host(_) | Identity::Router(_)
            )
            | (
                Identity::Host(_) | Identity::Router(_),
                Relation::Exposes,
                Identity::Service { .. }
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_derives_machine_network_host_and_gateway_relationships() {
        let local = "192.168.1.20".parse().unwrap();
        let gateway = "192.168.1.1".parse().unwrap();
        let interface = super::super::interface::NetworkInterface {
            id: InterfaceId(7),
            kind: super::super::interface::InterfaceKind::Wifi,
            name: "en0".into(),
            index: 7,
            mac: Some("00:11:22:33:44:55".parse().unwrap()),
            mtu: Some(1500),
            state: super::super::interface::InterfaceState {
                up: true,
                running: true,
            },
            addresses: vec![super::super::interface::InterfaceAddress {
                address: local,
                prefix: 24,
            }],
            statistics: None,
        };
        let neighbor = super::super::neighbor::Neighbor {
            address: gateway,
            mac: Some("aa:bb:cc:dd:ee:ff".parse().unwrap()),
            interface: Some(InterfaceId(7)),
            state: super::super::neighbor::NeighborState::Reachable,
            evidence: super::super::neighbor::Evidence::SystemNeighborTable,
            observed_at_ms: 40,
        };
        let route = super::super::route::Route {
            destination: "0.0.0.0/0".parse().unwrap(),
            gateway: Some(gateway),
            interface: InterfaceId(7),
            metric: Some(10),
            flags: super::super::route::ROUTE_UP,
        };
        let graph = Topology::observe_snapshot(&[interface], &[neighbor], &[route], 42).unwrap();
        assert!(graph
            .nodes
            .contains(&Identity::Network("192.168.1.0/24".parse().unwrap())));
        assert!(graph.facts.iter().any(|fact| {
            fact.relation == Relation::UsesGateway
                && graph.nodes[fact.source.0 as usize] == Identity::Machine
                && graph.nodes[fact.target.0 as usize] == Identity::Router(gateway)
        }));
        assert!(graph.facts.iter().any(|fact| {
            fact.relation == Relation::HasHost
                && graph.nodes[fact.target.0 as usize] == Identity::Host(gateway)
        }));
        assert!(graph
            .facts
            .iter()
            .all(|fact| fact.provenance == Provenance::Observed));
    }

    #[test]
    fn evidence_merges_and_durable_graph_excludes_observations() {
        let host = Identity::Host("192.168.1.20".parse().unwrap());
        let mac = Identity::Mac("aa:bb:cc:dd:ee:ff".parse().unwrap());
        let mut graph = Topology::default();
        graph
            .record(
                host,
                Relation::HasMac,
                mac,
                Evidence {
                    provenance: Provenance::Observed,
                    confidence: 150,
                    first_seen_ms: 10,
                    last_seen_ms: 10,
                },
            )
            .unwrap();
        graph
            .record(
                host,
                Relation::HasMac,
                mac,
                Evidence {
                    provenance: Provenance::Learned,
                    confidence: 230,
                    first_seen_ms: 20,
                    last_seen_ms: 30,
                },
            )
            .unwrap();
        assert_eq!(graph.facts.len(), 1);
        assert_eq!(graph.facts[0].first_seen_ms, 10);
        assert_eq!(graph.facts[0].last_seen_ms, 30);
        assert_eq!(graph.durable().unwrap().facts.len(), 1);

        let mut transient = Topology::default();
        transient
            .record(
                Identity::Machine,
                Relation::ConnectsTo,
                host,
                Evidence {
                    provenance: Provenance::Observed,
                    confidence: 200,
                    first_seen_ms: 1,
                    last_seen_ms: 1,
                },
            )
            .unwrap();
        assert!(transient.durable().unwrap().facts.is_empty());
        assert!(transient.encode(&mut Vec::new()).is_err());

        let durable = graph.durable().unwrap();
        let mut bytes = Vec::new();
        durable.encode(&mut bytes).unwrap();
        let mut reader = Reader {
            bytes: &bytes,
            pos: 0,
        };
        assert_eq!(Topology::decode(&mut reader).unwrap(), durable);
        assert_eq!(reader.pos, bytes.len());
    }
}
