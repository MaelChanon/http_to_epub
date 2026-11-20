//! sushiscan.net - same theme as sushiscan.fr, with two differences that matter:
//! the whole origin (image CDN included) sits behind Cloudflare, and `#readerarea`
//! is served empty and filled client-side, so page URLs come from the inlined
//! `ts_reader.run({...})` payload instead of from the markup.
//!
//! A Cloudflare clearance is optional and supplied by the caller - see
//! [`crate::utils::cloudflare`] for the split between the origin's always-on hotlink
//! protection and its intermittent challenge.

use super::{CfChapter, MangaInfo, Provider, base::keep_resolved};
use crate::utils::cloudflare::{CfError, CfSession, Clearance, widen};
use futures::stream::{self, StreamExt, TryStreamExt};
use scraper::{Html, Selector};
use serde::Deserialize;
use std::sync::Arc;

const MANGA_FETCH_CONCURRENCY: usize = 10;
const CHAPTER_FETCH_CONCURRENCY: usize = 10;

pub struct SushiScanNet {
    url: String,
    session: Arc<CfSession>,
}

impl SushiScanNet {
    /// Start here: no token needed while the origin is not challenging. If it starts
    /// again, calls fail with [`CfError::ClearanceRequired`] - then use
    /// [`SushiScanNet::with_clearance`].
    pub fn new(url: &str) -> Result<SushiScanNet, CfError> {
        let url = url.trim_end_matches('/').to_string();
        let session = CfSession::new(&url)?;
        Ok(SushiScanNet { url, session })
    }

    /// `clearance` must come from a challenge solver run by the caller. Does no I/O: a
    /// stale token only shows up as [`CfError::ClearanceRequired`] on the first request.
    pub fn with_clearance(url: &str, clearance: &Clearance) -> Result<SushiScanNet, CfError> {
        let url = url.trim_end_matches('/').to_string();
        let session = CfSession::with_clearance(&url, clearance)?;
        Ok(SushiScanNet { url, session })
    }

    /// Over a session built by the caller, e.g. with [`CfSession::with_options`] to
    /// add a proxy. The session must target the same origin as `url`.
    pub fn with_session(url: &str, session: Arc<CfSession>) -> SushiScanNet {
        let url = url.trim_end_matches('/').to_string();
        SushiScanNet { url, session }
    }

    /// The Cloudflare session, so callers can fetch covers and other assets from
    /// the protected origin with the same clearance.
    pub fn session(&self) -> &Arc<CfSession> {
        &self.session
    }

    async fn get_manga_chapter(
        &self,
        _tag: &str,
        url: &str,
        number: usize,
    ) -> Result<CfChapter, Box<dyn std::error::Error>> {
        let body = self.session.get_text(url).await.map_err(widen)?;
        let pages = extract_reader_images(&body)
            .ok_or_else(|| format!("no ts_reader payload on {}", url))?;

        Ok(CfChapter::new(pages, number, Arc::clone(&self.session)))
    }
}

impl Provider for SushiScanNet {
    type MangaInfo = MangaInfo;
    type Chapter = CfChapter;

    async fn get_manga_info(&self, tag: &str) -> Result<MangaInfo, Box<dyn std::error::Error>> {
        let url = format!("{}/catalogue/{}/", self.url, tag);
        let body = self.session.get_text(&url).await.map_err(widen)?;
        let document = Html::parse_document(&body);
        let title_selector = Selector::parse(".entry-title").expect("invalid title selector");
        let chapter_selector = Selector::parse("#chapterlist>ul").expect("invalid chapter selector");
        let cover_selector = Selector::parse(".seriestucontl img").expect("invalid cover selector");

        let name = document
            .select(&title_selector)
            .next()
            .map(|el| el.inner_html())
            .ok_or("manga title not found")?;

        let chapter_count = document
            .select(&chapter_selector)
            .next()
            .map(|el| el.child_elements().count())
            .unwrap_or(0);

        let cover_url = document
            .select(&cover_selector)
            .next()
            .map(|el| el.value().attr("src").unwrap_or("").to_string())
            .ok_or("cover not found")?;

        Ok(MangaInfo {
            chapter_count,
            cover_url,
            name,
            tag: tag.to_string(),
        })
    }

