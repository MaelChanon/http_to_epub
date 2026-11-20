//! HTTP session for an origin behind Cloudflare.
//!
//! Two separate protections live on such an origin, and conflating them wastes a
//! debugging cycle:
//!
//! - **Hotlink protection** rejects any image request without a `Referer`. It is the
//!   origin's own rule, always on, and a session handles it unconditionally.
//! - **The Cloudflare challenge** demands a `cf_clearance` cookie. It is toggled by
//!   whoever runs the site, and on sushiscan.net it has been observed both on
//!   (2026-09-14) and off (2026-09-30). A [`Clearance`] is therefore optional.
//!
//! This crate deliberately does **not** solve the challenge. Solving it needs a
//! headless browser (FlareSolverr and friends), which is an infrastructure concern:
//! it belongs to whoever runs the process, so the token can be obtained once,
//! cached, and shared by every consumer - including the ones that fetch images
//! without going through this crate at all.
//!
//! So start with [`CfSession::new`], which carries no token. If the origin ever turns
//! the challenge back on, requests fail with [`CfError::ClearanceRequired`] instead of
//! a bare 403 - the caller's cue to obtain a token and switch to
//! [`CfSession::with_clearance`]. That error is the trigger for building solver
//! plumbing, not something to build against in advance.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{COOKIE, HeaderMap, HeaderValue, REFERER};
use serde::{Deserialize, Serialize};

const MAX_RETRIES: u32 = 3;

/// Stable marker on [`CfError::ClearanceRequired`] messages. An FFI boundary that can
/// only carry strings (the Neon addon) can still tell "a token is needed here" apart
/// from a genuine failure by matching this prefix.
pub const CLEARANCE_REQUIRED_MARKER: &str = "CF_CLEARANCE_REQUIRED";

/// A Cloudflare clearance, as produced by a challenge solver.
///
/// The two fields are indivisible: Cloudflare binds `cf_clearance` to the exact
/// User-Agent of the browser that earned it (and to that browser's exit IP), so
/// replaying the cookie under a different User-Agent is rejected.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clearance {
    /// Full `Cookie` header value, e.g. `cf_clearance=abc...`.
    pub cookie: String,
    pub user_agent: String,
}

impl Clearance {
    pub fn new(cookie: impl Into<String>, user_agent: impl Into<String>) -> Clearance {
        Clearance {
            cookie: cookie.into(),
            user_agent: user_agent.into(),
        }
    }

    /// From the bare `cf_clearance` value, e.g. copied from a browser's devtools.
    /// `user_agent` must be that same browser's exact User-Agent (see the type docs).
    pub fn from_token(token: &str, user_agent: impl Into<String>) -> Clearance {
        Clearance::new(format!("cf_clearance={}", token.trim()), user_agent)
    }

    /// Builds the `Cookie` header from the `name`/`value` pairs a solver returns.
    pub fn from_cookies<'a>(
        cookies: impl IntoIterator<Item = (&'a str, &'a str)>,
        user_agent: impl Into<String>,
    ) -> Clearance {
        let cookie = cookies
            .into_iter()
            .map(|(name, value)| format!("{}={}", name, value))
            .collect::<Vec<_>>()
            .join("; ");
        Clearance::new(cookie, user_agent)
    }
}

#[derive(Debug)]
pub enum CfError {
    /// Cloudflare is challenging: no clearance was supplied, or the one supplied went
    /// stale. Only the caller - which owns the solver - can produce a fresh one, so
    /// retrying as-is will never succeed.
    ClearanceRequired { url: String },
    /// The origin answered, but not with a success status.
    Status {
        url: String,
        status: reqwest::StatusCode,
    },
    Transport(reqwest::Error),
    /// The clearance could not be turned into valid headers.
    InvalidClearance(String),
}

impl CfError {
    /// Whether the caller should obtain a fresh clearance and retry.
    pub fn needs_clearance(&self) -> bool {
        matches!(self, CfError::ClearanceRequired { .. })
    }
}

