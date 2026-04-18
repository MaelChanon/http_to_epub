  mod provider;
mod utils;
mod epub;
use provider::{sushiscan::SushiScan, Provider};
use epub::{build,EpubParams};

use crate::{epub::{CbzParams, build_cbz}, utils::{build_client, fetch_with_retry}};
 #[tokio::main]                                                                                                                          
  async fn main() {
    let scanner = SushiScan::new("https://sushiscan.fr");
    let op = scanner.get_manga_info("jojos-bizarre-adventure-jojolion").await.unwrap();
    let chapters = scanner.get_manga_chapters(&op).await.unwrap();
    let cover = fetch_with_retry(&build_client(), &op.cover_url).await.unwrap();
    // let build_params = EpubParams{
    //   width: 1120,
    //   height: 1680,
    //   creator: "mael".to_string(),
    //   lang: "fr-FR".to_string(),
    //   tablet_model: "kobo.3.1".to_string(),
    //   manga_info: op,
    //   chapters: chapters,
    //   cover: cover
    // };
    // let _ = build(&build_params,std::path::Path::new("/home/mael/Documents/code/htpp_to_epub/out/blabla.epub")).await;
    let cbz_params = CbzParams {
      manga_info: op,
      chapters: chapters,
      cover: cover,
  };

  build_cbz(&cbz_params, std::path::Path::new("out/manga.cbz")).await.unwrap();
  }