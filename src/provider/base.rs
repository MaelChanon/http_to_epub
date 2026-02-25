use serde::{Deserialize, Serialize};
use futures::stream::{self, StreamExt};


#[derive(Serialize, Deserialize, Debug)]
pub struct MangaInfo {
    pub tag: String,
    pub name: String,
    pub chapter_count: usize
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Chapter {
    pub pages: Vec<String>,
    pub chapter_number: usize
}

pub trait Provider {
    async fn get_manga_info(&self, tag: &str) -> Result<MangaInfo, Box<dyn std::error::Error>>;
    async fn get_all_mangas(&self) -> Result<Vec<MangaInfo>, Box<dyn std::error::Error>>;
    async fn get_manga_chapters(&self, manga_info: &MangaInfo) -> Result<Vec<Chapter>, Box<dyn std::error::Error>>;
    //fn getChapter(number: u32) -> Chapter
}

const MAX_RETRIES: u32 = 3;

async fn fetch_with_retry(url: &str) -> Result<bytes::Bytes, reqwest::Error> {
    let mut last_err = None;
    for attempt in 1..=MAX_RETRIES {
        match reqwest::get(url).await?.bytes().await {
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

impl Chapter {
    pub async fn save(&self, dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        tokio::fs::create_dir_all(dir).await?;
        let results: Vec<(String, Result<bytes::Bytes, reqwest::Error>)> = stream::iter(self.pages.clone())
            .map(|url| async move {
                let bytes = fetch_with_retry(&url).await;
                (url, bytes)
            })
            .buffered(10)
            .collect()
            .await;

        for (i, (url, result)) in results.into_iter().enumerate() {
            let ext = url.rsplit('.').next().unwrap_or("jpg");
            let path = dir.join(format!("{:03}.{}", i + 1, ext));
            match result {
                Ok(bytes) => tokio::fs::write(path, bytes).await?,
                Err(e) => eprintln!("error fetching page {} : {}", i + 1, e),
            }
        }
        Ok(())
    }
}