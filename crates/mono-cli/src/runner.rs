use mono_core::Result;
use mono_core::command::Plan;
use mono_platform_arch::commands;
use std::{
    io::{self, IsTerminal, Write},
    process::{Command, Stdio},
};

pub fn execute(plan: Plan, dry_run: bool) -> Result<()> {
    if dry_run {
        plan.describe();
        return Ok(());
    }
    // Check every required tool before running any step.
    for step in &plan.steps {
        if step.program == "@create-file" {
            continue;
        }
        if !std::path::Path::new(&step.program).is_file() {
            if step.program == "/usr/bin/gio" {
                return Err("Корзина недоступна. Объект сохранён. Для удаления без корзины явно укажите --permanent.".into());
            }
            return Err(format!(
                "В окружении недоступна нужная возможность ({}). См. mono manual backend <command>.",
                step.program
            ));
        }
    }
    if let Some(device) = &plan.format_device {
        commands::validate_format(device)?;
    }
    if plan.confirm {
        plan.describe();
        if !io::stdin().is_terminal() {
            return Err(
                "Confirmation requires an interactive terminal. Use --dry-run to inspect the plan."
                    .into(),
            );
        }
        let expected = plan.format_device.as_deref().unwrap_or("yes");
        print!("Confirm by typing {expected:?}: ");
        io::stdout().flush().map_err(|e| e.to_string())?;
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .map_err(|e| e.to_string())?;
        if answer.trim() != expected {
            return Err("Cancelled; no changes made".into());
        }
    } else {
        for note in &plan.notes {
            println!("{note}");
        }
    }
    if let Some(device) = &plan.format_device {
        commands::validate_format(device)?;
    }
    if let Some(args) = &plan.delete_args {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        commands::plan(&args)?;
    }
    for step in plan.steps {
        if step.program == "@create-file" {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&step.args[0])
                .map_err(|e| e.to_string())?;
            continue;
        }
        let mut command = Command::new(&step.program);
        command.args(&step.args);
        if step.detached {
            let child = command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            println!(
                "Started PID {} (application readiness not checked)",
                child.id()
            );
        } else {
            let status = command
                .status()
                .map_err(|e| format!("{}: {e}", step.program))?;
            if !status.success() {
                if step.program == "/usr/bin/gio" {
                    return Err("Не удалось переместить объект в корзину. Окончательное удаление не выполнялось.".into());
                }
                return Err(format!("{} returned {status}", step.program));
            }
        }
    }
    Ok(())
}
