mod runner;
use mono_core::{Result, capabilities};
use mono_platform_arch::Arch;

fn run(args: &[String]) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["help" | "--help" | "-h"] => help(None, false)?,
        ["help", command] => help(Some(command), false)?,
        ["manual"] => help(None, true)?,
        ["manual", command] => help(Some(command), true)?,
        ["manual", "backend", command] => backend_manual(command)?,
        ["version"] => println!("Mono / Monux {}", env!("CARGO_PKG_VERSION")),
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
        ["network"] | ["network", "status"] => {
            let n = capabilities::network(&Arch)?;
            println!("Addresses:\n{}\n\nRoutes:\n{}", n.addresses, n.routes);
        }
        ["network", "check", host] => check(host, 443)?,
        ["network", "check", host, port] => {
            let port = port.parse::<u16>().map_err(|_| "Port must be 1..65535")?;
            check(host, port)?;
        }
        _ => runner::execute(mono_platform_arch::commands::plan(&args)?, false)?,
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut dry_run = false;
    let mut after_separator = false;
    args.retain(|arg| {
        if arg == "--" {
            after_separator = true;
        }
        if !after_separator && arg == "--dry-run" {
            dry_run = true;
            false
        } else {
            true
        }
    });
    let result = if dry_run {
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        match borrowed.as_slice() {
            []
            | ["help" | "manual" | "version" | "status"]
            | ["help" | "manual", _]
            | ["manual", "backend", _]
            | ["network"]
            | ["network", "status"] => {
                println!("Read-only Mono command (not executed): {borrowed:?}");
                Ok(())
            }
            ["network", "check", host] => preview_network(host, "443"),
            ["network", "check", host, port] => preview_network(host, port),
            _ => {
                mono_platform_arch::commands::plan(&borrowed).and_then(|p| runner::execute(p, true))
            }
        }
    } else {
        run(&args)
    };
    match result {
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

fn help(command: Option<&str>, manual: bool) -> Result<()> {
    use mono_core::catalog::COMMANDS;
    if let Some(command) = command {
        let e = COMMANDS.iter().find(|e| e.name == command).ok_or_else(|| {
            format!("В Mono нет руководства для {command:?}. Список команд: mono help")
        })?;
        if manual {
            println!("{}\n\n{}\n\nПримеры:", e.name.to_uppercase(), e.details);
            for example in e.examples {
                println!("    {example}");
            }
            if command != "delete" {
                println!("\n--dry-run\n    Ничего не изменяет. Показывает будущие действия.");
            }
        } else {
            println!(
                "{} — {}\n\nUsage:\n  mono {} {}\n\nExamples:",
                e.name.to_uppercase(),
                e.summary,
                e.name,
                e.syntax
            );
            for example in e.examples {
                println!("  {example}");
            }
            println!("\nOptions:");
            if command == "delete" {
                println!(
                    "  --force       разрешить удаление непустого каталога\n  --permanent   удалить без корзины"
                );
            }
            println!(
                "  --dry-run     показать, что будет сделано\n\nMore:\n  mono manual {}",
                e.name
            );
        }
    } else {
        println!(
            "MONO — {}\n",
            if manual {
                "руководство"
            } else {
                "команды"
            }
        );
        if manual {
            println!(
                "Mono управляет файлами, программами, службами и устройствами.\nmono <команда> [аргументы]\n\nmono help <команда> — краткая справка\nmono manual <команда> — поведение и ограничения\nmono <команда> [аргументы] --dry-run — предпросмотр без изменений\n\nУдаление по умолчанию использует корзину. Опасные действия требуют подтверждения.\nДоступность отдельных функций зависит от оборудования, прав и окружения.\n"
            );
        }
        for e in COMMANDS {
            println!("  {:12} {}", e.name, e.summary);
        }
        println!(
            "\nПодробнее: mono {} <команда>",
            if manual { "manual" } else { "help" }
        );
    }
    Ok(())
}

fn backend_manual(command: &str) -> Result<()> {
    let e = mono_core::catalog::COMMANDS
        .iter()
        .find(|e| e.name == command)
        .ok_or_else(|| format!("Unknown Mono command: {command}"))?;
    println!("Arch Linux backend: {}\n\n{}", e.name, e.backend);
    Ok(())
}

fn preview_network(host: &str, port: &str) -> Result<()> {
    capabilities::validate_host(host)?;
    let port = port.parse::<u16>().map_err(|_| "Port must be 1..65535")?;
    if port == 0 {
        return Err("Port must be 1..65535".into());
    }
    println!("DNS resolution and TCP connection to {host}, port {port} (not executed)");
    Ok(())
}
