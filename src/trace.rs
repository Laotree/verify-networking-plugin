use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

const TRACE_TIMEOUT: Duration = Duration::from_secs(30);
const NALI_TIMEOUT: Duration = Duration::from_secs(5);

/// Run mtr (preferred) or traceroute toward `host`.
/// If nali is on PATH, pipe the output through it for IP geolocation annotations.
/// Returns the tool label and enriched output, or None if neither tracer is available.
pub async fn run_trace(host: &str) -> Option<(String, String)> {
    if let Some(out) = try_run("mtr", &["--report", "--no-dns", "-c", "3", host]).await {
        let (label, enriched) = enrich("mtr", out).await;
        return Some((label, enriched));
    }
    if let Some(out) = try_run("traceroute", &["-n", host]).await {
        let (label, enriched) = enrich("traceroute", out).await;
        return Some((label, enriched));
    }
    None
}

async fn enrich(base_tool: &str, output: String) -> (String, String) {
    let mut child = match Command::new("nali")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (base_tool.to_string(), output);
        }
        Err(_) => return (base_tool.to_string(), output),
    };

    // Write to nali's stdin and close it so nali knows input is done.
    let mut stdin = child.stdin.take().expect("stdin piped");
    stdin.write_all(output.as_bytes()).await.ok();
    drop(stdin);

    match timeout(NALI_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(out)) => {
            let enriched = String::from_utf8_lossy(&out.stdout).into_owned();
            if enriched.trim().is_empty() {
                (base_tool.to_string(), output)
            } else {
                (format!("{} + nali", base_tool), enriched)
            }
        }
        _ => (base_tool.to_string(), output),
    }
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
