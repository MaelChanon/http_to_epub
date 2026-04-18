
use serde::{Deserialize, Serialize};
use futures::stream::{self, StreamExt};
use futures::Stream;
use std::pin::Pin;
use crate::utils::fetch_with_retry;


#[derive(Serialize, Deserialize, Debug)]
pub struct MangaInfo {
    pub tag: String,
    pub name: String,
    pub cover_url: String,
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
}

pub trait TChapter {
    fn fetch_pages<'a>(&'a self, client: &'a reqwest::Client) -> Pin<Box<dyn Stream<Item = bytes::Bytes> + Send + 'a>>;
    fn get_pages(&self) -> &Vec<String>;
}


impl TChapter for Chapter {
    fn get_pages(&self) -> &Vec<String> {
        &self.pages
    }

    fn fetch_pages<'a>(&'a self, client: &'a reqwest::Client) -> Pin<Box<dyn Stream<Item = bytes::Bytes> + Send + 'a>> {
        Box::pin(
            stream::iter(self.pages.iter().cloned())
                .map(move |url| {
                    let client = client.clone();
                    async move {
                        match fetch_with_retry(&client, &url).await {
                            Ok(bytes) => Some(bytes),
                            Err(e) => {
                                eprintln!("error fetching page {}: {}", url, e);
                                None
                            }
                        }
                    }
                })
                .buffered(5)
                .filter_map(|x| async move { x })
        )
    }
}