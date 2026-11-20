pub mod cloudflare;

use std::time::Duration;

const MAX_RETRIES: u32 = 3;
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Time allowed to open a connection. A host that does not even accept the
/// connection within this window is down, not slow.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Time allowed for a whole request, body included. Generous enough for a large
/// scan page, short enough that a stalled origin cannot hang a download forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A client builder with the shared User-Agent and timeouts, for callers that need
/// to add their own settings before building.
pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
}

pub fn build_client() -> reqwest::Client {
    client_builder()
        .build()
        .expect("failed to build http client")
}

/// GETs `url` and returns its body. Non-2xx answers are errors, so a gateway error
/// page (504...) is never parsed as an empty catalogue or chapter.
pub async fn get_text(client: &reqwest::Client, url: &str) -> Result<String, reqwest::Error> {
    client.get(url).send().await?.error_for_status()?.text().await
}

/// Downloads `url`, retrying transport errors and non-2xx answers, so an error page
/// is never handed back as image bytes.
pub async fn fetch_with_retry(client: &reqwest::Client, url: &str) -> Result<bytes::Bytes, reqwest::Error> {
    let mut last_err = None;
    for attempt in 1..=MAX_RETRIES {
        let outcome = async {
            client.get(url).send().await?.error_for_status()?.bytes().await
        }
        .await;
        match outcome {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                eprintln!("try {}/{} failed - {} : {}", attempt, MAX_RETRIES, url, e);
                last_err = Some(e);
                tokio::time::sleep(Duration::from_secs(attempt as u64)).await;
            }
        }
    }
    Err(last_err.expect("MAX_RETRIES is non-zero"))
}
