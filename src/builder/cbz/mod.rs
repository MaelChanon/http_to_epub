use std::io::Write;
use zip::write::SimpleFileOptions;
use crate::utils::build_client;
use crate::builder::base::{BuildParams, encode_image};
use futures::StreamExt;

const PAGE_ENCODE_CONCURRENCY: usize = 20;

pub async fn build(param: &BuildParams, output_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let client = build_client();

    zip.start_file("000_cover.jpg", options)?;
    zip.write_all(&param.cover)?;

    let (width, height, split) = (param.width, param.height, param.split_double_page);
    for (chap_idx, chapter) in param.chapters.iter().enumerate() {
        let mut encoded_stream = chapter.fetch_pages(&client)
            .map(|page_bytes| tokio::task::spawn_blocking(move || {
                encode_image(page_bytes.to_vec(), width, height, split)
            }))
            .buffered(PAGE_ENCODE_CONCURRENCY);

        let mut page_idx = 0;
        while let Some(result) = encoded_stream.next().await {
            let encoded_pages = result.unwrap_or(Ok(vec![])).unwrap_or(vec![]);

            for (encoded, _, _) in encoded_pages {
                let img_path = format!("{:04}_{:04}.jpg", chap_idx + 1, page_idx + 1);
                zip.start_file(&img_path, options)?;
                zip.write_all(&encoded)?;
                page_idx += 1;
            }
        }
    }

    zip.finish()?;
    Ok(())
}
