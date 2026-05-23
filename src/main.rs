use std::io::IsTerminal;

mod checks;
mod ui;

#[tokio::main]
async fn main() {
    if std::io::stderr().is_terminal() {
        colored::control::set_override(true);
    }

    eprintln!();
    eprintln!("  Verifying network before Claude starts...");
    eprintln!();

    loop {
        eprintln!("  Checking...");
        eprintln!();
        let results = checks::run_all().await;
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
