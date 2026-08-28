pub fn generate_manifest_entry(xhtml_id: &str, tag: &str, vol_str: &str, name: &str) -> String {
    format!("<item id=\"{xhtml_id}\" href=\"Text/{tag}/{vol_str}/{name}.xhtml\" media-type=\"application/xhtml+xml\"/>\n")
}

pub fn generate_manifest_footer() -> &'static str {
    "\n<item id=\"nav\" href=\"nav.xhtml\" properties=\"nav\" media-type=\"application/xhtml+xml\"/>\
\n<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\
\n<item id=\"css\" href=\"Text/style.css\" media-type=\"text/css\"/>\
\n<item id=\"cover\" href=\"Images/cover.jpg\" media-type=\"image/jpeg\" properties=\"cover-image\"/>\
\n<item id=\"id1\" href=\"../kte-css/stylehacks.css\" media-type=\"text/css\"/>"
}

pub enum SpineSpread {
    Left,
    Right,
    Center,
}

impl SpineSpread {
    fn as_property(&self) -> &'static str {
        match self {
            SpineSpread::Left => "page-spread-left",
            SpineSpread::Right => "page-spread-right",
            SpineSpread::Center => "rendition:page-spread-center",
        }
    }
}

pub fn generate_spine_entry(xhtml_id: &str, spread: Option<SpineSpread>) -> String {
    match spread {
        Some(spread) => format!("<itemref idref=\"{xhtml_id}\" properties=\"{}\"/>\n", spread.as_property()),
        None => format!("<itemref idref=\"{xhtml_id}\"/>\n"),
    }
}

pub fn generate(
    title: &str,
    creator: &str,
    uuid: &str,
    uuid2: &str,
    lang: &str,
    manifest: &str,
    spine: &str,
    modif_date: &str,
) -> String {
    format!(
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
\n<meta property=\"rendition:spread\">landscape</meta>\
\n<meta property=\"rendition:layout\">pre-paginated</meta>\
\n<meta refines=\"#id-1\" property=\"role\" scheme=\"marc:relators\">aut</meta>\
\n<meta refines=\"#id-1\" property=\"file-as\">{creator}</meta>\
\n</metadata>\
\n<manifest>\
\n{manifest}</manifest>\
\n<spine page-progression-direction=\"rtl\" toc=\"ncx\">\
\n{spine}</spine>\
\n</package>",
    )
}
