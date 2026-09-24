use mono_core::{
    Result,
    diagnostics::{Check, Health, SECTIONS, percentage_health},
};
use std::{
    fs,
    io::Read,
    net::{SocketAddr, TcpStream},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Output {
    success: bool,
    text: String,
}
// Fixed commands only. Deadline covers the child; stdout is drained concurrently and capped.
fn bounded(program: &str, args: &[&str], timeout: Duration) -> Result<Output> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("LC_ALL", "C")
        .spawn()
        .map_err(|e| format!("Check unavailable: {e}"))?;
    let stdout = child.stdout.take().ok_or("Cannot capture check output")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = stdout;
        let mut data = Vec::new();
        let mut buf = [0; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let keep = n.min(262144_usize.saturating_sub(data.len()));
                    data.extend_from_slice(&buf[..keep]);
                }
            }
        }
        let _ = tx.send(String::from_utf8_lossy(&data).into_owned());
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let text = rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|_| "Check output timed out")?;
                return Ok(Output {
                    success: status.success(),
                    text,
                });
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                // A child stuck in kernel I/O must not block the diagnostics report.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Err("Check timed out or could not complete".into());
            }
        }
    }
}
fn command(program: &str, args: &[&str]) -> Result<String> {
    let o = bounded(program, args, Duration::from_secs(2))?;
    if !o.success {
        return Err("Check unavailable or access denied".into());
    }
    Ok(o.text)
}
fn read(path: &str) -> Result<String> {
    fs::read_to_string(path).map_err(|e| format!("Data unavailable: {e}"))
}
fn unknown(name: &'static str, error: impl Into<String>) -> Check {
    Check::new(name, Health::Unknown, error)
}
pub fn diagnose(section: &str) -> Result<Vec<Check>> {
    if !SECTIONS.contains(&section) {
        return Err(format!(
            "Unknown diagnostic section: {section}. Run mono help diagnose"
        ));
    }
    let checks: &[&str] = if section == "system" {
        SECTIONS
    } else {
        std::slice::from_ref(&section)
    };
    Ok(checks
        .iter()
        .map(|s| match *s {
            "system" => system(),
            "cpu" => cpu(),
            "memory" => memory(),
            "storage" => storage(),
            "filesystems" => filesystems(),
            "temperature" => temperature(),
            "network" => network(),
            "services" => services(),
            "boot" => boot(),
            "hardware" => hardware(),
            "power" => power(),
            "logs" => logs(),
            _ => unreachable!(),
        })
        .collect())
}
fn system() -> Check {
    match (read("/proc/sys/kernel/osrelease"), read("/proc/uptime")) {
        (Ok(kernel), Ok(uptime)) => Check::new(
            "System",
            Health::Ok,
            format!(
                "Kernel: {}; uptime: {} seconds. Basic system data accessible.",
                kernel.trim(),
                uptime.split_whitespace().next().unwrap_or("unknown")
            ),
        ),
        _ => unknown("System", "Cannot read basic kernel/uptime data"),
    }
}
fn cpu() -> Check {
    let result = (|| -> Result<Check> {
        let load = read("/proc/loadavg")?;
        let one = load
            .split_whitespace()
            .next()
            .ok_or("Missing load average")?
            .parse::<f64>()
            .map_err(|e| e.to_string())?;
        let cores = read("/proc/stat")?
            .lines()
            .filter(|l| {
                l.strip_prefix("cpu")
                    .is_some_and(|s| s.starts_with(|c: char| c.is_ascii_digit()))
            })
            .count();
        if cores == 0 {
            return Err("CPU count unavailable".into());
        }
        Ok(Check::new(
            "CPU",
            if one / cores as f64 >= 1.5 {
                Health::Warning
            } else {
                Health::Ok
            },
            format!(
                "1-minute load: {one:.2}; logical CPUs: {cores}. Load includes tasks waiting for I/O."
            ),
        ))
    })();
    result.unwrap_or_else(|e| unknown("CPU", e))
}
fn memory_values(data: &str) -> Result<(u64, u64)> {
    let field = |name: &str| {
        data.lines().find_map(|l| {
            l.strip_prefix(name)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
    };
    let total = field("MemTotal:").ok_or("MemTotal missing")?;
    let available = field("MemAvailable:").ok_or("MemAvailable missing")?;
    if total == 0 || available > total {
        return Err("Invalid memory counters".into());
    }
    Ok((total, available))
}
fn memory() -> Check {
    let result = (|| -> Result<Check> {
        let (total, available) = memory_values(&read("/proc/meminfo")?)?;
        let used = (total - available) as f64 / total as f64 * 100.;
        let mut c = Check::new(
            "Memory",
            percentage_health(used, 85., 95.),
            format!(
                "RAM usage: {used:.1}% ({:.2} / {:.2} GiB), based on available memory.",
                (total - available) as f64 / 1048576.,
                total as f64 / 1048576.
            ),
        );
        let mut largest = None;
        if let Ok(entries) = fs::read_dir("/proc") {
            for e in entries.flatten() {
                let pid = e.file_name().to_string_lossy().into_owned();
                if !pid.bytes().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                if let Ok(data) = fs::read_to_string(e.path().join("status")) {
                    let rss = data
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("VmRSS:")?
                                .split_whitespace()
                                .next()?
                                .parse::<u64>()
                                .ok()
                        })
                        .unwrap_or(0);
                    let name = data
                        .lines()
                        .find_map(|l| l.strip_prefix("Name:"))
                        .unwrap_or("unknown")
                        .trim()
                        .to_owned();
                    if largest.as_ref().is_none_or(|(max, _, _)| rss > *max) {
                        largest = Some((rss, name, pid));
                    }
                }
            }
        }
        if let Some((rss, name, pid)) = largest {
            c.details.push(format!("Largest visible process: {name} (PID {pid}) — {:.2} GiB RSS; shared memory is included.",rss as f64/1048576.));
        } else {
            c.details
                .push("Largest process unavailable (permissions or process exit).".into());
        }
        Ok(c)
    })();
    result.unwrap_or_else(|e| unknown("Memory", e))
}
fn storage_from(data: &str) -> Result<Check> {
    let mut problems = Vec::new();
    let mut count = 0;
    let mut health = Health::Ok;
    for line in data.lines().skip(1) {
        let cols: Vec<_> = line.split_whitespace().collect();
        if cols.len() < 6 {
            continue;
        }
        let Ok(percent) = cols[4].trim_end_matches('%').parse::<f64>() else {
            continue;
        };
        count += 1;
        let h = percentage_health(percent, 85., 95.);
        if h == Health::Error {
            health = Health::Error;
        } else if h == Health::Warning && health != Health::Error {
            health = Health::Warning;
        }
        if h.problem() {
            problems.push(format!("{}: {percent:.0}% used", cols[5..].join(" ")));
        }
    }
    if count == 0 {
        return Err("No filesystem capacity data".into());
    }
    let mut c = Check::new(
        "Storage",
        health,
        format!(
            "Checked capacity of {count} mounted filesystems (excluding temporary/read-only image types)."
        ),
    );
    c.details.extend(problems.into_iter().take(5));
    Ok(c)
}
fn storage() -> Check {
    command(
        "/usr/bin/df",
        &[
            "-P", "-B1", "-x", "tmpfs", "-x", "devtmpfs", "-x", "squashfs",
        ],
    )
    .and_then(|s| storage_from(&s))
    .unwrap_or_else(|e| unknown("Storage", e))
}
fn filesystems() -> Check {
    let result = (|| -> Result<Check> {
        let mounts = read("/proc/self/mountinfo")?;
        for line in mounts.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() > 5 && fields[4] == "/" {
                let ro = fields[5].split(',').any(|x| x == "ro");
                return Ok(Check::new(
                    "Filesystems",
                    if ro { Health::Warning } else { Health::Ok },
                    if ro {
                        "Root filesystem is read-only (may be intentional for a live system)."
                    } else {
                        "Root filesystem is mounted read-write. Integrity/fsck was not checked."
                    },
                ));
            }
        }
        Err("Root mount state unavailable".into())
    })();
    result.unwrap_or_else(|e| unknown("Filesystems", e))
}
fn temperature() -> Check {
    let mut values = Vec::new();
    for base in ["/sys/class/thermal", "/sys/class/hwmon"] {
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                if let Ok(files) = fs::read_dir(entry.path()) {
                    for file in files.flatten() {
                        let name = file.file_name().to_string_lossy().into_owned();
                        if (name == "temp"
                            || (name.starts_with("temp") && name.ends_with("_input")))
                            && let Ok(text) = fs::read_to_string(file.path())
                            && let Ok(value) = text.trim().parse::<f64>()
                            && value > 0.
                            && value <= 150000.
                        {
                            values.push(value / 1000.);
                        }
                    }
                }
            }
        }
    }
    let Some(max) = values.into_iter().reduce(f64::max) else {
        return Check::new(
            "Temperature",
            Health::Skipped,
            "No readable temperature sensors exposed by this environment.",
        );
    };
    Check::new(
        "Temperature",
        percentage_health(max, 85., 95.),
        format!(
            "Highest exposed sensor: {max:.1} °C. General thresholds: 85/95 °C; device-specific limits may differ."
        ),
    )
}
fn network_result(dns: Option<bool>, tcp: bool) -> Check {
    match (dns, tcp) {
        (Some(true), true) => Check::new(
            "Network",
            Health::Ok,
            "DNS resolved archlinux.org; at least one numeric TCP/443 probe succeeded. This is not a full internet/TLS test.",
        ),
        (Some(false), true) => Check::new(
            "Network",
            Health::Error,
            "DNS resolution of archlinux.org failed. Numeric TCP/443 connectivity is available; the tested IP connection works independently of DNS.",
        ),
        (Some(true), false) => Check::new(
            "Network",
            Health::Warning,
            "DNS resolved archlinux.org, but TCP/443 probes failed. A firewall or endpoint restriction is possible; an internet outage is not established.",
        ),
        (Some(false), false) => Check::new(
            "Network",
            Health::Error,
            "Both DNS and numeric TCP probes failed. The cause is undetermined; this alone does not prove a general internet outage.",
        ),
        (None, _) => unknown(
            "Network",
            format!(
                "DNS check could not run. Numeric TCP probe: {}.",
                if tcp { "reachable" } else { "not reachable" }
            ),
        ),
    }
}
fn network() -> Check {
    let dns = match bounded(
        "/usr/bin/getent",
        &["ahosts", "archlinux.org"],
        Duration::from_secs(2),
    ) {
        Ok(o) => Some(o.success && !o.text.trim().is_empty()),
        Err(e) if e.contains("timed out") => Some(false),
        _ => None,
    };
    let tcp = ["1.1.1.1:443", "9.9.9.9:443"].iter().any(|s| {
        let addr: SocketAddr = s.parse().expect("constant socket address");
        TcpStream::connect_timeout(&addr, Duration::from_millis(700)).is_ok()
    });
    network_result(dns, tcp)
}
fn services() -> Check {
    match command(
        "/usr/bin/systemctl",
        &[
            "--failed",
            "--type=service",
            "--no-legend",
            "--plain",
            "--no-pager",
        ],
    ) {
        Ok(text) => {
            let names: Vec<_> = text
                .lines()
                .filter_map(|l| l.split_whitespace().find(|s| s.ends_with(".service")))
                .collect();
            let mut c = Check::new(
                "Services",
                if names.is_empty() {
                    Health::Ok
                } else {
                    Health::Warning
                },
                format!("{} failed service(s).", names.len()),
            );
            c.details
                .extend(names.iter().take(5).map(|s| s.to_string()));
            if names.len() > 5 {
                c.details.push(format!("... and {} more", names.len() - 5));
            }
            c
        }
        Err(e) => unknown("Services", e),
    }
}
fn boot() -> Check {
    match command(
        "/usr/bin/systemctl",
        &["show", "--property=SystemState", "--value"],
    ) {
        Ok(text) => {
            let state = text.trim();
            let h = match state {
                "running" => Health::Ok,
                "degraded" | "starting" | "initializing" => Health::Warning,
                "maintenance" => Health::Error,
                _ => Health::Unknown,
            };
            Check::new(
                "Boot",
                h,
                format!("Current boot state: {state}. This does not audit previous boots."),
            )
        }
        Err(e) => unknown("Boot", e),
    }
}
fn journal(args: &[&str]) -> Result<usize> {
    let mut readable = false;
    for base in ["/run/log/journal", "/var/log/journal"] {
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                if fs::File::open(entry.path().join("system.journal")).is_ok() {
                    readable = true;
                }
            }
        }
    }
    if !readable {
        return Err("System journal is absent or unreadable; no healthy state inferred".into());
    }
    let text = command("/usr/bin/journalctl", args)?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    if trimmed.lines().any(|l| !l.starts_with('{')) {
        return Err("Journal output unavailable".into());
    }
    Ok(trimmed.lines().count())
}
fn hardware() -> Check {
    let kernel = read("/proc/sys/kernel/osrelease")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if kernel.contains("microsoft") {
        return Check::new(
            "Hardware",
            Health::Skipped,
            "WSL exposes virtual hardware; physical device health is not observable here.",
        );
    }
    // The kernel's error records are evidence, not a complete hardware self-test.
    match journal(&[
        "-k",
        "-b",
        "--priority=0..3",
        "--no-pager",
        "--quiet",
        "-o",
        "json",
        "-n",
        "21",
    ]) {
        Ok(n) if n > 0 => Check::new(
            "Hardware",
            Health::Warning,
            format!(
                "{} kernel error record(s) in this boot; these may be driver/software errors. Inspect mono logs boot.",
                if n >= 21 {
                    "at least 21".into()
                } else {
                    n.to_string()
                }
            ),
        ),
        Ok(_) => Check::new(
            "Hardware",
            Health::Ok,
            "No visible kernel error records in this boot. No physical self-test or SMART test performed.",
        ),
        Err(e) => unknown("Hardware", e),
    }
}
fn power() -> Check {
    let mut batteries = Vec::new();
    let mut unreadable = false;
    if let Ok(entries) = fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            if fs::read_to_string(entry.path().join("type"))
                .unwrap_or_default()
                .trim()
                != "Battery"
            {
                continue;
            }
            let capacity = fs::read_to_string(entry.path().join("capacity"))
                .ok()
                .and_then(|s| s.trim().parse::<u8>().ok());
            let status = fs::read_to_string(entry.path().join("status")).unwrap_or_default();
            if let Some(capacity) = capacity.filter(|n| *n <= 100) {
                batteries.push((capacity, status.trim().to_owned()));
            } else {
                unreadable = true;
            }
        }
    }
    if batteries.is_empty() {
        return Check::new(
            "Power",
            if unreadable {
                Health::Unknown
            } else {
                Health::Skipped
            },
            "No readable battery data; AC supply health is not measurable here.",
        );
    }
    let mut h = if unreadable {
        Health::Unknown
    } else {
        Health::Ok
    };
    let mut details = Vec::new();
    for (capacity, status) in batteries {
        if status == "Discharging" && capacity <= 10 {
            h = Health::Error;
        } else if status == "Discharging" && capacity <= 20 && h != Health::Error {
            h = Health::Warning;
        }
        if status.is_empty() && h == Health::Ok {
            h = Health::Unknown;
        }
        details.push(format!(
            "Battery: {capacity}%, state: {}",
            if status.is_empty() {
                "unknown"
            } else {
                &status
            }
        ));
    }
    Check {
        name: "Power",
        health: h,
        details,
    }
}
fn logs() -> Check {
    match journal(&[
        "-b",
        "--since=-15min",
        "--priority=0..3",
        "--no-pager",
        "--quiet",
        "-o",
        "json",
        "-n",
        "51",
    ]) {
        Ok(0) => Check::new(
            "Logs",
            Health::Ok,
            "No visible error-level records in the last 15 minutes of this boot. Journal access may be limited by permissions.",
        ),
        Ok(n) => Check::new(
            "Logs",
            Health::Warning,
            format!(
                "{} error-level record(s) in the last 15 minutes. Run mono logs. Records may share the same underlying cause.",
                if n >= 51 {
                    "at least 51".into()
                } else {
                    n.to_string()
                }
            ),
        ),
        Err(e) => unknown("Logs", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_uses_available_not_free() {
        assert_eq!(
            memory_values("MemTotal: 1000 kB\nMemFree: 10 kB\nMemAvailable: 600 kB\n").unwrap(),
            (1000, 600)
        );
    }
    #[test]
    fn invalid_memory_is_not_ok() {
        assert!(memory_values("MemTotal: 0 kB\nMemAvailable: 0 kB").is_err());
        assert!(memory_values("MemTotal: 10 kB").is_err());
    }
    #[test]
    fn storage_reports_full_mounts() {
        let c = storage_from(
            "Filesystem 1-blocks Used Available Capacity Mounted on\n/dev/sda 100 96 4 96% /\n",
        )
        .unwrap();
        assert_eq!(c.health, Health::Error);
        assert!(c.details[1].contains("96%"));
    }
    #[test]
    fn empty_storage_is_unknown() {
        assert!(storage_from("Filesystem capacity").is_err());
    }
    #[test]
    fn separates_dns_from_tcp() {
        assert_eq!(network_result(Some(false), true).health, Health::Error);
        assert!(network_result(Some(false), true).details[0].contains("available"));
        assert_eq!(network_result(Some(true), false).health, Health::Warning);
        assert_eq!(network_result(None, true).health, Health::Unknown);
    }
    #[test]
    fn missing_tool_is_an_error() {
        assert!(bounded("/nonexistent-monux-check", &[], Duration::from_millis(50)).is_err());
    }
    #[test]
    fn process_deadline_is_enforced() {
        let now = Instant::now();
        assert!(bounded("/usr/bin/sleep", &["2"], Duration::from_millis(30)).is_err());
        assert!(now.elapsed() < Duration::from_secs(1));
    }
}
