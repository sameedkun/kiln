//! `kiln requests [site]` ([ADR 0065]): how fast each site answers, from Caddy's access log.
//!
//! [ADR 0065]: ../../../docs/adr/0065-request-timing.md

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// How many of the slowest requests are kept per site.
const SLOWEST: usize = 10;

/// One request from the access log.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub host: String,
    pub at: f64,
    pub method: String,
    pub uri: String,
    pub status: u64,
    pub ms: f64,
}

/// A request from one access-log line, or `None` for a line that isn't one.
#[must_use]
pub fn parse(line: &str) -> Option<Request> {
    let v: Value = serde_json::from_str(line).ok()?;
    let request = &v["request"];
    let host = request["host"].as_str()?;
    // `shop.test:8080` and `[::1]:80` are the site without the port.
    let host = match host.rsplit_once(':') {
        Some((name, port)) if port.parse::<u16>().is_ok() && !name.ends_with(':') => name,
        _ => host,
    };
    Some(Request {
        host: host.trim_end_matches('.').to_ascii_lowercase(),
        at: v["ts"].as_f64().unwrap_or_default(),
        method: request["method"].as_str().unwrap_or_default().to_owned(),
        uri: request["uri"].as_str().unwrap_or_default().to_owned(),
        status: v["status"].as_u64().unwrap_or_default(),
        ms: v["duration"].as_f64()? * 1000.0,
    })
}

/// The value below which `percent` of `sorted` (ascending) falls: the nearest-rank percentile.
#[must_use]
pub fn percentile(sorted: &[f64], percent: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (percent * sorted.len()).div_ceil(100).max(1);
    sorted[rank.min(sorted.len()) - 1]
}

/// Requests and failures (5xx) in each of the 24 hours up to `now` (seconds since 1970),
/// oldest first; older requests are left out (P4C-05).
#[must_use]
pub fn hourly(requests: &[&Request], now: f64) -> (Vec<u64>, Vec<u64>) {
    let mut counts = vec![0_u64; 24];
    let mut failures = vec![0_u64; 24];
    for r in requests {
        let ago = (now - r.at) / 3600.0;
        if !(0.0..24.0).contains(&ago) {
            continue;
        }
        // 0 to 23 hours ago, as a bucket from the oldest (0) to the newest (23).
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // in 0..24
        let bucket = 23 - ago.floor() as usize;
        counts[bucket] += 1;
        if r.status >= 500 {
            failures[bucket] += 1;
        }
    }
    (counts, failures)
}

/// How many requests answered with each class of status: 2xx, 3xx, 4xx, 5xx.
#[must_use]
pub fn statuses(requests: &[&Request]) -> Value {
    let class = |c: u64| requests.iter().filter(|r| r.status / 100 == c).count();
    json!({"2xx": class(2), "3xx": class(3), "4xx": class(4), "5xx": class(5)})
}

/// Each site's figures, by host, with the last 24 hours counted up to now.
#[must_use]
pub fn summarize(requests: &[Request]) -> BTreeMap<String, Value> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    summarize_at(requests, now)
}

/// [`summarize`] as of `now` (seconds since 1970).
#[must_use]
pub fn summarize_at(requests: &[Request], now: f64) -> BTreeMap<String, Value> {
    let mut by_host: BTreeMap<&str, Vec<&Request>> = BTreeMap::new();
    for r in requests {
        by_host.entry(r.host.as_str()).or_default().push(r);
    }
    by_host
        .into_iter()
        .map(|(host, all)| {
            let mut times: Vec<f64> = all.iter().map(|r| r.ms).collect();
            times.sort_by(f64::total_cmp);
            let mut slowest = all.clone();
            slowest.sort_by(|a, b| b.ms.total_cmp(&a.ms));
            slowest.truncate(SLOWEST);
            let round = |ms: f64| (ms * 10.0).round() / 10.0;
            let (hours, failing_hours) = hourly(&all, now);
            (
                host.to_owned(),
                json!({
                    "host": host,
                    "count": all.len(),
                    "failures": all.iter().filter(|r| r.status >= 500).count(),
                    "median_ms": round(percentile(&times, 50)),
                    "p95_ms": round(percentile(&times, 95)),
                    "since": all.iter().map(|r| r.at).fold(f64::INFINITY, f64::min),
                    "hours": hours,
                    "failing_hours": failing_hours,
                    "statuses": statuses(&all),
                    "slowest": slowest.iter().map(|r| json!({
                        "at": r.at,
                        "method": r.method,
                        "uri": r.uri,
                        "status": r.status,
                        "ms": round(r.ms),
                    })).collect::<Vec<_>>(),
                }),
            )
        })
        .collect()
}

