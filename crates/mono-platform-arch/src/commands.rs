use mono_core::{Result, capabilities::PackageName};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use mono_core::command::Plan;
fn safe(value: &str) -> Result<&str> {
    if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        Err("Empty values, leading '-' and control characters are not accepted here".into())
    } else {
        Ok(value)
    }
}
fn path(value: &str) -> Result<String> {
    if value.is_empty() {
        return Err("Empty path".into());
    }
    let value = if let Some(rest) = value.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?).join(rest)
    } else {
        PathBuf::from(value)
    };
    let value = if value.is_absolute() {
        value
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(value)
    };
    value
        .into_os_string()
        .into_string()
        .map_err(|_| "Path is not UTF-8".into())
}
fn username(value: &str) -> Result<&str> {
    if value.is_empty()
        || value.len() > 32
        || !value.bytes().enumerate().all(|(i, c)| {
            c.is_ascii_lowercase() || c == b'_' || (i > 0 && (c.is_ascii_digit() || c == b'-'))
        })
    {
        return Err("Expected a Linux username (lowercase letters, digits, '_' or '-')".into());
    }
    Ok(value)
}
fn unit(value: &str) -> Result<String> {
    safe(value)?;
    if !value
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b"_.@:-".contains(&c))
    {
        return Err("Invalid service name".into());
    }
    Ok(if value.contains('.') {
        value.to_owned()
    } else {
        format!("{value}.service")
    })
}
fn is_service(value: &str) -> Result<bool> {
    let u = unit(value)?;
    let result = Command::new("/usr/bin/systemctl")
        .args(["show", "--property=LoadState", "--value", "--", &u])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(result.status.success() && String::from_utf8_lossy(&result.stdout).trim() == "loaded")
}
fn device(value: &str) -> Result<String> {
    let p = fs::canonicalize(value).map_err(|e| format!("Device: {e}"))?;
    use std::os::unix::fs::FileTypeExt;
    if !p.starts_with("/dev")
        || !fs::metadata(&p)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_block_device()
    {
        return Err("Expected a block device under /dev".into());
    }
    Ok(p.to_string_lossy().into_owned())
}
fn protected_delete(value: &str) -> Result<()> {
    let p = Path::new(value);
    // Resolve the parent, but preserve a final symlink so deleting a link never follows it.
    let parent =
        fs::canonicalize(p.parent().ok_or("Cannot delete root")?).map_err(|e| e.to_string())?;
    let target = parent.join(p.file_name().ok_or("Invalid deletion target")?);
    let forbidden = [
        "/", "/bin", "/boot", "/dev", "/etc", "/home", "/lib", "/lib64", "/opt", "/proc", "/root",
        "/run", "/sbin", "/sys", "/tmp", "/usr", "/var",
    ];
    if forbidden.iter().any(|f| target == Path::new(f))
        || target.starts_with("/proc")
        || target.starts_with("/sys")
        || target.starts_with("/dev")
    {
        return Err("Refusing deletion of a protected system path".into());
    }
    let cwd = fs::canonicalize(std::env::current_dir().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if cwd.starts_with(&target) {
        return Err("Refusing deletion of the current directory or its ancestors".into());
    }
    if std::env::var_os("HOME")
        .and_then(|h| fs::canonicalize(h).ok())
        .as_ref()
        == Some(&target)
    {
        return Err("Refusing deletion of HOME".into());
    }
    Ok(())
}

pub fn plan(args: &[&str]) -> Result<Plan> {
    let mut p = Plan::default();
    if args.first() == Some(&"delete") && args.get(1) != Some(&"user") {
        return delete_plan(&args[1..]);
    }
    match args {
        ["status", "network"] => p.add("ip", &["-brief", "address"]),
        ["status", name] => p.add("systemctl", &["--no-pager", "status", "--", &unit(name)?]),
        ["info", value] => {
            if Path::new(&path(value)?).exists() || value.contains('/') {
                if value.starts_with("/dev/") {
                    p.add("lsblk", &["-O", "--", &device(value)?]);
                } else {
                    p.add("stat", &["--", &path(value)?]);
                }
            } else {
                PackageName::new(value)?;
                p.add("pacman", &["-Si", "--", value]);
            }
        }
        ["list", "apps"] => p.add("pacman", &["-Q"]),
        ["list", "disks"] | ["disks"] => {
            p.add("lsblk", &["-o", "NAME,TYPE,SIZE,FSTYPE,LABEL,MOUNTPOINTS"])
        }
        ["list", "services"] => p.add(
            "systemctl",
            &["--no-pager", "list-unit-files", "--type=service"],
        ),
        ["create", "file", name] => {
            let name = path(name)?;
            if Path::new(&name).symlink_metadata().is_ok() {
                return Err("Target already exists".into());
            }
            p.add("unused", &[&name]);
            p.steps[0].program = "@create-file".into();
        }
        ["create", "dir", name] => p.add("mkdir", &["--", &path(name)?]),
        ["create", "user", name] => {
            p.add("useradd", &["-m", "--", username(name)?]);
            p.confirm = true;
            p.notes.push(
                "Creates a user with a locked password. Set it separately with passwd <user>."
                    .into(),
            );
        }
        ["delete", "user", name] => {
            username(name)?;
            let uid = super::output("/usr/bin/id", &["-u", name])?
                .parse::<u32>()
                .map_err(|e| e.to_string())?;
            if uid < 1000 || uid == 65534 {
                return Err("Refusing deletion of a system user".into());
            }
            p.add("userdel", &["--", name]);
            p.confirm = true;
            p.notes.push("Deletes the account; keeps its home directory. Active users are rejected by userdel.".into());
        }
        ["copy", from, to] => p.add("cp", &["-a", "-i", "--", &path(from)?, &path(to)?]),
        ["move", from, to] => p.add("mv", &["-i", "--", &path(from)?, &path(to)?]),
        ["find", pattern] => p.add("find", &[&path(".")?, "-name", pattern, "-print"]),
        ["find", pattern, root] => p.add("find", &[&path(root)?, "-name", pattern, "-print"]),
        ["read", name] => p.add("cat", &["--", &path(name)?]),
        ["edit", name] => {
            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .unwrap_or("micro".into());
            if editor.split_whitespace().count() != 1 {
                return Err("EDITOR/VISUAL must be one executable path without arguments".into());
            }
            p.add("micro", &[&path(name)?]);
            p.steps[0].program = if editor.contains('/') {
                editor
            } else {
                format!("/usr/bin/{editor}")
            };
        }
        ["open", value] => {
            safe(value)?;
            if Path::new(&path(value)?).exists() || value.contains('/') || value.contains("://") {
                let target = if value.contains("://") {
                    value.to_string()
                } else {
                    path(value)?
                };
                p.add("xdg-open", &[&target]);
            } else {
                launch(&mut p, value)?;
            }
        }
        ["permissions", name] => p.add("stat", &["-c", "%A %a %n", "--", &path(name)?]),
        ["permissions", name, mode] => {
            let mode = if *mode == "executable" { "u+x" } else { mode };
            if mode != "u+x"
                && !(mode.len() >= 3
                    && mode.len() <= 4
                    && mode.bytes().all(|c| (b'0'..=b'7').contains(&c)))
            {
                return Err("Mode must be executable or 3/4 octal digits".into());
            }
            p.add("chmod", &[mode, "--", &path(name)?]);
            p.confirm = true;
        }
        ["owner", name] => p.add("stat", &["-c", "%U:%G (%u:%g) %n", "--", &path(name)?]),
        ["owner", name, owner] => {
            p.add("chown", &[username(owner)?, "--", &path(name)?]);
            p.confirm = true;
        }
        ["search", query] => p.add("pacman", &["-Ss", "--", query]),
        ["install", name] => {
            PackageName::new(name)?;
            p.add("pacman", &["-S", "--needed", "--", name]);
        }
        ["uninstall", name] => {
            PackageName::new(name)?;
            p.add("pacman", &["-R", "--", name]);
        }
        ["update"] => p.add("pacman", &["-Syu"]),
        ["update", name] => {
            PackageName::new(name)?;
            p.add("pacman", &["-Syu", "--", name]);
            p.notes.push(
                "Arch: full system upgrade plus this package; partial upgrades are not performed."
                    .into(),
            );
        }
        ["clean"] | ["clean", "packages"] => {
            p.add("pacman", &["-Sc"]);
            p.notes
                .push("Clean package cache using pacman's interactive confirmation.".into());
        }
        [verb @ ("start" | "stop" | "restart"), name] => {
            safe(name)?;
            if is_service(name)? {
                p.add("systemctl", &[verb, "--", &unit(name)?]);
            } else {
                if *verb == "restart" {
                    return Err(
                        "Application restart is not supported; specify a systemd service".into(),
                    );
                }
                if *verb != "start" {
                    p.add("pkill", &["-TERM", "-x", "--", &process_pattern(name)?]);
                }
                if *verb != "stop" {
                    launch(&mut p, name)?;
                }
            }
            p.confirm = *verb != "start";
        }
        [verb @ ("enable" | "disable"), name] => {
            p.add("systemctl", &[verb, "--", &unit(name)?]);
            p.confirm = true;
        }
        ["kill", value] => {
            safe(value)?;
            if value.bytes().all(|c| c.is_ascii_digit()) {
                let pid = value.parse::<u32>().map_err(|e| e.to_string())?;
                if pid <= 1 || pid == std::process::id() {
                    return Err("Refusing this PID".into());
                }
                p.add("kill", &["-KILL", "--", value]);
            } else {
                p.add("pkill", &["-KILL", "-x", "--", &process_pattern(value)?]);
            }
            p.confirm = true;
        }
        ["usage"] => {
            p.add("free", &["-h"]);
            p.add("ps", &["-eo", "pid,comm,%cpu,%mem,rss", "--sort=-rss"]);
        }
        ["usage", name] => {
            if Path::new(&path(name)?).exists() {
                p.add("df", &["-h", "--", &path(name)?]);
            } else {
                p.add("ps", &["-C", safe(name)?, "-o", "pid,comm,%cpu,%mem,rss"]);
            }
        }
        ["check", "disk", name] => {
            p.add("smartctl", &["-H", &device(name)?]);
            p.notes.push("Read-only SMART health check, not a filesystem repair; some virtual/USB disks do not support SMART.".into());
        }
        ["check", "network"] => {
            p.add("ip", &["-brief", "address"]);
            p.add("ip", &["route", "get", "1.1.1.1"]);
            p.notes.push("Checks local addresses and route selection only; no packets sent. For DNS/TCP use mono network check <host>.".into());
        }
        ["logs"] => p.add("journalctl", &["--no-pager", "-n", "100"]),
        ["logs", "boot"] => p.add("journalctl", &["--no-pager", "-b", "-n", "100"]),
        ["logs", name] => p.add(
            "journalctl",
            &["--no-pager", "-u", &unit(name)?, "-n", "100"],
        ),
        ["scan", "wifi"] => p.add("nmcli", &["device", "wifi", "list", "--rescan", "yes"]),
        ["connect", "wifi", ssid] => {
            p.add("nmcli", &["radio", "wifi", "on"]);
            p.add(
                "nmcli",
                &["--ask", "device", "wifi", "connect", safe(ssid)?],
            );
            p.confirm = true;
        }
        ["disconnect", "wifi"] => {
            p.add("nmcli", &["radio", "wifi", "off"]);
            p.confirm = true;
            p.notes.push(
                "Turns Wi-Fi radio off for all adapters. Re-enable with mono connect wifi <SSID>."
                    .into(),
            );
        }
        ["scan", "bluetooth"] => p.add("bluetoothctl", &["--timeout", "10", "scan", "on"]),
        [verb @ ("connect" | "disconnect"), "bluetooth", name] => {
            let addr = bluetooth_address(name)?;
            p.add("bluetoothctl", &["--timeout", "20", verb, &addr]);
            p.confirm = true;
            p.notes.push(
                "Device must already be paired; use bluetoothctl for initial pairing.".into(),
            );
        }
        ["ping", host] => {
            safe(host)?;
            p.add("ping", &["-c", "4", "-W", "2", "--", host]);
        }
        ["network", "interfaces"] | ["ip"] => p.add("ip", &["-brief", "address"]),
        ["ip", iface] => p.add("ip", &["address", "show", "dev", safe(iface)?]),
        ["ip", iface, action @ ("add" | "delete"), cidr] => {
            let (ip, prefix) = cidr.split_once('/').ok_or("Expected IP/prefix")?;
            let ip = ip.parse::<std::net::IpAddr>().map_err(|e| e.to_string())?;
            let prefix = prefix.parse::<u8>().map_err(|e| e.to_string())?;
            if prefix > if ip.is_ipv4() { 32 } else { 128 } {
                return Err("Invalid prefix".into());
            }
            p.add("ip", &["address", action, cidr, "dev", safe(iface)?]);
            p.confirm = true;
            p.notes
                .push("Temporary address change; does not persist after reboot.".into());
        }
        ["dns"] => p.add("cat", &["/etc/resolv.conf"]),
        ["dns", iface, server] => {
            server
                .parse::<std::net::IpAddr>()
                .map_err(|e| e.to_string())?;
            p.add("resolvectl", &["dns", safe(iface)?, server]);
            p.confirm = true;
            p.notes
                .push("Runtime per-link DNS; requires systemd-resolved. Not persistent.".into());
        }
        ["ports"] => p.add("ss", &["-lntup"]),
        ["ports", port] => {
            let port = port.parse::<u16>().map_err(|e| e.to_string())?;
            if port == 0 {
                return Err("Invalid port".into());
            }
            p.add("ss", &["-lntup", &format!("sport = :{port}")]);
        }
        ["mount", name] | ["unmount", name] => {
            let verb = if args[0] == "mount" {
                "mount"
            } else {
                "unmount"
            };
            p.add("udisksctl", &[verb, "-b", &device(name)?]);
            p.confirm = true;
        }
        ["format", name, filesystem] => {
            let name = device(name)?;
            let program = match *filesystem {
                "ext4" => "mkfs.ext4",
                "vfat" | "fat32" => "mkfs.vfat",
                "exfat" => "mkfs.exfat",
                _ => return Err("Supported filesystems: ext4, vfat/fat32, exfat".into()),
            };
            p.add(program, &[&name]);
            p.confirm = true;
            p.format_device = Some(name);
        }
        [verb @ ("reboot" | "shutdown" | "sleep")] => {
            p.add(
                "systemctl",
                &[match *verb {
                    "shutdown" => "poweroff",
                    "sleep" => "suspend",
                    _ => "reboot",
                }],
            );
            p.confirm = true;
        }
        ["lock"] => {
            let session = std::env::var("XDG_SESSION_ID")
                .map_err(|_| "No graphical/login session: XDG_SESSION_ID is not set")?;
            p.add("loginctl", &["lock-session", safe(&session)?]);
            p.notes.push(
                "Requires a running session locker integrated with the desktop/compositor.".into(),
            );
        }
        _ => return Err("Invalid command or arguments. Run: mono help <command>".into()),
    }
    Ok(p)
}
fn process_pattern(value: &str) -> Result<String> {
    safe(value)?;
    if value.len() > 15 {
        return Err("Process names longer than 15 bytes require an explicit PID".into());
    }
    let mut result = String::new();
    for c in value.chars() {
        if ".^$*+?()[]{}|\\".contains(c) {
            result.push('\\');
        }
        result.push(c);
    }
    Ok(result)
}
fn launch(p: &mut Plan, app: &str) -> Result<()> {
    safe(app)?;
    if !app
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err("Expected an application executable name".into());
    }
    p.add(app, &[]);
    p.steps.last_mut().unwrap().detached = true;
    Ok(())
}
fn bluetooth_address(name: &str) -> Result<String> {
    safe(name)?;
    if name.len() == 17
        && name.split(':').count() == 6
        && name
            .split(':')
            .all(|s| s.len() == 2 && s.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Ok(name.into());
    }
    let devices = super::output("/usr/bin/bluetoothctl", &["devices"])?;
    let matches: Vec<_> = devices
        .lines()
        .filter_map(|l| l.strip_prefix("Device ")?.split_once(' '))
        .filter(|(_, n)| *n == name)
        .collect();
    if matches.len() != 1 {
        return Err("Bluetooth name not found or ambiguous. Use the MAC address shown by mono scan bluetooth.".into());
    }
    Ok(matches[0].0.into())
}

