use crate::{NetworkStatus, Platform, Result, SystemStatus};

#[derive(Debug)]
pub struct PackageName(String);

impl PackageName {
    pub fn new(name: &str) -> Result<Self> {
        if name.is_empty()
            || name.starts_with(['-', '.'])
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"@._+-".contains(&c))
        {
            return Err(format!("Invalid package name: {name}"));
        }
        Ok(Self(name.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn status(platform: &impl Platform) -> Result<SystemStatus> {
    platform.status()
}
pub fn install(platform: &impl Platform, name: &str) -> Result<()> {
    platform.install(&PackageName::new(name)?)
}
pub fn network(platform: &impl Platform) -> Result<NetworkStatus> {
    platform.network()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_options_and_shell_expressions() {
        for name in ["", "--help", "-S", ".hidden", "a;b", "$(id)", "a b", "a/b"] {
            assert!(PackageName::new(name).is_err(), "{name}");
        }
        for name in ["nmap", "gcc-libs", "libc++", "python2.7", "foo@bar"] {
            assert!(PackageName::new(name).is_ok(), "{name}");
        }
    }
}

pub fn network_check(
    platform: &impl Platform,
    host: &str,
    port: u16,
) -> Result<crate::NetworkCheck> {
    validate_host(host)?;
    if port == 0 {
        return Err("Port must be 1..65535".into());
    }
    platform.network_check(host, port)
}

fn validate_host(host: &str) -> Result<()> {
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    let name = host.strip_suffix('.').unwrap_or(host);
    if name.is_empty()
        || name.len() > 253
        || !name.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err("Expected a hostname or IP address, without URL or port".into());
    }
    Ok(())
}

#[cfg(test)]
mod network_tests {
    use super::*;
    #[test]
    fn validates_targets() {
        for host in [
            "localhost",
            "archlinux.org",
            "archlinux.org.",
            "127.0.0.1",
            "::1",
        ] {
            assert!(validate_host(host).is_ok());
        }
        for host in [
            "",
            "https://archlinux.org",
            "host:443",
            "-host",
            "a..b",
            "a b",
            "$(id)",
        ] {
            assert!(validate_host(host).is_err());
        }
    }
}
