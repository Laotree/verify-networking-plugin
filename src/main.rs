use std::io::IsTerminal;

mod checks;
mod proxy;
mod trace;
mod ui;

#[tokio::main]
async fn main() {
    if std::io::stderr().is_terminal() {
        colored::control::set_override(true);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();

    // --daemon: run as TCP CONNECT proxy for per-call network checks
    if args.iter().any(|a| a == "--daemon" || a == "-d") {
        let port = args
            .iter()
            .position(|a| a == "--port" || a == "-p")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<u16>().ok());
        if let Err(e) = proxy::run_daemon(port).await {
            eprintln!("Daemon error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // --non-interactive: plain-text output, no /dev/tty prompt, structured exit codes.
    // Used by the Codex desktop app wrapper.
    let non_interactive = args.iter().any(|a| a == "--non-interactive");

    // First positional arg (non-flag) selects the target tool.
    // Defaults to Claude when invoked without arguments (backward compat).
    let target: &'static checks::Target = if args.iter().any(|a| a == "codex") {
        &checks::CODEX
    } else {
        &checks::CLAUDE
    };

    if !non_interactive {
        eprintln!();
        eprintln!("  Verifying network before {} starts...", target.tool_name);
        eprintln!();
    }

    loop {
        if !non_interactive {
            eprintln!("  Checking...");
            eprintln!();
        }

        let results = checks::run_all(target).await;

        if non_interactive {
            // Plain render, no prompt. Exit codes: 0=green, 2=yellow, 1=red.
            let status = ui::render_plain(&results);
            match status {
                ui::Status::Green => std::process::exit(0),
                ui::Status::Yellow => std::process::exit(2),
                ui::Status::Red => std::process::exit(1),
            }
        }

        let status = ui::render(&results);
        match status {
            ui::Status::Green => std::process::exit(0),
            _ => {
                let exit_ip = results
                    .iter()
                    .find(|r| r.name == "Exit IP")
                    .and_then(|r| r.detail.split_whitespace().next())
                    .map(|s| s.to_string());

                let trace = ui::run_with_spinner(
                    "Running traceroute for a more precise path analysis — this may take ~30 s",
                    trace::run_trace(target.host),
                )
                .await;

                if let Some((tool, output)) = trace {
                    ui::print_trace(&tool, target.host, &output);
                    if let Some(ref ip) = exit_ip {
                        ui::print_exit_ip_warning(ip, &output);
                    }
                }
                match ui::prompt() {
                    ui::Choice::Continue => std::process::exit(0),
                    ui::Choice::Retry => eprintln!(),
                    ui::Choice::BlockSession => std::process::exit(0),
                    ui::Choice::Quit => {
                        eprintln!("  Aborted.");
                        std::process::exit(1);
                    }
                }
            }
        }
    }
}
