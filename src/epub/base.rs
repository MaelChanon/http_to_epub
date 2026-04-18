use std::io::{Cursor, Write};
use bytes::Bytes;
use zip::{write::SimpleFileOptions};
use crate::provider::{Chapter, MangaInfo, base::TChapter};
use crate::utils::build_client;
use uuid::Uuid;
use image::{ImageFormat, imageops::FilterType};
use chrono;
use futures::StreamExt;

async fn encode_image(bytes: Vec<u8>, target_width: u32, target_height: u32) -> Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error + Send + Sync>> {
    tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error + Send + Sync>> {
        let img = image::load_from_memory(&bytes)?;
        let (w, h) = (img.width(), img.height());
        let (tw, th) = if w > h { (target_height, target_width) } else { (target_width, target_height) };
        let resized = img.resize(tw, th, FilterType::Triangle);
        let (rw, rh) = (resized.width(), resized.height());
        let mut output = Vec::new();
        resized.write_to(&mut Cursor::new(&mut output), ImageFormat::Jpeg)?;
        Ok((output, rw, rh))
    }).await?
}

pub struct EpubParams {
    pub height: u32,
    pub width: u32,
    pub cover: Bytes,
    pub chapters: Vec<Chapter>,
    pub manga_info: MangaInfo,
    pub tablet_model: String,
    pub creator: String,
    pub lang: String,
}

