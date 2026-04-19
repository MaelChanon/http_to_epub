mod files;

use std::io::Write;
use zip::write::SimpleFileOptions;
use crate::utils::build_client;
use crate::builder::base::BuildParams;
use uuid::Uuid;
use chrono;
use futures::StreamExt;

const PAGE_ENCODE_CONCURRENCY: usize = 10;

pub async fn build(param: &BuildParams, output_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    let uuid = Uuid::new_v4();
    let uuid2 = Uuid::new_v4();
    let modif_date = chrono::offset::Local::now();
    
    zip.start_file("mimetype", SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored))?;
    zip.write_all(b"application/epub+zip")?;

    zip.start_file("META-INF/container.xml", options)?;
    zip.write_all(files::container::generate().as_bytes())?;

    zip.start_file("kte-css/stylehacks.css", options)?;
    zip.write_all(files::stylehacks::generate().as_bytes())?;

    zip.start_file("OEBPS/Text/style.css", options)?;
    zip.write_all(files::style::generate().as_bytes())?;

    zip.start_file("OEBPS/Images/cover.jpg", options)?;
    zip.write_all(&param.cover)?;

    let mut item_manifest = String::new();
    let mut item_spine = String::new();
    let mut toc_content = String::new();
    let mut nav_toc_content = String::new();
    let mut nav_pagelist_content = String::new();

    let client = build_client();

    for (vol_idx, chapter) in param.chapters.iter().enumerate() {
        let vol_num = vol_idx + 1;
        let vol_str = format!("volume-{:04}", vol_num);

        toc_content.push_str(&files::toc_ncx::generate_entry(&param.manga_info.tag(), vol_num));
        nav_toc_content.push_str(&files::nav::generate_entry(&param.manga_info.tag(), vol_num));
        nav_pagelist_content.push_str(&files::nav::generate_entry(&param.manga_info.tag(), vol_num));

        let (width, height, split) = (param.width, param.height, param.split_double_page);
        let mut encoded_stream = chapter.fetch_pages(&client)
            .map(|page_bytes| tokio::task::spawn_blocking(move || {
                crate::builder::base::encode_image(page_bytes.to_vec(), width, height, split)
            }))
            .buffered(PAGE_ENCODE_CONCURRENCY);

        let mut page_idx = 0;
        while let Some(result) = encoded_stream.next().await {
            let encoded_pages = result.unwrap_or(Ok(vec![])).unwrap_or(vec![]);

            for (encoded, _, _) in encoded_pages {
                let name = format!("kcc-{:04}-kcc", page_idx);

                zip.start_file(format!("OEBPS/Images/{}/{}/{}.jpg", param.manga_info.tag(), vol_str, name), options)?;
                zip.write_all(&encoded)?;

                zip.start_file(format!("OEBPS/Text/{}/{}/{}.xhtml", param.manga_info.tag(), vol_str, name), options)?;
                zip.write_all(files::page::generate(&name, param.width, param.height, &param.manga_info.tag(), &vol_str).as_bytes())?;

                let xhtml_id = format!("page_Images_{}_{}_{}", param.manga_info.tag(), vol_str, name);
                item_manifest.push_str(&files::content_opf::generate_manifest_entry(&xhtml_id, &param.manga_info.tag(), &vol_str, &name));
                item_spine.push_str(&files::content_opf::generate_spine_entry(&xhtml_id));
                println!("page {} vol {}", page_idx, vol_idx);
                page_idx += 1;
            }
        }
    }

    item_manifest.push_str(files::content_opf::generate_manifest_footer());

    zip.start_file("OEBPS/toc.ncx", options)?;
    zip.write_all(files::toc_ncx::generate(&uuid.to_string(), &param.manga_info.name(), &toc_content).as_bytes())?;

    zip.start_file("OEBPS/nav.xhtml", options)?;
    zip.write_all(files::nav::generate(&param.manga_info.name(), &nav_toc_content, &nav_pagelist_content).as_bytes())?;

    zip.start_file("OEBPS/content.opf", options)?;
    zip.write_all(files::content_opf::generate(
        &param.manga_info.name(),
        &param.creator,
        &uuid.to_string(),
        &uuid2.to_string(),
        &param.lang,
        &item_manifest,
        &item_spine,
        &modif_date.to_string(),
    ).as_bytes())?;

    zip.finish()?;
    Ok(())
}
