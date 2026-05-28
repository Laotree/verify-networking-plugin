use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

const TRACE_TIMEOUT: Duration = Duration::from_secs(30);

/// Run mtr (preferred) or traceroute toward `host`.
/// Returns the tool name and its output, or None if neither is available.
pub async fn run_trace(host: &str) -> Option<(&'static str, String)> {
    if let Some(out) = try_run("mtr", &["--report", "--no-dns", "-c", "3", host]).await {
        return Some(("mtr", out));
    }
    if let Some(out) = try_run("traceroute", &["-n", host]).await {
        return Some(("traceroute", out));
    }
    None
}

async fn try_run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = Command::new(cmd);
    c.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    let child = match c.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return None,
    };

    match timeout(TRACE_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(out)) => {
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            if text.trim().is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}
