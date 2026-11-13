use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;
use url::Url;

use crate::error::{AppError, AppResult};

const APP_USER_AGENT: &str =
    "Novelium/0.1.0 (Desktop Novel Reader; polite-client; +https://github.com/pineapplemanxd/Novelium)";

#[derive(Debug, Clone)]
struct RobotsPolicy {
    disallow: Vec<String>,
    allow: Vec<String>,
    crawl_delay_ms: u64,
    fetched_at: Instant,
}

impl RobotsPolicy {
    fn allow_all() -> Self {
        Self {
            disallow: Vec::new(),
            allow: Vec::new(),
            crawl_delay_ms: 80,
            fetched_at: Instant::now(),
        }
    }

    pub fn parse(body: &str) -> Self {
        let mut group_matches_star = false;
        let mut group_matches_novelium = false;
        let mut saw_rules_in_group = false;
        let mut saw_blank_since_ua = false;

        let mut star_disallow = Vec::new();
        let mut star_allow = Vec::new();
        let mut star_delay_ms: Option<u64> = None;

        let mut nov_disallow = Vec::new();
        let mut nov_allow = Vec::new();
        let mut nov_delay_ms: Option<u64> = None;

        let clean_body = body.strip_prefix('\u{feff}').unwrap_or(body);
        for raw_line in clean_body.lines() {
            let line = raw_line
                .trim_start_matches('\u{feff}')
                .split('#')
                .next()
                .unwrap_or("")
                .trim();
            if line.is_empty() {
                if group_matches_star || group_matches_novelium || saw_rules_in_group {
                    saw_blank_since_ua = true;
                }
                continue;
            }
            let Some((key, val)) = line.split_once(':') else {
                continue;
            };
            let k = key.trim().to_ascii_lowercase();
            let v = val.trim();

            if k == "user-agent" {
                if saw_rules_in_group || saw_blank_since_ua {
                    group_matches_star = false;
                    group_matches_novelium = false;
                    saw_rules_in_group = false;
                }
                saw_blank_since_ua = false;
                let ua = v.to_ascii_lowercase();
                if ua.contains("novelium") {
                    group_matches_novelium = true;
                } else if ua == "*" {
                    group_matches_star = true;
                }
                continue;
            }

            saw_rules_in_group = true;
            if group_matches_novelium {
                match k.as_str() {
                    "disallow" if !v.is_empty() => nov_disallow.push(v.to_string()),
                    "allow" if !v.is_empty() => nov_allow.push(v.to_string()),
                    "crawl-delay" => {
                        if let Ok(sec) = v.parse::<f64>() {
                            nov_delay_ms = Some((sec.clamp(0.1, 10.0) * 1000.0) as u64);
                        }
                    }
                    _ => {}
                }
            }
            if group_matches_star {
                match k.as_str() {
                    "disallow" if !v.is_empty() => star_disallow.push(v.to_string()),
                    "allow" if !v.is_empty() => star_allow.push(v.to_string()),
                    "crawl-delay" => {
                        if let Ok(sec) = v.parse::<f64>() {
                            star_delay_ms = Some((sec.clamp(0.1, 10.0) * 1000.0) as u64);
                        }
                    }
                    _ => {}
                }
            }
        }

        if !nov_disallow.is_empty() || !nov_allow.is_empty() || nov_delay_ms.is_some() {
            Self {
                disallow: nov_disallow,
                allow: nov_allow,
                crawl_delay_ms: nov_delay_ms.unwrap_or(200),
                fetched_at: Instant::now(),
            }
        } else {
            Self {
                disallow: star_disallow,
                allow: star_allow,
                crawl_delay_ms: star_delay_ms.unwrap_or(200),
                fetched_at: Instant::now(),
            }
        }
    }

    pub fn is_path_allowed(&self, path_and_query: &str) -> bool {
        let mut best_allow_len = 0usize;
        for rule in &self.allow {
            if matches_robots_rule(path_and_query, rule) {
                best_allow_len = best_allow_len.max(rule.len());
            }
        }
        let mut best_disallow_len = 0usize;
        for rule in &self.disallow {
            if matches_robots_rule(path_and_query, rule) {
                best_disallow_len = best_disallow_len.max(rule.len());
            }
        }
        if best_disallow_len == 0 {
            return true;
        }
        best_allow_len >= best_disallow_len
    }
}