pub fn validate_format(name: &str) -> Result<()> {
    let name = device(name)?;
    let kind = super::output("/usr/bin/lsblk", &["-dnro", "TYPE", "--", &name])?;
    if kind != "part" {
        return Err("Formatting is restricted to partitions; whole disks, loop devices and mapped volumes are refused".into());
    }
    let mounts = super::output("/usr/bin/lsblk", &["-nro", "MOUNTPOINTS", "--", &name])?;
    if !mounts.trim().is_empty() {
        return Err("Partition or child device is mounted/in use (including swap)".into());
    }
    let swaps = fs::read_to_string("/proc/swaps").map_err(|e| e.to_string())?;
    for line in swaps.lines().skip(1) {
        if let Some(swap) = line.split_whitespace().next()
            && fs::canonicalize(swap).ok().as_deref() == Some(Path::new(&name))
        {
            return Err("Partition is active swap".into());
        }
    }
    let base = Path::new(&name).file_name().ok_or("Invalid device")?;
    let holders = Path::new("/sys/class/block").join(base).join("holders");
    if fs::read_dir(holders)
        .map_err(|e| e.to_string())?
        .next()
        .is_some()
    {
        return Err("Partition has active device-mapper/RAID holders".into());
    }
    Ok(())
}

fn delete_plan(args: &[&str]) -> Result<Plan> {
    let mut force = false;
    let mut permanent = false;
    let mut target = None;
    let mut positional = false;
    for arg in args {
        match *arg {
            "--" if !positional => positional = true,
            "--force" if !positional => force = true,
            "--permanent" if !positional => permanent = true,
            flag if !positional && flag.starts_with('-') => {
                return Err(format!(
                    "Неизвестный параметр: {flag}. См. mono help delete"
                ));
            }
            name => {
                if target.replace(name).is_some() {
                    return Err("Укажите один объект удаления".into());
                }
            }
        }
    }
    let raw = path(target.ok_or("Укажите объект: mono delete <target> [options]")?)?;
    let raw = Path::new(&raw);
    let parent = fs::canonicalize(raw.parent().ok_or("Удаление корня запрещено")?)
        .map_err(|e| e.to_string())?;
    let name = parent.join(raw.file_name().ok_or("Некорректный путь удаления")?);
    let name = name.to_str().ok_or("Путь не в UTF-8")?;
    protected_delete(name).map_err(|_| {
        "🔴 Удаление защищённого пути запрещено; --force не отменяет защиту".to_owned()
    })?;
    let metadata = fs::symlink_metadata(name).map_err(|e| format!("Объект недоступен: {e}"))?;
    if metadata.is_dir()
        && !force
        && fs::read_dir(name)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err("Каталог не пуст. Для удаления содержимого добавьте --force".into());
    }
    let system = [
        "/etc", "/usr", "/var", "/boot", "/run", "/opt", "/bin", "/sbin", "/lib", "/lib64",
    ]
    .iter()
    .any(|root| Path::new(name).starts_with(root));
    let risk = if system {
        "🟡 системный путь"
    } else {
        "🟢 пользовательский объект"
    };
    let mut p = Plan {
        confirm: true,
        description: Some(format!(
            "DELETE\nОбъект: {name}\nДействие: {}\nРиск: {risk}\nНепустой каталог: {}",
            if permanent {
                "удалить без корзины"
            } else {
                "переместить в корзину"
            },
            if force {
                "разрешён (--force)"
            } else {
                "не разрешён"
            }
        )),
        ..Plan::default()
    };
    let mut recheck = vec!["delete".to_owned(), name.to_owned()];
    if force {
        recheck.push("--force".into());
    }
    if permanent {
        recheck.push("--permanent".into());
    }
    p.delete_args = Some(recheck);
    if permanent {
        if metadata.is_dir() {
            if force {
                p.add("rm", &["-r", "--one-file-system", "--", name]);
            } else {
                p.add("rmdir", &["--", name]);
            }
        } else {
            p.add("rm", &["--", name]);
        }
    } else {
        p.add("gio", &["trash", "--", name]);
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_option_injection_is_rejected() {
        for cmd in ["install", "uninstall", "update"] {
            assert!(plan(&[cmd, "--root"]).is_err());
        }
    }
    #[test]
    fn shell_text_stays_one_argument() {
        let p = plan(&["search", "video editor; touch /tmp/evil"]).unwrap();
        assert_eq!(
            p.steps[0].args,
            ["-Ss", "--", "video editor; touch /tmp/evil"]
        );
    }
    #[test]
    fn arch_upgrade_is_full() {
        assert_eq!(
            plan(&["update", "firefox"]).unwrap().steps[0].args,
            ["-Syu", "--", "firefox"]
        );
    }
    #[test]
    fn destructive_plans_require_confirmation() {
        for cmd in ["reboot", "shutdown", "sleep"] {
            assert!(plan(&[cmd]).unwrap().confirm);
        }
        assert!(plan(&["kill", "4821"]).unwrap().confirm);
    }
    #[test]
    fn protected_paths_are_rejected() {
        for path in ["/", "/etc", "/usr", "/root", "/proc/1"] {
            assert!(plan(&["delete", path]).is_err());
        }
    }
    #[test]
    fn invalid_mutations_are_rejected() {
        for args in [
            vec!["kill", "0"],
            vec!["kill", "1"],
            vec!["permissions", "file", "999"],
            vec!["ip", "eth0", "add", "1.1.1.1/64"],
            vec!["dns", "eth0", "nonsense"],
        ] {
            assert!(plan(&args).is_err());
        }
    }
    #[test]
    fn process_names_are_literal() {
        assert_eq!(process_pattern("a.b").unwrap(), "a\\.b");
    }
}