    async fn get_all_mangas(&self) -> Result<Vec<MangaInfo>, Box<dyn std::error::Error>> {
        let base_url = format!("{}/catalogue/", self.url);
        let mangas_selector =
            Selector::parse(".listupd>div>div>a").expect("invalid mangas selector");
        let mut all_tags: Vec<String> = Vec::new();
        let mut idx = 1;

        loop {
            let url = format!("{}?page={}", base_url, idx);
            let body = self.session.get_text(&url).await.map_err(widen)?;
            let document = Html::parse_document(&body);

            let page_tags: Vec<String> = document
                .select(&mangas_selector)
                .filter_map(|el| el.value().attr("href"))
                .filter_map(tag_from_url)
                .collect();
            if page_tags.is_empty() {
                break;
            }

            // the last page keeps being served for any higher `page=`, so stop as
            // soon as a page brings nothing new rather than looping forever
            let before = all_tags.len();
            for tag in page_tags {
                if !all_tags.contains(&tag) {
                    all_tags.push(tag);
                }
            }
            if all_tags.len() == before {
                break;
            }
            idx += 1;
        }

        println!("found {} mangas", all_tags.len());

        let results: Vec<(String, Result<MangaInfo, _>)> = stream::iter(all_tags)
            .map(|tag| async move {
                let info = self.get_manga_info(&tag).await;
                (tag, info)
            })
            .buffer_unordered(MANGA_FETCH_CONCURRENCY)
            .collect()
            .await;
        keep_resolved(results)
    }

    async fn get_manga_chapters(
        &self,
        manga_info: &MangaInfo,
    ) -> Result<Vec<CfChapter>, Box<dyn std::error::Error>> {
        let url = format!("{}/catalogue/{}/", self.url, manga_info.tag);
        let body = self.session.get_text(&url).await.map_err(widen)?;
        let document = Html::parse_document(&body);

        let chapter_selector =
            Selector::parse("#chapterlist ul li a").expect("invalid chapter selector");

        let mut chapter_urls: Vec<String> = document
            .select(&chapter_selector)
            .filter_map(|el| el.value().attr("href").map(str::to_string))
            .collect();
        chapter_urls.reverse();

        stream::iter(chapter_urls.into_iter().enumerate())
            .map(|(idx, url)| async move { self.get_manga_chapter(&manga_info.tag, &url, idx).await })
            .buffered(CHAPTER_FETCH_CONCURRENCY)
            .try_collect()
            .await
    }
}

#[derive(Deserialize)]
struct TsReader {
    sources: Vec<TsSource>,
}

#[derive(Deserialize)]
struct TsSource {
    images: Vec<String>,
}

/// Pulls the page list out of the `ts_reader.run({...})` call inlined in a chapter
/// page. Only the first source is used; the others are mirrors of the same scan.
fn extract_reader_images(html: &str) -> Option<Vec<String>> {
    const MARKER: &str = "ts_reader.run(";
    let start = html.find(MARKER)? + MARKER.len();
    let payload = balanced_object(&html[start..])?;
    let reader: TsReader = serde_json::from_str(payload).ok()?;
    reader
        .sources
        .into_iter()
        .find(|source| !source.images.is_empty())
        .map(|source| source.images)
}

/// Returns the leading `{...}` of `s`, matching braces and skipping string literals.
fn balanced_object(s: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (i, b) in s.bytes().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match b {
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// `https://sushiscan.net/catalogue/<tag>/` -> `<tag>`
fn tag_from_url(href: &str) -> Option<String> {
    let tag = href.trim_end_matches('/').rsplit('/').next()?;
    (!tag.is_empty()).then(|| tag.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_first_non_empty_source() {
        let html = r#"<script>ts_reader.run({"post_id":1,"sources":[{"source":"Server 1","images":[]},{"source":"Server 2","images":["https://c1/a.jpg","https://c1/b.jpg"]}]});</script>"#;
        assert_eq!(
            extract_reader_images(html).unwrap(),
            vec!["https://c1/a.jpg", "https://c1/b.jpg"]
        );
    }

    /// The payload carries escaped URLs and braces inside strings; the brace matcher
    /// must not be fooled by either.
    #[test]
    fn survives_escaped_strings_and_braces() {
        let html = r#"<script>ts_reader.run({"noimagehtml":"<center>{}<\/center>","sources":[{"images":["https:\/\/c1\/a.jpg"]}]});</script>"#;
        assert_eq!(extract_reader_images(html).unwrap(), vec!["https://c1/a.jpg"]);
    }

    #[test]
    fn no_payload_yields_none() {
        assert!(extract_reader_images("<div id=\"readerarea\"></div>").is_none());
        // truncated payload: unbalanced braces must not panic or return garbage
        assert!(extract_reader_images(r#"ts_reader.run({"sources":[{"images":["#).is_none());
    }

    #[test]
    fn tags_come_from_the_last_path_segment() {
        assert_eq!(tag_from_url("https://sushiscan.net/catalogue/07-ghost/").as_deref(), Some("07-ghost"));
        assert_eq!(tag_from_url("https://sushiscan.net/catalogue/07-ghost").as_deref(), Some("07-ghost"));
    }
}
