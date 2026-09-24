use mono_core::{Result, capabilities};
use mono_platform_arch::Arch;

fn run(args: &[String]) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["help" | "--help" | "-h"] => println!(
            "Monux {}\n\nCommands:\n  mono status\n  mono install <package>\n  mono network [status]\n  mono network check <host> [port]",
            env!("CARGO_PKG_VERSION")
        ),
        ["status"] => {
            let s = capabilities::status(&Arch)?;
            println!(
                "Monux {}\n\nPlatform:        {}\nOS:              {}\nArchitecture:    {}\nKernel:          {}\nInit:            {}\nPackage manager: {}\nUser:            {}",
                env!("CARGO_PKG_VERSION"),
                s.platform,
                s.os,
                s.architecture,
                s.kernel,
                s.init,
                s.package_manager,
                s.user
            );
        }
        ["install", package] => capabilities::install(&Arch, package)?,
        ["network"] | ["network", "status"] => {
            let n = capabilities::network(&Arch)?;
            println!("Addresses:\n{}\n\nRoutes:\n{}", n.addresses, n.routes);
        }
        ["network", "check", host] => check(host, 443)?,
        ["network", "check", host, port] => {
            let port = port.parse::<u16>().map_err(|_| "Port must be 1..65535")?;
            check(host, port)?;
        }
        _ => return Err("Usage: mono status | mono install <package> | mono network [status] | mono network check <host> [port]".into()),
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Monux: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn check(host: &str, port: u16) -> Result<()> {
    println!("Target: {host}, TCP port {port}");
    let result = capabilities::network_check(&Arch, host, port)?;
    println!(
        "{}: {}",
        if host.parse::<std::net::IpAddr>().is_ok() {
            "IP (DNS skipped)"
        } else {
            "DNS OK"
        },
        result
            .addresses
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    for failure in &result.failures {
        println!("TCP attempt: {failure}");
    }
    match result.connected {
        Some(address) => {
            println!("TCP OK: {address}\nTCP connection succeeded; TLS/HTTP not checked.");
            Ok(())
        }
        None => {
            Err("TCP check failed. This does not prove that the internet is unavailable.".into())
        }
    }
}
