use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};
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
    /// Service-owned Cloudflare trace endpoint used to observe the egress IP
    /// for traffic that matches this service's routing rules.
    pub trace_url: &'static str,
}

pub const CLAUDE: Target = Target {
    tool_name: "Claude",
    host: "api.anthropic.com",
    addr: "api.anthropic.com:443",
    trace_url: "https://api.anthropic.com/cdn-cgi/trace",
};

pub const CODEX: Target = Target {
    tool_name: "Codex",
    host: "api.openai.com",
    addr: "api.openai.com:443",
    trace_url: "https://api.openai.com/cdn-cgi/trace",
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

/// Egress fields the region gate needs from a trace response.
struct TraceInfo {
    ip: String,
    country: String,
    colo: String,
}

/// Read the `key=value` pairs of a Cloudflare trace response.
///
/// Returns `None` when the body is not a trace, or when the egress IP or the
/// country is missing. The region gate fails closed on a missing country: an
/// unknown region cannot be reported as a supported one.
fn parse_trace(body: &str) -> Option<TraceInfo> {
    let mut ip = None;
    let mut country = None;
    let mut colo = None;
    for line in body.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "ip" => ip = Some(value.trim()),
                "loc" => country = Some(value.trim().to_uppercase()),
                "colo" => colo = Some(value.trim()),
                _ => {}
            }
        }
    }

    Some(TraceInfo {
        ip: ip.filter(|v| !v.is_empty())?.to_string(),
        country: country.filter(|v| !v.is_empty())?,
        colo: colo.filter(|v| !v.is_empty()).unwrap_or("?").to_string(),
    })
}

/// Egress IP to look for in the traceroute hops.
///
/// Only a resolved IP or a region failure yields a value. The remaining
/// statuses report a problem in prose, so their detail must not be split as
/// if it started with an address.
pub fn exit_ip(results: &[CheckResult]) -> Option<String> {
    results
        .iter()
        .find(|r| r.name == "Exit IP" && r.status != Status::Warn)
        .and_then(|r| r.detail.split_whitespace().next())
        .map(|s| s.to_string())
}

fn error_result(name: &'static str) -> CheckResult {
    CheckResult {
        name,
        status: Status::Fail,
        detail: "Internal error".to_string(),
    }
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
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return CheckResult {
                name: "Exit IP",
                status: Status::Warn,
                detail: format!("Client build error: {}", e),
            }
        }
    };

    // A generic IP echo service cannot answer this question: split routing can
    // send it and the target service through different outbounds. The trace
    // endpoint below belongs to the target service, so the egress IP it reports
    // is the one used for traffic that matches that service.
    let body = match client.get(target.trace_url).send().await {
        Ok(resp) => match resp.text().await {
            Ok(body) => body,
            Err(e) => {
                return CheckResult {
                    name: "Exit IP",
                    status: Status::Warn,
                    detail: format!("Cannot read service trace response: {}", e),
                }
            }
        },
        Err(e) => {
            return CheckResult {
                name: "Exit IP",
                status: Status::Warn,
                detail: format!("Cannot reach service trace {}: {}", target.trace_url, e),
            }
        }
    };

    let info = match parse_trace(&body) {
        Some(info) => info,
        None => {
            return CheckResult {
                name: "Exit IP",
                status: Status::Warn,
                detail: format!(
                    "Cannot read egress IP or country from service trace {}",
                    target.trace_url
                ),
            }
        }
    };

    if BLOCKED_COUNTRIES.contains(&info.country.as_str()) {
        CheckResult {
            name: "Exit IP",
            status: Status::Fail,
            detail: format!(
                "{} [{}] colo={} — {} unavailable in this region",
                info.ip, info.country, info.colo, target.tool_name
            ),
        }
    } else {
        CheckResult {
            name: "Exit IP",
            status: Status::Ok,
            detail: format!(
                "{} [{}] colo={} via {}",
                info.ip, info.country, info.colo, target.trace_url
            ),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    // Shape returned by a Cloudflare trace endpoint: one key=value pair per line.
    const TRACE: &str = "fl=455f123
h=api.anthropic.com
ip=203.0.113.7
ts=1790769805.000
visit_scheme=https
uag=curl/8.7.1
colo=KIX
sliver=050-tier1
http=http/2
loc=JP
tls=TLSv1.3
sni=plaintext
warp=off
gateway=off
rbi=off
kex=X25519
";

    #[test]
    fn reads_egress_fields_from_a_service_trace() {
        let info = parse_trace(TRACE).expect("trace must parse");
        assert_eq!(info.ip, "203.0.113.7");
        assert_eq!(info.country, "JP");
        assert_eq!(info.colo, "KIX");
    }

    #[test]
    fn reads_egress_fields_from_a_partial_trace() {
        let info = parse_trace("ip=203.0.113.7\nloc=JP\n").expect("trace must parse");
        assert_eq!(info.ip, "203.0.113.7");
        assert_eq!(info.country, "JP");
    }

    #[test]
    fn rejects_a_trace_without_a_country() {
        assert!(parse_trace("ip=203.0.113.7\ncolo=KIX").is_none());
    }

    #[test]
    fn rejects_a_trace_with_an_empty_country() {
        assert!(parse_trace("ip=203.0.113.7\nloc=\ncolo=KIX").is_none());
    }

    #[test]
    fn rejects_a_trace_without_an_ip() {
        assert!(parse_trace("loc=JP\ncolo=KIX").is_none());
    }

    #[test]
    fn rejects_a_response_that_is_not_a_trace() {
        assert!(parse_trace("<html><body>403 Forbidden</body></html>").is_none());
        assert!(parse_trace("").is_none());
    }

    #[test]
    fn falls_back_to_a_placeholder_colo() {
        let info = parse_trace("ip=203.0.113.7\nloc=JP").expect("trace must parse");
        assert_eq!(info.colo, "?");
    }

    #[test]
    fn normalises_the_country_before_the_region_gate() {
        let info = parse_trace("ip=203.0.113.7\nloc=cn").expect("trace must parse");
        assert!(BLOCKED_COUNTRIES.contains(&info.country.as_str()));
    }

    #[test]
    fn exit_ip_ignores_a_result_that_reports_a_problem() {
        let results = vec![CheckResult {
            name: "Exit IP",
            status: Status::Warn,
            detail: "Cannot read egress IP or country from service trace".to_string(),
        }];
        assert_eq!(exit_ip(&results), None);
    }

    #[test]
    fn exit_ip_returns_the_address_of_a_resolved_result() {
        let results = vec![CheckResult {
            name: "Exit IP",
            status: Status::Ok,
            detail: "203.0.113.7 [JP] colo=KIX via https://api.anthropic.com/cdn-cgi/trace"
                .to_string(),
        }];
        assert_eq!(exit_ip(&results), Some("203.0.113.7".to_string()));
    }

    #[test]
    fn exit_ip_returns_the_address_of_a_region_failure() {
        let results = vec![CheckResult {
            name: "Exit IP",
            status: Status::Fail,
            detail: "203.0.113.7 [CN] colo=SJC — Claude unavailable in this region".to_string(),
        }];
        assert_eq!(exit_ip(&results), Some("203.0.113.7".to_string()));
    }
}
