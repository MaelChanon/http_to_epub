mod provider;
use provider::{sushiscan::SushiScan, Provider};

 #[tokio::main]                                                                                                                          
  async fn main() {
      let scanner = SushiScan::new("https://sushiscan.fr");
    let op = scanner.get_manga_info("ichi-the-witch").await.unwrap();
    let chapters = scanner.get_manga_chapters(&op).await.unwrap();
    for (i, chapter) in chapters.iter().enumerate() {
        let dir = format!("out/{}/{}-{}", op.tag, op.tag, chapter.chapter_number);
        chapter.save(std::path::Path::new(&dir)).await.unwrap();
        println!("chapitre {} sauvegardé", i + 1);
    }
  }