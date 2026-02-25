use super::{MangaInfo, Provider, Chapter};
use scraper::{Html, Selector};
use futures::stream::{self, StreamExt};


pub struct SushiScan {
    url: String,
}

impl SushiScan {
    pub fn new(url: &str) -> impl Provider {
        SushiScan {
            url: url.to_string()
        }
    }
    async fn get_manga_chapter(&self, tag: &str, chapter: usize) -> Result<Chapter, Box<dyn std::error::Error>> {
        let url = format!("{}/{}-chapitre-{}", self.url, tag, chapter);
        println!("test = {}",url);
        let body = reqwest::get(&url).await?.text().await?;
        let document = Html::parse_document(&body);
        let images_selector = Selector::parse("#readerarea>p>img").expect("invalid images selector");
    
        let image_urls: Vec<String> = document
            .select(&images_selector)
            .filter_map(|el| el.value().attr("src").map(|s| s.to_string()))
            .collect();

     
        Ok(Chapter{pages: image_urls.into_iter().collect(), chapter_number: chapter})
    }
}
impl Provider for SushiScan {
    async fn get_manga_info(&self, tag: &str) -> Result<MangaInfo, Box<dyn std::error::Error>> {
        let url = format!("{}/catalogue/{}", self.url, tag);
        let body = reqwest::get(&url).await?.text().await?;
        let document = Html::parse_document(&body);
        let title_selector = Selector::parse(".entry-title").expect("invalid title selector");
        let chapter_selector = Selector::parse("#chapterlist>ul").expect("invalid chapter selector");

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

        Ok(MangaInfo {
            chapter_count,
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
            let body = reqwest::get(&url).await?.text().await?;
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
            .buffer_unordered(50)
            .collect()
            .await;
        results.into_iter().collect()
    }
    async fn get_manga_chapters(&self, manga_info: &MangaInfo) -> Result<Vec<Chapter>, Box<dyn std::error::Error>> {
        let ids: Vec<usize> = (1..=manga_info.chapter_count).collect();
        stream::iter(ids)
            .map(|chapter| async move { self.get_manga_chapter(&manga_info.tag, chapter).await })
            .buffered(50)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect()
    }
}
