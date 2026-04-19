mod provider;
mod utils;
mod builder;
use provider::{sushiscan::SushiScan, Provider, TChapter};
use builder::{build, FileMod, BuildParams};
use crate::{utils::{build_client, fetch_with_retry}};

#[tokio::main]
async fn main() {
    let scanner = SushiScan::new("https://sushiscan.fr");
    let op = scanner.get_manga_info("jojos-bizarre-adventure-jojolion").await.unwrap();
    let chapters= scanner.get_manga_chapters(&op).await.unwrap();
    let cover = fetch_with_retry(&build_client(), &op.cover_url).await.unwrap();
    let params = BuildParams {
        width: 1120,
        height: 1680,
        creator: "mael".to_string(),
        lang: "fr-FR".to_string(),
        manga_info: Box::new(op),
        chapters: chapters.into_iter().map(|c| Box::new(c) as Box<dyn TChapter + Send + Sync>).collect(),
        cover,
        split_double_page: true
    };
    let _ = build(&FileMod::EPUB(params), std::path::Path::new("out/kmt.epub")).await;
}