impl std::fmt::Display for CfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CfError::ClearanceRequired { url } => write!(
                f,
                "{}: cloudflare is challenging on {} - a fresh clearance is needed",
                CLEARANCE_REQUIRED_MARKER, url
            ),
            CfError::Status { url, status } => write!(f, "{} answered {}", url, status),
            CfError::Transport(e) => write!(f, "transport error: {}", e),
            CfError::InvalidClearance(e) => write!(f, "invalid clearance: {}", e),
        }
    }
}

impl std::error::Error for CfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CfError::Transport(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for CfError {
    fn from(e: reqwest::Error) -> CfError {
        CfError::Transport(e)
    }
}

/// An HTTP client for a Cloudflare-fronted origin: always sends a `Referer` and a
/// browser User-Agent, and optionally replays a [`Clearance`].
pub struct CfSession {
    client: reqwest::Client,
}

impl CfSession {
    /// A session with no clearance - enough whenever the origin is not currently
    /// challenging. `url` is any URL on the origin; its scheme and host set the
    /// `Referer` the hotlink protection expects. Does no I/O.
    pub fn new(url: &str) -> Result<Arc<CfSession>, CfError> {
        CfSession::build(url, None)
    }

    /// A session replaying `clearance`, for when the origin *is* challenging.
    pub fn with_clearance(url: &str, clearance: &Clearance) -> Result<Arc<CfSession>, CfError> {
        CfSession::build(url, Some(clearance))
    }

    fn build(url: &str, clearance: Option<&Clearance>) -> Result<Arc<CfSession>, CfError> {
        let referer = origin_of(url);
        Ok(Arc::new(CfSession {
            client: client_for(clearance, &referer)?,
        }))
    }

    /// The underlying client, for callers that need to drive their own requests
    /// against the same origin. Cheap: `reqwest::Client` is a handle on a shared pool.
    pub fn client(&self) -> reqwest::Client {
        self.client.clone()
    }

    /// GETs `url`. Non-2xx answers surface as errors, so a Cloudflare error page is
    /// never mistaken for a page or an image.
    pub async fn get(&self, url: &str) -> Result<reqwest::Response, CfError> {
        let response = self.client.get(url).send().await?;
        if is_challenge(response.status(), response.headers()) {
            return Err(CfError::ClearanceRequired {
                url: url.to_string(),
            });
        }
        let status = response.status();
        if !status.is_success() {
            return Err(CfError::Status {
                url: url.to_string(),
                status,
            });
        }
        Ok(response)
    }

    pub async fn get_text(&self, url: &str) -> Result<String, CfError> {
        Ok(self.get(url).await?.text().await?)
    }

    /// `utils::fetch_with_retry` equivalent. Retries transport hiccups but gives up at
    /// once when a clearance is needed: no amount of retrying produces a token.
    pub async fn get_bytes(&self, url: &str) -> Result<Bytes, CfError> {
        let mut last_err = None;
        for attempt in 1..=MAX_RETRIES {
            let outcome = match self.get(url).await {
                Ok(response) => response.bytes().await.map_err(CfError::from),
                Err(e) => Err(e),
            };
            match outcome {
                Ok(bytes) => return Ok(bytes),
                Err(e) if e.needs_clearance() => return Err(e),
                Err(e) => {
                    eprintln!("try {}/{} failed - {} : {}", attempt, MAX_RETRIES, url, e);
                    last_err = Some(e);
                    tokio::time::sleep(Duration::from_secs(attempt as u64)).await;
                }
            }
        }
        Err(last_err.expect("MAX_RETRIES is non-zero"))
    }
}

/// Widens an error into the plain `Box<dyn Error>` the `Provider` trait returns.
/// The coercion only drops the `Send + Sync` bounds.
pub fn widen(e: impl std::error::Error + Send + Sync + 'static) -> Box<dyn std::error::Error> {
    Box::new(e)
}

/// Tells a Cloudflare interstitial apart from an ordinary 403. Cloudflare tags the
/// former with `cf-mitigated: challenge`; the origin's own hotlink protection answers
/// a plain 403, which renewing the clearance would never fix.
fn is_challenge(status: reqwest::StatusCode, headers: &HeaderMap) -> bool {
    if !matches!(status.as_u16(), 403 | 503) {
        return false;
    }
    headers.contains_key("cf-mitigated")
        // older "checking your browser" interstitials carry no cf-mitigated header
        || (status.as_u16() == 503
            && headers
                .get(reqwest::header::SERVER)
                .is_some_and(|s| s.as_bytes().eq_ignore_ascii_case(b"cloudflare")))
}

/// `https://host/some/path` -> `https://host/`
fn origin_of(url: &str) -> String {
    let after_scheme = url.find("://").map(|i| i + 3).unwrap_or(0);
    match url[after_scheme..].find('/') {
        Some(i) => url[..after_scheme + i + 1].to_string(),
        None => format!("{}/", url),
    }
}

/// Always sets a `Referer` for the origin's hotlink protection. With a clearance, also
/// replays the browser identity it was issued to - the cookie *and* that browser's
/// User-Agent, which Cloudflare binds together.
///
/// Both headers sit in the client's default headers rather than in a jar, which is safe
/// here because the client only ever talks to the one origin - image CDN subdomain
/// included, since `cf_clearance` is scoped to the whole apex domain.
fn client_for(clearance: Option<&Clearance>, referer: &str) -> Result<reqwest::Client, CfError> {
    let invalid = |e: reqwest::header::InvalidHeaderValue| CfError::InvalidClearance(e.to_string());

    let mut headers = HeaderMap::new();
    headers.insert(REFERER, HeaderValue::from_str(referer).map_err(invalid)?);
    if let Some(clearance) = clearance {
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&clearance.cookie).map_err(invalid)?,
        );
    }

    let user_agent = clearance.map_or(crate::utils::USER_AGENT, |c| c.user_agent.as_str());

    reqwest::Client::builder()
        .user_agent(user_agent)
        .default_headers(headers)
        .build()
        .map_err(CfError::Transport)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn cf_mitigated_403_is_a_challenge() {
        assert!(is_challenge(
            StatusCode::FORBIDDEN,
            &headers(&[("cf-mitigated", "challenge"), ("server", "cloudflare")]),
        ));
    }

    /// The origin's hotlink protection answers a bare 403 through Cloudflare. Renewing
    /// the clearance would never fix it, so it must not be reported as a challenge.
    #[test]
    fn hotlink_403_is_not_a_challenge() {
        assert!(!is_challenge(
            StatusCode::FORBIDDEN,
            &headers(&[("server", "cloudflare"), ("cf-ray", "a3b02ef70c36702a-CDG")]),
        ));
    }

    #[test]
    fn cloudflare_503_is_a_challenge() {
        assert!(is_challenge(
            StatusCode::SERVICE_UNAVAILABLE,
            &headers(&[("server", "cloudflare")]),
        ));
    }

    #[test]
    fn origin_503_is_not_a_challenge() {
        assert!(!is_challenge(
            StatusCode::SERVICE_UNAVAILABLE,
            &headers(&[("server", "nginx")]),
        ));
    }

    #[test]
    fn success_is_never_a_challenge() {
        assert!(!is_challenge(
            StatusCode::OK,
            &headers(&[("cf-mitigated", "challenge")]),
        ));
    }

    #[test]
    fn origin_keeps_scheme_and_host_only() {
        assert_eq!(origin_of("https://sushiscan.net/catalogue/x/"), "https://sushiscan.net/");
        assert_eq!(origin_of("https://sushiscan.net"), "https://sushiscan.net/");
        assert_eq!(origin_of("https://sushiscan.net/"), "https://sushiscan.net/");
    }

    #[test]
    fn clearance_joins_cookies() {
        let c = Clearance::from_cookies([("cf_clearance", "abc"), ("other", "1")], "UA/1");
        assert_eq!(c.cookie, "cf_clearance=abc; other=1");
        assert_eq!(c.user_agent, "UA/1");
    }

    #[test]
    fn clearance_from_bare_token() {
        let c = Clearance::from_token(" abc.def-1 \n", "UA/1");
        assert_eq!(c.cookie, "cf_clearance=abc.def-1");
    }
}
