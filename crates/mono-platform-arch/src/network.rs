use mono_core::{NetworkCheck, Result};
use std::{
    net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs},
    sync::mpsc,
    time::{Duration, Instant},
};

const DNS_TIMEOUT: Duration = Duration::from_secs(3);
const TCP_BUDGET: Duration = Duration::from_secs(5);

fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let host = host.to_owned();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("dns".into())
        .spawn(move || {
            let result = (host.as_str(), port)
                .to_socket_addrs()
                .map(|values| values.collect::<Vec<_>>())
                .map_err(|e| format!("DNS failed: {e}"));
            let _ = tx.send(result);
        })
        .map_err(|e| format!("Cannot start DNS resolver: {e}"))?;
    receive(rx, DNS_TIMEOUT)
}

fn receive(
    rx: mpsc::Receiver<Result<Vec<SocketAddr>>>,
    timeout: Duration,
) -> Result<Vec<SocketAddr>> {
    rx.recv_timeout(timeout).map_err(|e| match e {
        mpsc::RecvTimeoutError::Timeout => "DNS timed out".to_owned(),
        mpsc::RecvTimeoutError::Disconnected => "DNS resolver stopped".to_owned(),
    })?
}

pub fn check(host: &str, port: u16) -> Result<NetworkCheck> {
    let mut addresses = resolve(host, port)?;
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() {
        return Err("DNS returned no addresses".into());
    }
    let deadline = Instant::now() + TCP_BUDGET;
    let mut result = NetworkCheck {
        addresses,
        connected: None,
        failures: Vec::new(),
    };
    for address in &result.addresses {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            result
                .failures
                .push("TCP time budget exhausted; remaining addresses not checked".into());
            break;
        }
        match TcpStream::connect_timeout(address, remaining.min(Duration::from_secs(1))) {
            Ok(_) => {
                result.connected = Some(*address);
                break;
            }
            Err(e) => result.failures.push(format!("{address}: {e}")),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connects_to_local_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        assert_eq!(
            check("127.0.0.1", address.port()).unwrap().connected,
            Some(address)
        );
    }
    #[test]
    fn resolver_wait_is_bounded() {
        let (_tx, rx) = mpsc::channel();
        assert_eq!(
            receive(rx, Duration::from_millis(10)).unwrap_err(),
            "DNS timed out"
        );
    }
    #[test]
    fn resolver_error_is_preserved() {
        let (tx, rx) = mpsc::channel();
        tx.send(Err("DNS failed: test".into())).unwrap();
        assert_eq!(receive(rx, DNS_TIMEOUT).unwrap_err(), "DNS failed: test");
    }
}
