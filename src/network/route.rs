use super::{address::Cidr, address::IpAddress, InterfaceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub destination: Cidr,
    pub gateway: Option<IpAddress>,
    pub interface: InterfaceId,
    pub metric: Option<u32>,
    pub flags: u32,
}

pub const ROUTE_UP: u32 = 1;

pub fn select(routes: &[Route], destination: IpAddress) -> Option<&Route> {
    routes
        .iter()
        .filter(|route| route.flags & ROUTE_UP != 0 && route.destination.contains(destination))
        .max_by_key(|route| {
            (
                route.destination.prefix(),
                std::cmp::Reverse(route.metric.unwrap_or(u32::MAX)),
            )
        })
}

pub fn is_local(routes: &[Route], destination: IpAddress) -> bool {
    select(routes, destination).is_some_and(|route| route.gateway.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_then_lowest_metric_wins() {
        let routes = [
            Route {
                destination: "0.0.0.0/0".parse().unwrap(),
                gateway: Some("192.168.1.1".parse().unwrap()),
                interface: InterfaceId(4),
                metric: Some(20),
                flags: ROUTE_UP,
            },
            Route {
                destination: "10.0.0.0/8".parse().unwrap(),
                gateway: None,
                interface: InterfaceId(7),
                metric: Some(100),
                flags: ROUTE_UP,
            },
            Route {
                destination: "10.2.0.0/16".parse().unwrap(),
                gateway: Some("10.0.0.2".parse().unwrap()),
                interface: InterfaceId(8),
                metric: Some(30),
                flags: ROUTE_UP,
            },
        ];
        let chosen = select(&routes, "10.2.3.4".parse().unwrap()).unwrap();
        assert_eq!(chosen.interface, InterfaceId(8));
        assert!(is_local(&routes, "10.9.8.7".parse().unwrap()));
        assert!(!is_local(&routes, "8.8.8.8".parse().unwrap()));
    }
}
