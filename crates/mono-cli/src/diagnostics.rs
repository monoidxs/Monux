use mono_core::{
    Result,
    diagnostics::{Health, SECTIONS},
};

pub fn run(section: &str, dry_run: bool) -> Result<()> {
    if !SECTIONS.contains(&section) {
        return Err(format!(
            "Unknown section: {section}. Run mono help diagnose"
        ));
    }
    if dry_run {
        println!("Diagnostics: {section}. Read-only checks; nothing executed.");
        if matches!(section, "system" | "network") {
            println!("Network probes: DNS archlinux.org; TCP/443 to 1.1.1.1 or 9.9.9.9.");
        }
        return Ok(());
    }
    let checks = mono_platform_arch::diagnostics::diagnose(section)?;
    println!("Monux System Diagnostics\n");
    for c in &checks {
        println!("{:<13} {}", c.name, c.health.label());
    }
    let problems = checks.iter().filter(|c| c.health.problem()).count();
    let unknown = checks
        .iter()
        .filter(|c| c.health == Health::Unknown)
        .count();
    let skipped = checks
        .iter()
        .filter(|c| c.health == Health::Skipped)
        .count();
    println!("\nProblems found: {problems}");
    if unknown + skipped > 0 {
        println!("Unavailable: {unknown}; skipped: {skipped}");
    }
    for c in &checks {
        if c.health != Health::Ok || section != "system" {
            let mark = match c.health {
                Health::Error => "X",
                Health::Warning => "!",
                Health::Ok => "+",
                _ => "?",
            };
            println!("\n[{mark}] {}", c.name);
            for line in &c.details {
                println!("    {line}");
            }
        }
    }
    if problems > 0 {
        println!("\nRun:");
        for c in checks.iter().filter(|c| c.health.problem()) {
            println!("    mono diagnose {}", c.name.to_ascii_lowercase());
        }
    }
    // Diagnostic findings are report data. Invocation failures still use the CLI error path.
    Ok(())
}
