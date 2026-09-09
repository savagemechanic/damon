//! Fundamental userspace networking: typed addresses, byte-backed packets,
//! operating-system observations, routes, sockets, flows, and graph evidence.
//! Higher protocols consume these representations; they do not define them.
pub mod address;
pub mod connection;
pub mod interface;
pub mod neighbor;
pub mod packet;
pub mod route;
pub mod socket;
pub mod topology;
pub mod wifi;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InterfaceId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NetworkId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SocketId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId(pub u32);
