use super::{MangaInfo, Provider, Chapter};
use scraper::{Html, Selector};
use futures::stream::{self, StreamExt, TryStreamExt};
use crate::utils::build_client;

const MANGA_FETCH_CONCURRENCY: usize = 10;
const CHAPTER_FETCH_CONCURRENCY: usize = 10;


pub struct MangaOrigins {
    url: String,
    client: reqwest::Client,
}

impl MangaOrigins {
    pub fn new(url: &str) -> MangaOrigins {
        MangaOrigins {
            url: url.to_string(),
            client: build_client(),
        }
    }

    fn extract_tags(document: &Html, selector: &Selector, tags: &mut Vec<String>) {
        for el in document.select(selector) {
            let href = el.value().attr("href").unwrap_or("");
            let mut parts = href.trim_end_matches('/').rsplitn(2, '/');
            if let Some(tag) = parts.next() {
                if !tag.is_empty() && !tags.contains(&tag.to_string()) {
                    tags.push(tag.to_string());
                }
            }
        }
    }

    async fn get_manga_chapter(&self, _tag: &str, url: &str,number: usize) -> Result<Chapter, Box<dyn std::error::Error>> {
        let body = self.client.get(url).send().await?.text().await?;
        let document = Html::parse_document(&body);
        let images_selector = Selector::parse(".reading-content img").expect("invalid images selector");

        let image_urls: Vec<String> = document
            .select(&images_selector)
            .map(|el| {
                el.value().attr("data-src")
                    .or_else(|| el.value().attr("src"))
                    .unwrap_or("")
                    .to_string()
            })
            .collect();


        Ok(Chapter{pages: image_urls.into_iter().collect(), chapter_number: number})
    }
}
impl Provider for MangaOrigins {
    type MangaInfo = MangaInfo;
    type Chapter = Chapter;

    async fn get_manga_info(&self, tag: &str) -> Result<MangaInfo, Box<dyn std::error::Error>> {
        let url = format!("{}/oeuvre/{}/", self.url, tag);
        let body = self.client.get(&url).send().await?.text().await?;
        let document = Html::parse_document(&body);
        let title_selector = Selector::parse(".post-title>h1").expect("invalid title selector");
        let cover_selector = Selector::parse(".ori-sr-cover img").expect("invalid cover selector");
        let name = document
            .select(&title_selector)
            .next()
            .map(|el| el.inner_html().trim().to_string())
            .ok_or("manga title not found")?;

        let chapter_row_selector = Selector::parse(".ori-chl-row").expect("invalid chapter selector");
        let chapter_count = document
            .select(&chapter_row_selector)
            .count();

        let cover_url = document
            .select(&cover_selector)
            .next()
            .map(|el| {
                el.value().attr("data-src")
                    .or_else(|| el.value().attr("src"))
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default();

        Ok(MangaInfo {
            chapter_count,
            cover_url,
            name,
            tag: tag.to_string(),
        })
    }
    async fn get_all_mangas(&self) -> Result<Vec<MangaInfo>, Box<dyn std::error::Error>> {
        let link_selector = Selector::parse(".item-thumb a").expect("invalid link selector");
        let ajax_url = format!("{}/wp-admin/admin-ajax.php", self.url);
        let mut all_tags: Vec<String> = Vec::new();

        // First batch from initial page HTML
        let initial_body = self.client.get(format!("{}/catalogues/", self.url)).send().await?.text().await?;
        Self::extract_tags(&Html::parse_document(&initial_body), &link_selector, &mut all_tags);

        // Subsequent batches via AJAX
        let mut page = 1u32;
        loop {
            let params = [
                ("action", "madara_load_more"),
                ("page", &page.to_string()),
                ("template", "madara-core/content/content-archive"),
                ("vars[paged]", "1"),
                ("vars[orderby]", "meta_value_num"),
                ("vars[template]", "archive"),
                ("vars[sidebar]", "full"),
                ("vars[post_type]", "wp-manga"),
                ("vars[post_status]", "publish"),
                ("vars[meta_key]", "_latest_update"),
                ("vars[order]", "desc"),
                ("vars[meta_query][relation]", "AND"),
                ("vars[manga_archives_item_layout]", "big_thumbnail"),
                ("vars[manga_archives_item_columns]", "0"),
            ];

            let body = params.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");

            let html = self.client.post(&ajax_url)
                .header("x-requested-with", "XMLHttpRequest")
                .header("content-type", "application/x-www-form-urlencoded; charset=UTF-8")
                .body(body)
                .send().await?.text().await?;
            if html.trim().is_empty() {
                break;
            }

            let before = all_tags.len();
            Self::extract_tags(&Html::parse_fragment(&html), &link_selector, &mut all_tags);
            if all_tags.len() == before {
                break;
            }

            page += 1;
        }

        println!("found {} mangas", all_tags.len());

        let results: Vec<Result<MangaInfo, _>> = stream::iter(all_tags)
            .map(|tag| async move { self.get_manga_info(&tag).await })
            .buffer_unordered(MANGA_FETCH_CONCURRENCY)
            .collect()
            .await;
        results.into_iter().collect()
    }

async fn get_manga_chapters(&self, manga_info: &MangaInfo) -> Result<Vec<Chapter>, Box<dyn std::error::Error>> {
    let url = format!("{}/oeuvre/{}/", self.url, manga_info.tag);
    let html = self.client.get(&url).send().await?.text().await?;

    let document = Html::parse_document(&html);
    let chapter_selector = Selector::parse(".ori-chl-row a.ori-chl-corps").expect("invalid chapter selector");

    let mut chapter_urls: Vec<String> = document
        .select(&chapter_selector)
        .filter_map(|el| el.value().attr("href").map(|str| format!("{}?style=list", str.trim_end_matches('/'))))
        .collect();
    chapter_urls.reverse();
    stream::iter(chapter_urls.into_iter().enumerate())
        .map(|(idx, url)| async move {
            self.get_manga_chapter(&manga_info.tag, &url, idx).await
        })
        .buffered(CHAPTER_FETCH_CONCURRENCY)
        .try_collect()
        .await
}
}
