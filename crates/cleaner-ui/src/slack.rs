//! Browser-origin HTTP adapter for Slack's authenticated web-session endpoints.

use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;
use serde_json::Value;
use web_sys::UrlSearchParams;

/// Supported response failures, deliberately omitting token and message content.
#[derive(Debug)]
pub(crate) enum SlackError {
    /// Network request failed or was blocked.
    Transport,
    /// Response could not be parsed.
    InvalidResponse,
    /// The signed-in session could not access the requested method.
    Api(String),
    /// Slack's request limit was reached repeatedly.
    RateLimited,
}

impl std::fmt::Display for SlackError {
    /// A redacted user-visible description.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport => formatter.write_str("Slack network request failed"),
            Self::InvalidResponse => formatter.write_str("Slack returned an invalid response"),
            Self::Api(code) => write!(formatter, "Slack rejected the request: {code}"),
            Self::RateLimited => formatter.write_str("Slack rate limit persisted after bounded retries"),
        }
    }
}

impl std::error::Error for SlackError {}

/// A short-lived client holding an in-memory session credential.
pub(crate) struct Slack<'a> {
    /// An authenticated token captured locally from the signed-in Slack tab.
    token: &'a str,
}

impl<'a> Slack<'a> {
    /// Builds the Slack adapter; no token is persisted by this type.
    #[must_use]
    pub(crate) fn new(token: &'a str) -> Self {
        Self { token }
    }

    /// Calls a documented Slack endpoint and returns its unwrapped object.
    ///
    /// # Errors
    /// Returns a redacted failure if the session, network, or API is unavailable.
    pub(crate) async fn call(&self, method: &str, fields: &[(&str, &str)]) -> Result<Value, SlackError> {
        let params = UrlSearchParams::new().map_err(|_| SlackError::Transport)?;
        for (key, value) in fields {
            params.append(key, value);
        }
        let body = params.to_string().as_string().ok_or(SlackError::Transport)?;
        let url = format!("https://slack.com/api/{method}");
        for attempt in 0..3_u32 {
            let request = Request::post(&url)
                .header("Authorization", &format!("Bearer {}", self.token))
                .header("Content-Type", "application/x-www-form-urlencoded;charset=UTF-8")
                .body(body.clone())
                .map_err(|_| SlackError::Transport)?;
            let response = request.send().await.map_err(|_| SlackError::Transport)?;
            if response.status() == 429 {
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|seconds| seconds.parse::<u32>().ok())
                    .unwrap_or(3)
                    .clamp(1, 30);
                TimeoutFuture::new(delay.saturating_mul(1000)).await;
                if attempt == 2 {
                    return Err(SlackError::RateLimited);
                }
                continue;
            }
            if !response.ok() {
                return Err(SlackError::Api(format!("http_{}", response.status())));
            }
            let data = response
                .json::<Value>()
                .await
                .map_err(|_| SlackError::InvalidResponse)?;
            if data.get("ok").and_then(Value::as_bool) != Some(true) {
                let code = data.get("error").and_then(Value::as_str).unwrap_or("unknown_error");
                return Err(SlackError::Api(code.chars().take(100).collect()));
            }
            TimeoutFuture::new(1100).await;
            return Ok(data);
        }
        Err(SlackError::RateLimited)
    }
}
