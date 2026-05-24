use std::io::IsTerminal;

mod checks;
mod ui;

#[tokio::main]
async fn main() {
    if std::io::stderr().is_terminal() {
        colored::control::set_override(true);
    }

    // First positional argument selects the target tool.
    // Defaults to Claude when invoked without arguments (backward compat).
    let target: &'static checks::Target = match std::env::args().nth(1).as_deref() {
        Some("codex") => &checks::CODEX,
        _ => &checks::CLAUDE,
    };

    eprintln!();
    eprintln!("  Verifying network before {} starts...", target.tool_name);
    eprintln!();

    loop {
        eprintln!("  Checking...");
        eprintln!();
        let results = checks::run_all(target).await;
        let status = ui::render(&results);

        match status {
            ui::Status::Green => std::process::exit(0),
            _ => match ui::prompt() {
                ui::Choice::Continue => std::process::exit(0),
                ui::Choice::Retry => eprintln!(),
                ui::Choice::Quit => {
                    eprintln!("  Aborted.");
                    std::process::exit(1);
                }
            },
        }
    }
}
