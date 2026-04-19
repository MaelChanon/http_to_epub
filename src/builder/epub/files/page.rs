pub fn generate(name: &str, w: u32, h: u32, tag: &str, vol_str: &str) -> String {
    format!(
        "<?xml version='1.0' encoding='utf-8'?>\
\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\
\n  <head>\
\n    <title>{name}</title>\
\n    <link href=\"../../style.css\" type=\"text/css\" rel=\"stylesheet\"/>\
\n    <meta name=\"viewport\" content=\"width={w}, height={h}\"/>\
\n    <meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\"/>\
\n    <link rel=\"stylesheet\" href=\"../../../../kte-css/stylehacks.css\"/>\
\n  </head>\
\n  <body style=\"background-color:#000000;\"><div style=\"text-align:center;top:0.0%;\"><span>\
\n    <img width=\"{w}\" height=\"{h}\" src=\"../../../Images/{tag}/{vol_str}/{name}.jpg\"/>\
\n  </span></div></body></html>",
    )
}
