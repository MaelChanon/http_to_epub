
const MAX_RETRIES: u32 = 3;
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("failed to build http client")
}

pub async fn fetch_with_retry(client: &reqwest::Client, url: &str) -> Result<bytes::Bytes, reqwest::Error> {
    let mut last_err = None;
    for attempt in 1..=MAX_RETRIES {
        match client.get(url).send().await?.bytes().await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                eprintln!("try {}/{} failed - {} : {}", attempt, MAX_RETRIES, url, e);
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_secs(attempt as u64)).await;
            }
        }
    }
    Err(last_err.unwrap())
}

