use crate::checks::{CheckResult, Status as CheckStatus};
use colored::Colorize;
use std::io::{self, BufRead, Write};

pub enum Status {
    Green,
    Yellow,
    Red,
}

pub enum Choice {
    Continue,
    Retry,
    Quit,
}

pub fn render(results: &[CheckResult]) -> Status {
    // Determine overall status first so we can show the right summary
    let overall = overall_status(results);

    for r in results {
        let icon = match r.status {
            CheckStatus::Ok => "🟢",
            CheckStatus::Warn => "🟡",
            CheckStatus::Fail => "🔴",
        };
        // Pad name before colorizing so ANSI codes don't break alignment
        let name = format!("{:<14}", r.name);
        let detail = match r.status {
            CheckStatus::Ok => r.detail.green().to_string(),
            CheckStatus::Warn => r.detail.yellow().to_string(),
            CheckStatus::Fail => r.detail.red().bold().to_string(),
        };
        eprintln!("  {} {} {}", icon, name, detail);
    }

    eprintln!();
    match overall {
        Status::Green => eprintln!("  🟢 {}", "All checks passed.".green().bold()),
        Status::Yellow => {
            eprintln!("  🟡 {}", "Network concerns detected.".yellow().bold())
        }
        Status::Red => eprintln!("  🔴 {}", "Network issues detected.".red().bold()),
    }
    eprintln!();

    overall
}

pub fn prompt() -> Choice {
    eprint!("  [C]ontinue  [R]etry  [Q]uit › ");
    io::stderr().flush().ok();

    // stdin is piped (hook JSON), so read interactive input from the controlling terminal
    let tty = std::fs::File::open("/dev/tty")
        .expect("cannot open /dev/tty — interactive prompt unavailable");
    let mut reader = io::BufReader::new(tty);
    let mut line = String::new();
    reader.read_line(&mut line).ok();

    match line.trim().to_ascii_lowercase().as_str() {
        "c" | "continue" => Choice::Continue,
        "r" | "retry" => Choice::Retry,
        _ => Choice::Quit,
    }
}

fn overall_status(results: &[CheckResult]) -> Status {
    let has_fail = results.iter().any(|r| r.status == CheckStatus::Fail);
    let has_warn = results.iter().any(|r| r.status == CheckStatus::Warn);
    if has_fail {
        Status::Red
    } else if has_warn {
        Status::Yellow
    } else {
        Status::Green
    }
}
