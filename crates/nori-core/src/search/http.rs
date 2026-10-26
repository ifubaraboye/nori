//! Shared HTTP client with a consistent retry policy and user agent.

use std::time::Duration;

use chrono::DateTime;
use tokio_util::sync::CancellationToken;

pub const USER_AGENT: &str = "nori/0.1";
const MAX_BACKOFF: Duration = Duration::from_secs(20);
const BASE_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    pub fn new() -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(USER_AGENT)
            .build()?;
        Ok(Self { client })
    }

    /// Fetch a URL, retrying transient failures once per requested retry.
    pub async fn get(
        &self,
        url: &str,
        retries: usize,
        cancel: &CancellationToken,
    ) -> anyhow::Result<Vec<u8>> {
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..=retries {
            if cancel.is_cancelled() {
                anyhow::bail!("cancelled");
            }
            let response = tokio::select! {
                _ = cancel.cancelled() => anyhow::bail!("cancelled"),
                r = self.client.get(url).header("Accept", "*/*").send() => r,
            };
            let response = match response {
                Ok(r) => r,
                Err(e) => {
                    if cancel.is_cancelled() {
                        anyhow::bail!("cancelled");
                    }
                    last_err = Some(e.into());
                    if attempt < retries {
                        sleep_or_cancel(backoff(attempt, Duration::ZERO), cancel).await?;
                    }
                    continue;
                }
            };
            let status = response.status();
            if !is_retryable(status) {
                if !status.is_success() {
                    return Err(anyhow::anyhow!(
                        "HTTP {} from {}",
                        status.as_u16(),
                        host_of(url)
                    ));
                }
                return Ok(response.bytes().await?.to_vec());
            }
            let server = response
                .headers()
                .get("server")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .map(parse_retry_after)
                .unwrap_or_default();
            if status == reqwest::StatusCode::SERVICE_UNAVAILABLE
                && (server.contains("cloudflare") || server.contains("ddos-guard"))
            {
                return Err(anyhow::anyhow!("blocked by {server}"));
            }
            if attempt >= retries {
                return Err(anyhow::anyhow!(
                    "HTTP {} from {}",
                    status.as_u16(),
                    host_of(url)
                ));
            }
            if retry_after > MAX_BACKOFF {
                return Err(anyhow::anyhow!("retry in {}s", retry_after.as_secs()));
            }
            sleep_or_cancel(backoff(attempt, retry_after), cancel).await?;
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("unreachable")))
    }
}

fn is_retryable(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

pub fn host_of(raw: &str) -> &str {
    let raw = raw.split_once("://").map(|(_, rest)| rest).unwrap_or(raw);
    raw.split('/').next().unwrap_or(raw)
}

fn backoff(attempt: usize, retry_after: Duration) -> Duration {
    let exp = BASE_DELAY.saturating_mul(1 << attempt.min(6));
    let exp = exp.min(MAX_BACKOFF);
    let jitter = rand::random::<f64>() * exp.as_secs_f64();
    let delay = Duration::from_secs_f64(jitter);
    delay.max(retry_after)
}

fn parse_retry_after(value: &str) -> Duration {
    let value = value.trim();
    if value.is_empty() {
        return Duration::ZERO;
    }
    if let Ok(secs) = value.parse::<u64>() {
        return Duration::from_secs(secs);
    }
    if let Ok(date) = DateTime::parse_from_rfc2822(value) {
        let secs = date.timestamp() - chrono::Utc::now().timestamp();
        if secs > 0 {
            return Duration::from_secs(secs as u64);
        }
    }
    Duration::ZERO
}

async fn sleep_or_cancel(delay: Duration, cancel: &CancellationToken) -> anyhow::Result<()> {
    tokio::select! {
        _ = cancel.cancelled() => anyhow::bail!("cancelled"),
        _ = tokio::time::sleep(delay) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_short() {
        assert_eq!(host_of("https://yts.mx/api/v2/list.json"), "yts.mx");
    }
}
