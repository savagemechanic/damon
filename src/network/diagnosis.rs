use std::{io, net::SocketAddr, net::TcpStream, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Interface,
    Address,
    Route,
    Dns,
    Transport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub stage: Stage,
    pub passed: bool,
    pub detail: String,
}

pub fn internet() -> Vec<Evidence> {
    let mut evidence = Vec::new();
    let interfaces = match super::interface::discover() {
        Ok(interfaces) => interfaces,
        Err(error) => {
            evidence.push(failed(Stage::Interface, error));
            return evidence;
        }
    };
    let active = interfaces
        .iter()
        .filter(|interface| interface.state.up && interface.state.running)
        .count();
    if active == 0 {
        evidence.push(Evidence {
            stage: Stage::Interface,
            passed: false,
            detail: "no interface is both up and running".into(),
        });
        return evidence;
    }
    evidence.push(Evidence {
        stage: Stage::Interface,
        passed: true,
        detail: format!("{active} interface(s) are up and running"),
    });
    let addressed = interfaces
        .iter()
        .filter(|interface| interface.state.up && !interface.addresses.is_empty())
        .count();
    if addressed == 0 {
        evidence.push(Evidence {
            stage: Stage::Address,
            passed: false,
            detail: "no active interface has an IP address".into(),
        });
        return evidence;
    }
    evidence.push(Evidence {
        stage: Stage::Address,
        passed: true,
        detail: format!("{addressed} active interface(s) have an IP address"),
    });
    let routes = match super::route::discover() {
        Ok(routes) => routes,
        Err(error) => {
            evidence.push(failed(Stage::Route, error));
            return evidence;
        }
    };
    let Some(route) = super::route::default_gateway(&routes) else {
        evidence.push(Evidence {
            stage: Stage::Route,
            passed: false,
            detail: "no active default route was observed".into(),
        });
        return evidence;
    };
    evidence.push(Evidence {
        stage: Stage::Route,
        passed: true,
        detail: format!(
            "default traffic uses {}",
            route
                .gateway
                .map_or_else(|| "an on-link route".into(), |address| address.to_string())
        ),
    });
    let answer = match super::protocol::resolve_system("example.com") {
        Ok(answer) => answer,
        Err(error) => {
            evidence.push(failed(Stage::Dns, error));
            return evidence;
        }
    };
    evidence.push(Evidence {
        stage: Stage::Dns,
        passed: true,
        detail: format!(
            "example.com resolved to {} address(es)",
            answer.addresses.len()
        ),
    });
    let target = answer.addresses.into_iter().find_map(|address| {
        address
            .to_string()
            .parse()
            .ok()
            .map(|ip| SocketAddr::new(ip, 443))
    });
    match target {
        Some(target) => evidence.push(transport(target, Duration::from_secs(3))),
        None => evidence.push(Evidence {
            stage: Stage::Transport,
            passed: false,
            detail: "the resolved address could not be used as a socket endpoint".into(),
        }),
    }
    evidence
}

pub fn transport(target: SocketAddr, timeout: Duration) -> Evidence {
    match TcpStream::connect_timeout(&target, timeout) {
        Ok(stream) => {
            drop(stream);
            Evidence {
                stage: Stage::Transport,
                passed: true,
                detail: format!("TCP connected to {target}"),
            }
        }
        Err(error) => Evidence {
            stage: Stage::Transport,
            passed: false,
            detail: transport_error(&error),
        },
    }
}

fn transport_error(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::ConnectionRefused => "the destination refused the TCP connection".into(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
            "the TCP connection timed out".into()
        }
        io::ErrorKind::NetworkUnreachable => "the destination network is unreachable".into(),
        io::ErrorKind::HostUnreachable => "the destination host is unreachable".into(),
        _ => format!("the TCP connection failed: {error}"),
    }
}

fn failed(stage: Stage, error: impl std::fmt::Display) -> Evidence {
    Evidence {
        stage,
        passed: false,
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn transport_distinguishes_success_from_refusal() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        assert!(transport(address, Duration::from_secs(1)).passed);
        drop(listener);
        let refused = transport(address, Duration::from_millis(100));
        assert!(!refused.passed);
        assert!(refused.detail.contains("refused") || refused.detail.contains("failed"));
    }
}
