use super::{MangaInfo, Provider, Chapter};
use scraper::{Html, Selector};
use futures::stream::{self, StreamExt, TryStreamExt};
use crate::utils::build_client;


pub struct SushiScan {
    url: String,
    client: reqwest::Client,
}

impl SushiScan {
    pub fn new(url: &str) -> impl Provider {
        SushiScan {
            url: url.to_string(),
            client: build_client(),
        }
    }
    async fn get_manga_chapter(&self, tag: &str, url: &str,number: usize) -> Result<Chapter, Box<dyn std::error::Error>> {
        let body = self.client.get(url).send().await?.text().await?;
        let document = Html::parse_document(&body);
        let images_selector = Selector::parse("#readerarea>p>img").expect("invalid images selector");

        let image_urls: Vec<String> = document
            .select(&images_selector)
            .filter_map(|el| el.value().attr("src").map(|s| s.to_string()))
            .collect();


        Ok(Chapter{pages: image_urls.into_iter().collect(), chapter_number: number})
    }
}
impl Provider for SushiScan {
    async fn get_manga_info(&self, tag: &str) -> Result<MangaInfo, Box<dyn std::error::Error>> {
        let url = format!("{}/catalogue/{}/", self.url, tag);
        let body = self.client.get(&url).send().await?.text().await?;
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
        let mangas_selector = Selector::parse(".listupd>div>div>a").expect("invalid mangas selector");
        let mut all_tags: Vec<String> = Vec::new();
        let mut idx = 1;

        // Phase 1 : collecter tous les tags séquentiellement
        loop {
            let url = format!("{}?page={}", base_url, idx);
            let body = self.client.get(&url).send().await?.text().await?;
            let document = Html::parse_document(&body);

            let page_mangas: Vec<_> = document.select(&mangas_selector).collect();
            if page_mangas.is_empty() {
                break;
            }

            for el in page_mangas {
                let page_url = el.value().attr("href").unwrap_or("").to_string();
                let mut iter = page_url.split('/');
                iter.next_back();
                let tag = iter.next_back().unwrap_or("").to_string();
                print!("url = {} \n", page_url);
                print!("tag = {} \n", tag);
                all_tags.push(tag);
            }
            idx += 1;
            println!("idx = {}",idx)
        }

        // Phase 2 : fetch toutes les MangaInfo en parallèle (max 10 requêtes simultanées)
        let results: Vec<Result<MangaInfo, _>> = stream::iter(all_tags)
            .map(|tag| async move { self.get_manga_info(&tag).await })
            .buffer_unordered(10)
            .collect()
            .await;
        results.into_iter().collect()
    }

async fn get_manga_chapters(&self, manga_info: &MangaInfo) -> Result<Vec<Chapter>, Box<dyn std::error::Error>> {
    let url = format!("{}/catalogue/{}", self.url, manga_info.tag);
    let body = self.client.get(&url).send().await?.text().await?;
    let document = Html::parse_document(&body);

    let chapter_selector = Selector::parse("#chapterlist ul li a").expect("invalid chapter selector");

    let chapter_urls: Vec<String> = document
        .select(&chapter_selector)
        .filter_map(|el| el.value().attr("href").map(str::to_string))
        .collect();

    stream::iter(chapter_urls.into_iter().enumerate())
        .map(|(idx, url)| async move {
            self.get_manga_chapter(&manga_info.tag, &url, idx).await
        })
        .buffered(10)
        .try_collect()
        .await
}
}
