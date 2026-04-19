pub fn generate_entry(tag: &str, vol_num: usize) -> String {
    format!(
        "<li><a href=\"Text/{tag}/volume-{vol_num:04}/kcc-0000-kcc.xhtml\"><span>Volume {vol_num}</span></a></li>\n"
    )
}

pub fn generate(title: &str, nav_toc: &str, nav_pagelist: &str) -> String {
    format!(
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
    )
}
