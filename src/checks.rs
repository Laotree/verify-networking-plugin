use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio::time::timeout;

const PROBE_COUNT: usize = 3;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const LATENCY_WARN_MS: u128 = 500;

// Countries where API access is restricted
const BLOCKED_COUNTRIES: &[&str] = &["CN", "HK", "KP", "CU", "IR", "SY", "RU", "BY"];

/// Which CLI tool (and thus which API endpoint) to check.
pub struct Target {
    /// Human-readable tool name shown in UI messages.
    pub tool_name: &'static str,
    /// Hostname used for DNS resolution and connectivity probes.
    pub host: &'static str,
    /// `host:port` string used for TCP probes.
    pub addr: &'static str,
}

pub const CLAUDE: Target = Target {
    tool_name: "Claude",
    host: "api.anthropic.com",
    addr: "api.anthropic.com:443",
};

pub const CODEX: Target = Target {
    tool_name: "Codex",
    host: "api.openai.com",
    addr: "api.openai.com:443",
};

#[derive(Debug)]
pub struct CheckResult {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Deserialize)]
struct IpInfo {
    ip: String,
    country: Option<String>,
    org: Option<String>,
}

fn error_result(name: &'static str) -> CheckResult {
    CheckResult { name, status: Status::Fail, detail: "Internal error".to_string() }
}

pub async fn run_all(target: &'static Target) -> Vec<CheckResult> {
    let (dns_result, ip_result, conn_result) = tokio::join!(
        async {
            tokio::task::spawn_blocking(move || check_dns(target))
                .await
                .unwrap_or_else(|_| error_result("DNS"))
        },
        check_ip(target),
        check_connectivity(target),
    );
    vec![dns_result, ip_result, conn_result]
}

fn check_dns(target: &Target) -> CheckResult {
    match format!("{}:443", target.host).to_socket_addrs() {
        Ok(mut addrs) => {
            let ip = addrs
                .next()
                .map(|a| a.ip().to_string())
                .unwrap_or_else(|| "?".to_string());
            CheckResult {
                name: "DNS",
                status: Status::Ok,
                detail: format!("{} → {}", target.host, ip),
            }
        }
        Err(e) => CheckResult {
            name: "DNS",
            status: Status::Fail,
            detail: format!("Cannot resolve {}: {}", target.host, e),
        },
    }
}

async fn check_ip(target: &Target) -> CheckResult {
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(8)).build() {
        Ok(c) => c,
        Err(e) => {
            return CheckResult {
                name: "Exit IP",
                status: Status::Warn,
                detail: format!("Client build error: {}", e),
            }
        }
    };

    match client.get("https://ipinfo.io/json").send().await {
        Ok(resp) => match resp.json::<IpInfo>().await {
            Ok(info) => {
                let country = info.country.as_deref().unwrap_or("??");
                let org = info.org.as_deref().unwrap_or("unknown");
                if BLOCKED_COUNTRIES.contains(&country) {
                    CheckResult {
                        name: "Exit IP",
                        status: Status::Fail,
                        detail: format!(
                            "{} [{}] {} — {} unavailable in this region",
                            info.ip, country, org, target.tool_name
                        ),
                    }
                } else {
                    CheckResult {
                        name: "Exit IP",
                        status: Status::Ok,
                        detail: format!("{} [{}] {}", info.ip, country, org),
                    }
                }
            }
            Err(e) => CheckResult {
                name: "Exit IP",
                status: Status::Warn,
                detail: format!("Cannot parse ipinfo.io response: {}", e),
            },
        },
        Err(e) => CheckResult {
            name: "Exit IP",
            status: Status::Warn,
            detail: format!("Cannot reach ipinfo.io: {}", e),
        },
    }
}

async fn check_connectivity(target: &Target) -> CheckResult {
    let addr = target.addr;
    let host = target.host;

    let tasks: Vec<_> = (0..PROBE_COUNT)
        .map(|_| {
            tokio::spawn(async move {
                let start = Instant::now();
                let ok = timeout(PROBE_TIMEOUT, TcpStream::connect(addr))
                    .await
                    .is_ok_and(|r| r.is_ok());
                (ok, start.elapsed().as_millis())
            })
        })
        .collect();

    let mut latencies: Vec<u128> = Vec::new();
    let mut failures = 0usize;

    for task in tasks {
        match task.await {
            Ok((true, ms)) => latencies.push(ms),
            _ => failures += 1,
        }
    }

    if latencies.is_empty() {
        return CheckResult {
            name: "Connectivity",
            status: Status::Fail,
            detail: format!("Cannot reach {} (100% loss)", host),
        };
    }

    let avg_ms = latencies.iter().sum::<u128>() / latencies.len() as u128;
    let loss_pct = (failures * 100) / PROBE_COUNT;

    let status = if failures > 0 || avg_ms > LATENCY_WARN_MS {
        Status::Warn
    } else {
        Status::Ok
    };

    CheckResult {
        name: "Connectivity",
        status,
        detail: format!("{} avg {}ms  loss {}%", host, avg_ms, loss_pct),
    }
}