/// `kiln requests [site]`: every site's figures, or one site's.
pub fn requests(site: Option<&str>) -> Result<(), String> {
    let log = crate::web::server()?.access_log_file();
    let text = match std::fs::read(&log) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("cannot read {}: {e}", log.display())),
    };
    let wanted = site.map(|s| {
        let s = s.to_ascii_lowercase();
        if s.contains('.') {
            s
        } else {
            format!("{s}.test")
        }
    });
    let all: Vec<Request> = text
        .lines()
        .filter_map(parse)
        .filter(|r| wanted.as_deref().is_none_or(|w| r.host == w))
        .collect();
    let sites = summarize(&all);
    if crate::out::json() {
        crate::out::data(json!({
            "log": log,
            "sites": sites.values().collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if sites.is_empty() {
        say!(
            "No requests recorded yet{}.",
            wanted.map(|w| format!(" for {w}")).unwrap_or_default()
        );
        return Ok(());
    }
    for s in sites.values() {
        say!(
            "{}: {} requests, {} failed; median {} ms, slowest 5% over {} ms",
            s["host"].as_str().unwrap_or_default(),
            s["count"],
            s["failures"],
            s["median_ms"],
            s["p95_ms"]
        );
        for r in s["slowest"].as_array().into_iter().flatten().take(3) {
            say!(
                "  {:>8} ms  {} {} {}",
                r["ms"],
                r["status"],
                r["method"].as_str().unwrap_or_default(),
                r["uri"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(host: &str, uri: &str, status: u64, seconds: f64) -> String {
        json!({
            "level": "info", "ts": 1_794_570_330.0, "logger": "http.log.access.kiln_access",
            "msg": "handled request",
            "request": {"host": host, "method": "GET", "uri": uri},
            "status": status, "duration": seconds,
        })
        .to_string()
    }

    #[test]
    fn lines_are_read_into_requests_by_site() {
        let r = parse(&line("Shop.test:8080", "/cart?x=1", 200, 0.0125)).unwrap();
        assert_eq!(r.host, "shop.test");
        assert_eq!(r.uri, "/cart?x=1");
        assert!((r.ms - 12.5).abs() < 1e-9);
        assert_eq!(parse("not json"), None);
        assert_eq!(parse(r#"{"msg":"something else"}"#), None);
    }

    #[test]
    fn figures_per_site() {
        let mut lines: Vec<String> = (1..=20)
            .map(|i| line("shop.test", &format!("/p{i}"), 200, f64::from(i) / 1000.0))
            .collect();
        lines.push(line("shop.test", "/boom", 500, 0.3));
        lines.push(line("blog.test", "/", 200, 0.002));
        let all: Vec<Request> = lines.iter().filter_map(|l| parse(l)).collect();
        let sites = summarize(&all);
        let shop = &sites["shop.test"];
        assert_eq!(shop["count"], 21);
        assert_eq!(shop["failures"], 1);
        assert_eq!(shop["median_ms"], 11.0);
        assert_eq!(shop["p95_ms"], 20.0);
        assert_eq!(shop["slowest"][0]["uri"], "/boom");
        assert_eq!(shop["slowest"].as_array().unwrap().len(), SLOWEST);
        assert_eq!(sites["blog.test"]["count"], 1);
    }

    #[test]
    fn the_last_day_by_hour_and_status() {
        let mut lines = vec![
            line("shop.test", "/", 200, 0.01),
            line("shop.test", "/x", 404, 0.01),
            line("shop.test", "/boom", 500, 0.01),
        ];
        lines.push(line("shop.test", "/old", 302, 0.01));
        let mut all: Vec<Request> = lines.iter().filter_map(|l| parse(l)).collect();
        let now = 1_794_600_000.0;
        all[0].at = now - 10.0;
        all[1].at = now - 10.0;
        all[2].at = now - 3600.0 * 2.5;
        all[3].at = now - 3600.0 * 30.0;
        let shop = &summarize_at(&all, now)["shop.test"];
        let hours: Vec<u64> = serde_json::from_value(shop["hours"].clone()).unwrap();
        assert_eq!(hours.len(), 24);
        assert_eq!(hours[23], 2);
        assert_eq!(hours[21], 1);
        assert_eq!(hours.iter().sum::<u64>(), 3);
        assert_eq!(shop["failing_hours"][21], 1);
        assert_eq!(
            shop["statuses"],
            json!({"2xx": 1, "3xx": 1, "4xx": 1, "5xx": 1})
        );
    }

    #[test]
    fn percentiles_by_nearest_rank() {
        assert!((percentile(&[1.0, 2.0, 3.0, 4.0], 50) - 2.0).abs() < 1e-9);
        assert!((percentile(&[5.0], 95) - 5.0).abs() < 1e-9);
        assert!(percentile(&[], 50).abs() < 1e-9);
    }
}
