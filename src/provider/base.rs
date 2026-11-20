
use serde::{Deserialize, Serialize};
use futures::stream::{self, StreamExt};
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use crate::utils::fetch_with_retry;
use crate::utils::cloudflare::{CfError, CfSession};

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

/// Keeps the mangas that resolved and logs the others, so one broken entry (a 504,
/// a page missing its title...) does not sink the whole catalogue. A missing
/// Cloudflare clearance stays fatal: every other entry would fail the same way, and
/// only the caller can fix it.
pub(crate) fn keep_resolved<T>(
    results: Vec<(String, Result<T, Box<dyn std::error::Error>>)>,
) -> Result<Vec<T>, Box<dyn std::error::Error>> {
    let mut resolved = Vec::with_capacity(results.len());
    for (tag, result) in results {
        match result {
            Ok(manga) => resolved.push(manga),
            Err(e) if e.downcast_ref::<CfError>().is_some_and(CfError::needs_clearance) => return Err(e),
            Err(e) => eprintln!("skipping {}: {}", tag, e),
        }
    }
    Ok(resolved)
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

/// A [`Chapter`] whose pages are downloaded through a Cloudflare session, for origins
/// whose images sit behind the same protection as their pages (and so reject a bare
/// client).
pub struct CfChapter {
    pub pages: Vec<String>,
    pub chapter_number: usize,
    session: Arc<CfSession>,
}

impl CfChapter {
    pub fn new(pages: Vec<String>, chapter_number: usize, session: Arc<CfSession>) -> CfChapter {
        CfChapter { pages, chapter_number, session }
    }

    /// Drops the session, keeping only the page list (for serialisation, caching…).
    pub fn into_plain(self) -> Chapter {
        Chapter {
            pages: self.pages,
            chapter_number: self.chapter_number,
        }
    }
}

impl TChapter for CfChapter {
    /// The shared client is ignored on purpose: it carries no clearance, so a
    /// challenging origin would answer 403 to every page.
    fn fetch_pages<'a>(&'a self, _client: &'a reqwest::Client) -> Pin<Box<dyn Stream<Item = bytes::Bytes> + Send + 'a>> {
        Box::pin(
            stream::iter(self.pages.iter())
                .map(move |url| async move {
                    match self.session.get_bytes(url).await {
                        Ok(bytes) => Some(bytes),
                        Err(e) => {
                            eprintln!("error fetching page {}: {}", url, e);
                            None
                        }
                    }
                })
                .buffered(PAGE_FETCH_CONCURRENCY)
                .filter_map(|x| async move { x })
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_entries_are_skipped() {
        let results: Vec<(String, Result<u32, Box<dyn std::error::Error>>)> = vec![
            ("a".into(), Ok(1)),
            ("b".into(), Err("504 Gateway Timeout".into())),
            ("c".into(), Ok(3)),
        ];
        assert_eq!(keep_resolved(results).unwrap(), vec![1, 3]);
    }

    #[test]
    fn missing_clearance_stays_fatal() {
        let results: Vec<(String, Result<u32, Box<dyn std::error::Error>>)> = vec![
            ("a".into(), Ok(1)),
            ("b".into(), Err(Box::new(CfError::ClearanceRequired { url: "https://x/".into() }))),
        ];
        assert!(keep_resolved(results).is_err());
    }
}
