use std::io::Write;
use bytes::Bytes;
use zip::write::SimpleFileOptions;
use crate::provider::{Chapter, MangaInfo, base::TChapter};
use crate::utils::build_client;
use image::ImageFormat;
use std::io::Cursor;
use futures::StreamExt;

pub struct CbzParams {
    pub cover: Bytes,
    pub chapters: Vec<Chapter>,
    pub manga_info: MangaInfo,
}


pub async fn build(param: &CbzParams, output_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let client = build_client();

    zip.start_file("000_cover.jpg", options)?;
    zip.write_all(&param.cover)?;

    for (chap_idx, chapter) in param.chapters.iter().enumerate() {
        let mut pages = chapter.fetch_pages(&client);
        let mut page_idx = 0;
        while let Some(page_bytes) = pages.next().await {
            let img_path = format!("{:04}_{:04}.jpg", chap_idx + 1, page_idx + 1);

            let encoded = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
                let img = image::load_from_memory(&page_bytes)?;
                let mut output = Vec::new();
                img.write_to(&mut Cursor::new(&mut output), ImageFormat::Jpeg)?;
                Ok(output)
            }).await
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;

            zip.start_file(&img_path, options)?;
            zip.write_all(&encoded)?;
            println!("cbz page {} chap {}", page_idx + 1, chap_idx + 1);
            page_idx += 1;
        }
    }

    zip.finish()?;
    Ok(())
}
