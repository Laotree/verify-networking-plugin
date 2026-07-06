use crate::checks::{self, Status as CheckStatus};
use crate::ui::{self, Choice};
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};

const DEFAULT_PORT: u16 = 8443;
/// Target hosts this proxy intercepts and checks before proxying.
const MONITORED_HOSTS: &[&str] = &["api.anthropic.com", "api.openai.com"];

/// Per-session block flag: once the user chooses "Block this session",
/// further connections to the same target are denied without prompting.
struct SessionState {
    blocked_hosts: Vec<String>,
}

/// Run the daemon: listen on localhost:PORT as an HTTP CONNECT proxy.
pub async fn run_daemon(port: Option<u16>) -> Result<(), Box<dyn std::error::Error>> {
    let port = port.unwrap_or(DEFAULT_PORT);
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await?;

    // Determine which target this daemon primarily serves (for UI messages).
    // When run without args, default to Claude; user can pass `--target codex`.
    let target: &'static checks::Target = if std::env::args().any(|a| a == "codex") {
        &checks::CODEX
    } else {
        &checks::CLAUDE
    };

    eprintln!();
    eprintln!("  🌐 Network check daemon started");
    eprintln!("  Listening on {}", addr);
    eprintln!("  Monitoring: {}", MONITORED_HOSTS.join(", "));
    eprintln!();
    eprintln!("  Configure your tools to use this HTTP proxy:");
    eprintln!("    export https_proxy=http://127.0.0.1:{}", port);
    eprintln!("    export all_proxy=http://127.0.0.1:{}", port);
    eprintln!("    claude --proxy http://127.0.0.1:{} ...", port);
    eprintln!();
    eprintln!("  Press Ctrl+C to stop the daemon");
    eprintln!();

    let session = Arc::new(tokio::sync::Mutex::new(SessionState {
        blocked_hosts: Vec::new(),
    }));

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let session = Arc::clone(&session);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, peer_addr, target, session).await {
                eprintln!("  Connection error from {}: {}", peer_addr, e);
            }
        });
    }
}

/// Handle a single incoming connection (CONNECT request).
async fn handle_connection(
    mut stream: TcpStream,
    peer_addr: std::net::SocketAddr,
    target: &'static checks::Target,
    session: Arc<tokio::sync::Mutex<SessionState>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Read the CONNECT request (first line + headers)
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse "CONNECT host:port HTTP/1.1"
    let request_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 || parts[0].to_uppercase() != "CONNECT" {
        // Not a CONNECT request — reject
        let _ = stream
            .write_all(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n")
            .await;
        return Ok(());
    }

    let host_port = parts[1];
    let target_host = host_port.split(':').next().unwrap_or(host_port);

    // Check if this host is monitored
    let is_monitored = MONITORED_HOSTS
        .iter()
        .any(|h| target_host.eq_ignore_ascii_case(h));

    if is_monitored {
        // Check session block list
        {
            let session = session.lock().await;
            if session
                .blocked_hosts
                .iter()
                .any(|h| h.eq_ignore_ascii_case(target_host))
            {
                // Session blocked — deny connection
                eprintln!(
                    "  🔒 Blocked connection from {} to {} (session blocked)",
                    peer_addr, target_host
                );
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\n\r\nNetwork check blocked this connection. Restart the daemon to unblock.\r\n",
                    )
                    .await;
                return Ok(());
            }
        }

        // Run network checks
        eprintln!(
            "  🔍 Checking network for connection from {} to {}...",
            peer_addr, target_host
        );

        let results = checks::run_all(target).await;
        let status = ui::overall_status(&results);
        // Render results inline
        for r in &results {
            let icon = match r.status {
                CheckStatus::Ok => "🟢",
                CheckStatus::Warn => "🟡",
                CheckStatus::Fail => "🔴",
            };
            eprintln!("    {} {} {}", icon, r.name, r.detail);
        }

        match status {
            ui::Status::Green => {
                // All good — proxy without prompting
                eprintln!("    ✅ Network OK — proxying connection");
                send_ok_and_proxy(stream, host_port, target_host).await?;
            }
            _ => {
                // Risk detected — hold and prompt
                let _exit_ip = results
                    .iter()
                    .find(|r| r.name == "Exit IP")
                    .and_then(|r| r.detail.split_whitespace().next())
                    .map(|s| s.to_string());

                eprintln!();
                let choice = prompt_daemon(peer_addr, target_host);

                match choice {
                    Choice::Continue => {
                        eprintln!("    → User confirmed — proxying connection");
                        send_ok_and_proxy(stream, host_port, target_host).await?;
                    }
                    Choice::Retry => {
                        eprintln!("    → Re-checking...");
                        // Re-run checks recursively (simple approach: drop and let client retry)
                        let _ = stream
                            .write_all(
                                b"HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\n\r\nRetry requested. Please try again.\r\n",
                            )
                            .await;
                        // Client will retry and we'll check again
                    }
                    Choice::Quit => {
                        eprintln!("    → User quit");
                        let _ = stream
                            .write_all(
                                b"HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\n\r\nConnection blocked by network check.\r\n",
                            )
                            .await;
                    }
                    Choice::BlockSession => {
                        eprintln!(
                            "    → Blocking {} for this session",
                            target_host
                        );
                        {
                            let mut session = session.lock().await;
                            if !session
                                .blocked_hosts
                                .iter()
                                .any(|h| h.eq_ignore_ascii_case(target_host))
                            {
                                session.blocked_hosts.push(target_host.to_string());
                            }
                        }
                        let _ = stream
                            .write_all(
                                b"HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\n\r\nConnection blocked by network check.\r\n",
                            )
                            .await;
                    }
                }
            }
        }
    } else {
        // Not a monitored host — proxy without checking
        send_ok_and_proxy(stream, host_port, target_host).await?;
    }

    Ok(())
}

