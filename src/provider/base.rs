
use serde::{Deserialize, Serialize};
use futures::stream::{self, StreamExt};
use futures::Stream;
use std::pin::Pin;
use crate::utils::fetch_with_retry;

const PAGE_FETCH_CONCURRENCY: usize = 10;

#[derive(Serialize, Deserialize, Debug)]
pub struct MangaInfo {
    pub tag: String,
    pub name: String,
    pub cover_url: String,
    pub chapter_count: usize
}

#[derive(Serialize, Deserialize, Debug,Clone)]
pub struct Chapter {
    pub pages: Vec<String>,
    pub chapter_number: usize
}

pub trait Provider {
    type MangaInfo: TMangaInfo;
    type Chapter: TChapter;
    async fn get_manga_info(&self, tag: &str) -> Result<Self::MangaInfo, Box<dyn std::error::Error>>;
    async fn get_all_mangas(&self) -> Result<Vec<Self::MangaInfo>, Box<dyn std::error::Error>>;
    async fn get_manga_chapters(&self, manga_info: &Self::MangaInfo) -> Result<Vec<Self::Chapter>, Box<dyn std::error::Error>>;
}

pub trait TMangaInfo: Send + Sync {
    fn tag(&self) -> &str;
    fn name(&self) -> &str;
    fn cover_url(&self) -> &str;
}

impl TMangaInfo for MangaInfo {
    fn tag(&self) -> &str { &self.tag }
    fn name(&self) -> &str { &self.name }
    fn cover_url(&self) -> &str { &self.cover_url }
}

pub trait TChapter {
    fn fetch_pages<'a>(&'a self, client: &'a reqwest::Client) -> Pin<Box<dyn Stream<Item = bytes::Bytes> + Send + 'a>>;
}


impl TChapter for Chapter {

    fn fetch_pages<'a>(&'a self, client: &'a reqwest::Client) -> Pin<Box<dyn Stream<Item = bytes::Bytes> + Send + 'a>> {
        Box::pin(
            stream::iter(self.pages.iter())
                .map(move |url| {
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
                .buffered(PAGE_FETCH_CONCURRENCY)
                .filter_map(|x| async move { x })
        )
    }
}