fn matches_robots_rule(path: &str, rule: &str) -> bool {
    if rule == "/" {
        return true;
    }
    if rule.contains('*') {
        let parts: Vec<&str> = rule.split('*').collect();
        let mut cursor = path;
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            if i == 0 {
                if !cursor.starts_with(part) {
                    return false;
                }
                cursor = &cursor[part.len()..];
            } else if let Some(pos) = cursor.find(part) {
                cursor = &cursor[pos + part.len()..];
            } else {
                return false;
            }
        }
        return true;
    }
    if let Some(prefix) = rule.strip_suffix('$') {
        path == prefix
    } else {
        path.starts_with(rule)
    }
}

#[derive(Clone)]
pub struct SourceHttpClient {
    client: Client,
    robots_cache: Arc<Mutex<HashMap<String, RobotsPolicy>>>,
    domain_timers: Arc<Mutex<HashMap<String, Instant>>>,
    enforce_robots: bool,
}

impl SourceHttpClient {
    pub fn new() -> AppResult<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(APP_USER_AGENT));

        let client = Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(12))
            .build()?;

        Ok(Self {
            client,
            robots_cache: Arc::new(Mutex::new(HashMap::new())),
            domain_timers: Arc::new(Mutex::new(HashMap::new())),
            enforce_robots: false,
        })
    }

    pub fn with_robots_enforcement(mut self, enforce: bool) -> Self {
        self.enforce_robots = enforce;
        self
    }

    async fn get_policy(&self, url: &Url) -> RobotsPolicy {
        let Some(host) = url.host_str() else {
            return RobotsPolicy::allow_all();
        };
        let key = format!("{}://{}{}", url.scheme(), host, url.port().map(|p| format!(":{p}")).unwrap_or_default());

        // Check if loopback/test server (skip external robots wait if local test? No, even local test can serve /robots.txt or 404 fast!)
        {
            let cache = self.robots_cache.lock().await;
            if let Some(policy) = cache.get(&key) {
                if policy.fetched_at.elapsed() < Duration::from_secs(6 * 3600) {
                    return policy.clone();
                }
            }
        }

        let robots_url = format!("{key}/robots.txt");
        let policy = match self
            .client
            .get(&robots_url)
            .timeout(Duration::from_millis(2000))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let text = resp.text().await.unwrap_or_default();
                RobotsPolicy::parse(&text)
            }
            _ => RobotsPolicy::allow_all(),
        };

        let mut cache = self.robots_cache.lock().await;
        cache.insert(key, policy.clone());
        policy
    }

    async fn wait_rate_limit_and_check_robots(&self, url_str: &str) -> AppResult<()> {
        let url = Url::parse(url_str)?;
        let host = url.host_str().unwrap_or("unknown").to_string();

        let delay_ms = if self.enforce_robots {
            let policy = self.get_policy(&url).await;
            let mut path_q = url.path().to_string();
            if let Some(q) = url.query() {
                path_q.push('?');
                path_q.push_str(q);
            }
            if !policy.is_path_allowed(&path_q) {
                return Err(AppError::RobotsDisallowed(url_str.to_string()));
            }
            policy.crawl_delay_ms.min(400)
        } else {
            40
        };

        // Respect per-domain rate limiting without blocking other domains
        let sleep_dur = {
            let mut timers = self.domain_timers.lock().await;
            let now = Instant::now();
            let min_gap = Duration::from_millis(delay_ms);
            if let Some(last) = timers.get(&host) {
                let elapsed = now.saturating_duration_since(*last);
                if elapsed < min_gap {
                    let wait = min_gap - elapsed;
                    timers.insert(host.clone(), now + wait);
                    Some(wait)
                } else {
                    timers.insert(host.clone(), now);
                    None
                }
            } else {
                timers.insert(host.clone(), now);
                None
            }
        };

        if let Some(dur) = sleep_dur {
            tokio::time::sleep(dur).await;
        }
        Ok(())
    }

    async fn send_with_retry(&self, url: &str) -> AppResult<reqwest::Response> {
        self.wait_rate_limit_and_check_robots(url).await?;

        let mut attempt = 0u32;
        let max_attempts = 3u32;
        loop {
            attempt += 1;
            match self.client.get(url).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }
                    if (status == StatusCode::TOO_MANY_REQUESTS
                        || status == StatusCode::BAD_GATEWAY
                        || status == StatusCode::SERVICE_UNAVAILABLE
                        || status == StatusCode::GATEWAY_TIMEOUT)
                        && attempt < max_attempts
                    {
                        let retry_after = resp
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .map(|s| s.min(10))
                            .unwrap_or(attempt as u64);
                        tracing::warn!("HTTP {} for {} (attempt {}/{}), retrying in {}s", status, url, attempt, max_attempts, retry_after);
                        tokio::time::sleep(Duration::from_secs(retry_after)).await;
                        continue;
                    }
                    return Err(AppError::Other(format!(
                        "HTTP {} returned by {}",
                        status, url
                    )));
                }
                Err(e) => {
                    if attempt < max_attempts && (e.is_timeout() || e.is_connect()) {
                        let backoff_ms = 500 * (1u64 << (attempt - 1));
                        tracing::warn!("Network error for {} ({}), retrying in {}ms", url, e, backoff_ms);
                        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    return Err(AppError::Network(e));
                }
            }
        }
    }

    pub async fn get_text(&self, url: &str) -> AppResult<String> {
        let resp = self.send_with_retry(url).await?;
        let text = resp.text().await?;
        Ok(text)
    }

    pub async fn get_bytes(&self, url: &str) -> AppResult<Vec<u8>> {
        let resp = self.send_with_retry(url).await?;
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> AppResult<T> {
        let text = self.get_text(url).await?;
        let parsed = serde_json::from_str::<T>(&text)?;
        Ok(parsed)
    }

    pub async fn probe_latency(&self, url: &str) -> AppResult<u64> {
        self.wait_rate_limit_and_check_robots(url).await?;
        let start = Instant::now();
        let resp = self
            .client
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        let ms = start.elapsed().as_millis() as u64;
        if resp.status().is_success() || resp.status().is_redirection() {
            Ok(ms)
        } else {
            Err(AppError::Other(format!("Status {}", resp.status())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_robots_parser_disallow_and_allow() {
        let txt = r#"
User-agent: *
Disallow: /ebooks/search
Disallow: /private/
Allow: /private/public-page
Crawl-delay: 1.5
"#;
        let policy = RobotsPolicy::parse(txt);
        assert!(!policy.is_path_allowed("/ebooks/search?q=foo"));
        assert!(!policy.is_path_allowed("/private/secret"));
        assert!(policy.is_path_allowed("/private/public-page"));
        assert!(policy.is_path_allowed("/ebooks/1342"));
        assert_eq!(policy.crawl_delay_ms, 1500);
    }

    #[test]
    fn test_robots_novelium_specific_group() {
        let txt = r#"
User-agent: BadBot
Disallow: /

User-agent: *
Disallow: /admin

User-agent: Novelium
Disallow: /restricted
"#;
        let policy = RobotsPolicy::parse(txt);
        assert!(!policy.is_path_allowed("/restricted/foo"));
        assert!(policy.is_path_allowed("/books/123"));
    }

    #[test]
    fn test_robots_isolates_groups_across_blank_lines_and_bom() {
        let txt = "\u{feff}User-agent: *\nAllow: /\n\nUser-agent: Amazonbot\nDisallow: /\n\nUser-agent: GPTBot\nDisallow: /";
        let policy = RobotsPolicy::parse(txt);
        assert!(policy.is_path_allowed("/browse/scores/top"));
        assert!(policy.is_path_allowed("/books/123"));
    }

    #[tokio::test]
    async fn test_probe_latency_respects_robots_txt() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = sock.read(&mut buf).await;
                    let body = "User-agent: *\nDisallow: /blocked\n";
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });

        let http_default = SourceHttpClient::new().unwrap();
        assert!(http_default.probe_latency(&format!("{base}/blocked/api")).await.is_ok());

        let http_strict = SourceHttpClient::new().unwrap().with_robots_enforcement(true);
        let res = http_strict.probe_latency(&format!("{base}/blocked/api")).await;
        assert!(matches!(res, Err(AppError::RobotsDisallowed(_))));
        assert!(http_strict.probe_latency(&format!("{base}/ok")).await.is_ok());

        srv.abort();
    }
}