/// Send HTTP 200 Connection Established and start bidirectional proxy.
async fn send_ok_and_proxy(
    mut client: TcpStream,
    host_port: &str,
    _target_host: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Connect to the real target
    let server = match timeout(Duration::from_secs(10), TcpStream::connect(host_port)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            let _ = client
                .write_all(format!("HTTP/1.1 502 Bad Gateway\r\n\r\nCannot connect to {}: {}\r\n", host_port, e).as_bytes())
                .await;
            return Err(format!("Cannot connect to {}: {}", host_port, e).into());
        }
        Err(_) => {
            let _ = client
                .write_all(format!("HTTP/1.1 504 Gateway Timeout\r\n\r\nTimeout connecting to {}\r\n", host_port).as_bytes())
                .await;
            return Err(format!("Timeout connecting to {}", host_port).into());
        }
    };

    // Send 200 to client
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;

    // Bidirectional copy
    let (mut cr, mut cw) = tokio::io::split(client);
    let (mut sr, mut sw) = tokio::io::split(server);
    let client_to_server = tokio::io::copy(&mut cr, &mut sw);
    let server_to_client = tokio::io::copy(&mut sr, &mut cw);

    // Wait for one direction to finish (connection closed)
    tokio::select! {
        r = client_to_server => { r?; }
        r = server_to_client => { r?; }
    }

    Ok(())
}

/// Interactive prompt shown when risk is detected during daemon mode.
/// Reads from /dev/tty so it works even when stdin is piped.
fn prompt_daemon(peer_addr: std::net::SocketAddr, target_host: &str) -> Choice {
    eprintln!(
        "  ⚠️  Network risk detected — connection from {} to {} is held",
        peer_addr, target_host
    );
    eprintln!();
    eprint!("  [C]ontinue  [R]etry  [B]lock session  [Q]uit › ");
    io::stderr().flush().ok();

    let tty = match std::fs::File::open("/dev/tty") {
        Ok(f) => f,
        Err(_) => {
            // No TTY available — abort by default
            eprintln!("  (no TTY available — aborting)");
            return Choice::Quit;
        }
    };
    let mut reader = io::BufReader::new(tty);
    let mut line = String::new();
    reader.read_line(&mut line).ok();

    match line.trim().to_ascii_lowercase().as_str() {
        "c" | "continue" => Choice::Continue,
        "r" | "retry" => Choice::Retry,
        "b" | "block" | "block session" => Choice::BlockSession,
        _ => Choice::Quit,
    }
}