pub async fn build(param: &EpubParams, output_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    let uuid = Uuid::new_v4();
    let uuid2 = Uuid::new_v4();
    let modif_date =  chrono::offset::Local::now();

    zip.start_file(
        "mimetype",
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )?;
    zip.write_all(b"application/epub+zip")?;

    zip.start_file("META-INF/container.xml", options)?;
    zip.write_all(
        br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles>
<rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
</rootfiles>
</container>"#,
    )?;

    zip.start_file("kte-css/stylehacks.css", options)?;
    zip.write_all(b"div#book-inner {\n\tmargin-top: 0;\n\tmargin-bottom: 0;\n}\n")?;

    zip.start_file("OEBPS/Text/style.css", options)?;
    zip.write_all(b"@page {\nmargin: 0;\n}\nbody {\ndisplay: block;\nmargin: 0;\npadding: 0;\n}\n")?;

    zip.start_file("OEBPS/Images/cover.jpg", options)?;
    zip.write_all(&param.cover)?;

    let mut item_manifest = String::new();
    let mut item_spine = String::new();
    let mut toc_content = String::new();
    let mut nav_toc_content = String::new();
    let mut nav_pagelist_content = String::new();

    let num_chapters = param.chapters.len();
    let client = build_client();

    for (vol_idx, chapter) in param.chapters.iter().enumerate() {
        let vol_num = vol_idx + 1;
        let vol_str = format!("volume-{:04}", vol_num);

        toc_content.push_str(&format!(
            "<navPoint id=\"Text_{}_volume-{:04}\" playOrder=\"{}\"><navLabel><text>Volume {}</text></navLabel><content src=\"Text/{}/volume-{:04}/kcc-0000-kcc.xhtml\"/></navPoint>\n",
            param.manga_info.tag, vol_num, vol_num, vol_num, param.manga_info.tag, vol_num
        ));
        let toc_kobo_id = 4 + vol_idx * 2;
        let pagelist_kobo_id = 4 + num_chapters * 2 + 6 + vol_idx * 2;
        nav_toc_content.push_str(&format!(
            "<li><a href=\"Text/{tag}/volume-{vol_num:04}/kcc-0000-kcc.xhtml\"><span class=\"koboSpan\" id=\"kobo.{kobo_id}.1\">Volume {vol_num}</span></a></li>\n",
            tag = param.manga_info.tag,
            vol_num = vol_num,
            kobo_id = toc_kobo_id,
        ));
        nav_pagelist_content.push_str(&format!(
            "<li><a href=\"Text/{tag}/volume-{vol_num:04}/kcc-0000-kcc.xhtml\"><span class=\"koboSpan\" id=\"kobo.{kobo_id}.1\">Volume {vol_num}</span></a></li>\n",
            tag = param.manga_info.tag,
            vol_num = vol_num,
            kobo_id = pagelist_kobo_id,
        ));

        let mut pages = chapter.fetch_pages(&client);
        let mut page_idx = 0;
        while let Some(page_bytes) = pages.next().await {
            let name = format!("kcc-{:04}-kcc", page_idx);
            let img_path = format!("OEBPS/Images/{}/{}/{}.jpg", param.manga_info.tag, vol_str, name);

            let (encoded, img_w, img_h) = encode_image(page_bytes.to_vec(), param.width, param.height)
                .await
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
            zip.start_file(&img_path, options)?;
            zip.write_all(&encoded)?;

            let xhtml_path = format!("OEBPS/Text/{}/{}/{}.xhtml", param.manga_info.tag, vol_str, name);
            let (vp_w, vp_h) = (img_w, img_h);
            let xhtml = format!(
                "<?xml version='1.0' encoding='utf-8'?>\
\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\
\n  <head>\
\n    <title>{name}</title>\
\n    <link href=\"../../style.css\" type=\"text/css\" rel=\"stylesheet\"/>\
\n    <meta name=\"viewport\" content=\"width={w}, height={h}\"/>\
\n    <meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\"/>\
\n    <link rel=\"stylesheet\" href=\"../../../../kte-css/stylehacks.css\"/>\
\n  </head>\
\n  <body style=\"background-color:#000000;\"><div style=\"text-align:center;top:0.0%;\"><span id=\"kobo.3.1\" class=\"koboSpan\"><img width=\"{w}\" height=\"{h}\" src=\"../../../Images/{tag}/{vol_str}/{name}.jpg\"/></span></div></body></html>",
                name = name,
                w = param.width,
                h = param.height,
                tag = param.manga_info.tag,
                vol_str = vol_str,
            );
            zip.start_file(&xhtml_path, options)?;
            zip.write_all(xhtml.as_bytes())?;

            let xhtml_id = format!("page_Images_{}_{}_{}", param.manga_info.tag, vol_str, name);
            item_manifest.push_str(&format!(
                "<item id=\"{xhtml_id}\" href=\"Text/{tag}/{vol_str}/{name}.xhtml\" media-type=\"application/xhtml+xml\"/>\n",
                xhtml_id = xhtml_id,
                tag = param.manga_info.tag,
                vol_str = vol_str,
                name = name,
            ));
                println!("finito {} vol {}",page_idx, vol_idx);
            item_spine.push_str(&format!("<itemref idref=\"{}\"/>\n", xhtml_id));
            page_idx += 1;
        }
    }
    item_manifest.push_str(format!("\
\n<item id=\"nav\" href=\"nav.xhtml\" properties=\"nav\" media-type=\"application/xhtml+xml\"/>\
\n<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\
\n<item id=\"css\" href=\"Text/style.css\" media-type=\"text/css\"/>\
\n<item id=\"cover\" href=\"Images/cover.jpg\" media-type=\"image/jpeg\" properties=\"cover-image\"/>\
\n<item id=\"id1\" href=\"../kte-css/stylehacks.css\" media-type=\"text/css\"/>").as_str());

    zip.start_file("OEBPS/toc.ncx", options)?;
    zip.write_all(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
\n<ncx version=\"2005-1\" xml:lang=\"en-US\" xmlns=\"http://www.daisy.org/z3986/2005/ncx/\">\
\n<head>\
\n<meta name=\"dtb:uid\" content=\"urn:uuid:{uuid}\"/>\
\n<meta name=\"dtb:depth\" content=\"1\"/>\
\n<meta name=\"dtb:totalPageCount\" content=\"0\"/>\
\n<meta name=\"dtb:maxPageNumber\" content=\"0\"/>\
\n<meta name=\"generated\" content=\"true\"/>\
\n</head>\
\n<docTitle><text>{title}</text></docTitle>\
\n<navMap>\n{toc}</navMap>\
\n</ncx>",
        uuid = uuid,
        title = param.manga_info.name,
        toc = toc_content,
    ).as_bytes())?;

    zip.start_file("OEBPS/nav.xhtml", options)?;
    zip.write_all(format!(
        "<?xml version='1.0' encoding='utf-8'?>\
\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\
\n<head><title>{title}</title>\
\n<meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\"/>\
\n<link rel=\"stylesheet\" href=\"../kte-css/stylehacks.css\"/>\
\n</head>\
\n<body><div id=\"book-columns\"><div id=\"book-inner\">\
\n<nav epub:type=\"toc\" id=\"toc\"><ol>{nav_toc}</ol></nav>\
\n<nav epub:type=\"page-list\"><ol>{nav_pagelist}</ol></nav>\
\n</div></div></body>\
\n</html>",
        title = param.manga_info.name,
        nav_toc = nav_toc_content,
        nav_pagelist = nav_pagelist_content,
    ).as_bytes())?;

    zip.start_file("OEBPS/content.opf", options)?;
    zip.write_all(format!(
        "<?xml version='1.0' encoding='utf-8'?>\
\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"BookID\">\
\n<metadata xmlns:opf=\"http://www.idpf.org/2007/opf\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
\n<dc:title id=\"id\">{title}</dc:title>\
\n<dc:creator id=\"id-1\">{creator}</dc:creator>\
\n<dc:identifier>uuid:{uuid2}</dc:identifier>\
\n<dc:identifier id=\"BookID\">urn:uuid:{uuid}</dc:identifier>\
\n<dc:language>{lang}</dc:language>\
\n<meta property=\"dcterms:modified\">{modif_date}</meta>\
\n<meta refines=\"#id\" property=\"title-type\">main</meta>\
\n<meta refines=\"#id\" property=\"file-as\">{title}</meta>\
\n<meta name=\"cover\" content=\"cover\"/>\
\n<meta property=\"rendition:orientation\">portrait</meta>\
\n<meta property=\"rendition:spread\">portrait</meta>\
\n<meta property=\"rendition:layout\">pre-paginated</meta>\
\n<meta refines=\"#id-1\" property=\"role\" scheme=\"marc:relators\">aut</meta>\
\n<meta refines=\"#id-1\" property=\"file-as\">{creator}</meta>\
\n</metadata>\
\n<manifest>\
\n{manifest}</manifest>\
\n<spine page-progression-direction=\"rtl\" toc=\"ncx\">\
\n{spine}</spine>\
\n</package>",
        title = param.manga_info.name,
        creator = param.creator,
        uuid = uuid,
        uuid2 = uuid2,
        lang = param.lang,
        manifest = item_manifest,
        spine = item_spine,
        modif_date = modif_date.to_string(),
    ).as_bytes())?;

    zip.finish()?;
    Ok(())
}
