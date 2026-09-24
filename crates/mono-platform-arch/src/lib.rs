pub mod commands;
mod network;
use mono_core::{NetworkStatus, Platform, Result, SystemStatus, capabilities::PackageName};
use std::{fs, process::Command};

pub struct Arch;

fn output(command: &str, args: &[&str]) -> Result<String> {
    let result = Command::new(command)
        .args(args)
        .output()
        .map_err(|e| format!("Cannot start {command}: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "{command}: {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

fn os_value<'a>(data: &'a str, key: &str) -> Option<&'a str> {
    data.lines()
        .filter_map(|line| line.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.trim_matches('"'))
}

fn platform(kernel: &str) -> &'static str {
    let kernel = kernel.to_ascii_lowercase();
    if kernel.contains("microsoft-standard") || kernel.contains("wsl2") {
        "WSL2"
    } else if kernel.contains("microsoft") {
        "WSL"
    } else {
        "Linux"
    }
}

impl Platform for Arch {
    fn network_check(&self, host: &str, port: u16) -> Result<mono_core::NetworkCheck> {
        network::check(host, port)
    }

    fn status(&self) -> Result<SystemStatus> {
        let kernel = output("uname", &["-r"])?;
        let os = fs::read_to_string("/etc/os-release").map_err(|e| e.to_string())?;
        Ok(SystemStatus {
            platform: platform(&kernel).into(),
            os: os_value(&os, "PRETTY_NAME").unwrap_or("unknown").into(),
            architecture: output("uname", &["-m"])?,
            kernel,
            init: fs::read_to_string("/proc/1/comm")
                .map_err(|e| e.to_string())?
                .trim()
                .into(),
            package_manager: if os_value(&os, "ID") == Some("arch") {
                "pacman"
            } else {
                "unknown"
            }
            .into(),
            user: output("id", &["-un"])?,
        })
    }

    fn install(&self, package: &PackageName) -> Result<()> {
        let os = fs::read_to_string("/etc/os-release").map_err(|e| e.to_string())?;
        if os_value(&os, "ID") != Some("arch") {
            return Err("Package installation currently supports Arch Linux only".into());
        }
        if output("id", &["-u"])? != "0" {
            return Err("Package installation requires root: sudo mono install <package>".into());
        }
        let result = Command::new("/usr/bin/pacman")
            .args(["-S", "--needed", "--", package.as_str()])
            .status()
            .map_err(|e| format!("Cannot start pacman: {e}"))?;
        if result.success() {
            Ok(())
        } else {
            Err(format!("pacman failed: {result}"))
        }
    }

    fn network(&self) -> Result<NetworkStatus> {
        Ok(NetworkStatus {
            addresses: output("ip", &["-brief", "address"])?,
            routes: format!(
                "IPv4:\n{}\nIPv6:\n{}",
                output("ip", &["-4", "route"])?,
                output("ip", &["-6", "route"])?
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_platform_from_kernel() {
        assert_eq!(platform("6.18.33.2-microsoft-standard-WSL2"), "WSL2");
        assert_eq!(platform("4.4.0-Microsoft"), "WSL");
        assert_eq!(platform("6.12.1-arch1-1"), "Linux");
    }
}
