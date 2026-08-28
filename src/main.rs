mod provider;
mod utils;
mod builder;
use provider::{Provider, TChapter, TMangaInfo};
use builder::{build, FileMod, BuildParams};
use crate::{provider::sushiscan::SushiScan, utils::{build_client, fetch_with_retry}};

#[tokio::main]
async fn main() {
    let scanner = SushiScan::new("https://sushiscan.fr");
    let op = scanner.get_manga_info("kingdom").await.unwrap();
    let chapters= scanner.get_manga_chapters(&op).await.unwrap()[104..105].to_vec();
    let cover = fetch_with_retry(&build_client(), op.cover_url()).await.unwrap();
    let params = BuildParams {
        width: 1264,
        height: 1680,
        creator: "mael".to_string(),
        lang: "fr-FR".to_string(),
        manga_info: Box::new(op),
        chapters: chapters.into_iter().map(|c| Box::new(c) as Box<dyn TChapter + Send + Sync>).collect(),
        cover,
        split_double_page: false
    };
    let _ = build(&FileMod::EPUB(params), std::path::Path::new("out/jjb.epub")).await;
}
