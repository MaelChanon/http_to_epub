pub fn generate_entry(tag: &str, vol_num: usize) -> String {
    format!(
        "<navPoint id=\"Text_{tag}_volume-{vol_num:04}\" playOrder=\"{vol_num}\"><navLabel><text>Volume {vol_num}</text></navLabel><content src=\"Text/{tag}/volume-{vol_num:04}/kcc-0000-kcc.xhtml\"/></navPoint>\n"
    )
}

pub fn generate(uuid: &str, title: &str, toc: &str) -> String {
    format!(
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
    )
}
