use super::{CfChapter, Provider, TMangaInfo};
use crate::utils::cloudflare::{CfError, CfSession, Clearance, widen};
use futures::stream::{self, StreamExt};
use reqwest::Url;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

const MANGA_FETCH_CONCURRENCY: usize = 10;
const SCAN_BADGES: &str = ".as-badge-scan, .as-badge-webtoon";
const SINGLE_PAGES_SUFFIX: &str = " pp";

type AnyError = Box<dyn std::error::Error>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AnimeSamaManga {
    pub tag: String,
    pub name: String,
    pub cover_url: String,
    pub chapter_count: usize,
    /// Reader page under `/catalogue/`: `<slug>/<dir>/<lang>`.
    pub scan_path: String,
    /// The reader's `#titreOeuvre`, as the scan endpoints expect it.
    pub scan_name: String,
}

impl TMangaInfo for AnimeSamaManga {
    fn tag(&self) -> &str { &self.tag }
    fn name(&self) -> &str { &self.name }
    fn cover_url(&self) -> &str { &self.cover_url }
}

pub struct AnimeSama {
    url: String,
    session: Arc<CfSession>,
}

impl AnimeSama {
    /// `clearance` is only needed while Cloudflare is challenging; start with `None`
    /// and switch to `Some` once calls fail with [`CfError::ClearanceRequired`]. Does
    /// no I/O, so a stale clearance only shows up on the first request.
    pub fn new(url: &str, clearance: Option<&Clearance>) -> Result<AnimeSama, CfError> {
        let url = url.trim_end_matches('/').to_string();
        let session = match clearance {
            Some(clearance) => CfSession::with_clearance(&url, clearance)?,
            None => CfSession::new(&url)?,
        };
        Ok(AnimeSama { url, session })
    }

    /// Over a session built by the caller, e.g. with [`CfSession::with_options`] to
    /// add a proxy (anime-sama bans some hosting providers' IPs outright). The
    /// session must target the same origin as `url`.
    pub fn with_session(url: &str, session: Arc<CfSession>) -> AnimeSama {
        let url = url.trim_end_matches('/').to_string();
        AnimeSama { url, session }
    }

    pub fn session(&self) -> &Arc<CfSession> {
        &self.session
    }

    async fn scan_versions(&self, slug: &str) -> Result<Vec<String>, AnyError> {
        let url = format!("{}/catalogue/{}/", self.url, slug);
        let body = self.session.get_text(&url).await.map_err(widen)?;
        Ok(scan_panels(&body))
    }

    async fn manga_for(&self, slug: &str, version: &str, tag: String) -> Result<AnimeSamaManga, AnyError> {
        let scan_path = format!("{}/{}", slug, version);
        let url = format!("{}/catalogue/{}/", self.url, scan_path);
        let body = self.session.get_text(&url).await.map_err(widen)?;

        let (scan_name, name, cover_url) = {
            let document = Html::parse_document(&body);
            let title_selector = Selector::parse("#titreOeuvre").expect("invalid title selector");
            let cover_selector = Selector::parse("#imgOeuvre").expect("invalid cover selector");

            let title = document
                .select(&title_selector)
                .next()
                .ok_or_else(|| format!("no #titreOeuvre on {}", url))?;
            // the site's own script reads `innerHTML`, untrimmed, which is what
            // scraper's `inner_html` reproduces
            let scan_name = title.inner_html();
            let name = title.text().collect::<String>().trim().to_string();

            let cover_url = document
                .select(&cover_selector)
                .next()
                .and_then(|el| el.value().attr("src"))
                .unwrap_or_default()
                .to_string();
            (scan_name, name, cover_url)
        };

        let chapter_count = self.chapter_sizes(&scan_name).await?.len();

        Ok(AnimeSamaManga {
            tag,
            name,
            cover_url,
            chapter_count,
            scan_path,
            scan_name,
        })
    }

    async fn chapter_sizes(&self, name: &str) -> Result<BTreeMap<u32, usize>, AnyError> {
        let mut url = Url::parse(&format!("{}/s2/scans/get_nb_chap_et_img.php", self.url))?;
        url.query_pairs_mut().append_pair("oeuvre", name);
        let body = self.session.get_text(url.as_str()).await.map_err(widen)?;
        parse_chapter_sizes(&body)
    }

    fn page_url(&self, name: &str, chapter: u32, page: usize) -> Result<String, AnyError> {
        let mut url = Url::parse(&self.url)?;
        url.path_segments_mut()
            .map_err(|_| format!("{} cannot be a base url", self.url))?
            .pop_if_empty()
            .extend(["s2", "scans", name, &chapter.to_string(), &format!("{}.jpg", page)]);
        Ok(url.into())
    }

    async fn scan_slugs(&self) -> Result<Vec<String>, AnyError> {
        let card_selector = Selector::parse(".as-carte-oeuvre").expect("invalid card selector");
        let badge_selector = Selector::parse(SCAN_BADGES).expect("invalid badge selector");
        let mut slugs: Vec<String> = Vec::new();
        let mut page = 1;

        loop {
            let url = format!("{}/catalogue/?type%5B%5D=Scans&page={}", self.url, page);
            let body = self.session.get_text(&url).await.map_err(widen)?;
            let document = Html::parse_document(&body);

            let page_slugs: Vec<String> = document
                .select(&card_selector)
                .filter(|card| card.select(&badge_selector).next().is_some())
                .filter_map(|card| card.value().attr("data-url"))
                .filter_map(slug_from_url)
                .collect();

            let before = slugs.len();
            for slug in page_slugs {
                if !slugs.contains(&slug) {
                    slugs.push(slug);
                }
            }
            if slugs.len() == before {
                break;
            }
            page += 1;
        }
        Ok(slugs)
    }
}

impl Provider for AnimeSama {
    type MangaInfo = AnimeSamaManga;
    type Chapter = CfChapter;

    async fn get_manga_info(&self, tag: &str) -> Result<AnimeSamaManga, AnyError> {
        let (slug, version) = split_tag(tag);
        let version = match version {
            Some(version) => version,
            None => self
                .scan_versions(slug)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| format!("{} has no scans (anime only?)", slug))?,
        };
        self.manga_for(slug, &version, tag.to_string()).await
    }

    async fn get_all_mangas(&self) -> Result<Vec<AnimeSamaManga>, AnyError> {
        let slugs = self.scan_slugs().await?;
        println!("found {} works with scans", slugs.len());

        let results: Vec<(String, Result<Vec<AnimeSamaManga>, AnyError>)> = stream::iter(slugs)
            .map(|slug| async move {
                let mangas = async {
                    let versions = self.scan_versions(&slug).await?;
                    let mut mangas = Vec::with_capacity(versions.len());
                    for (idx, version) in versions.iter().enumerate() {
                        let tag = tag_for(&slug, version, idx == 0);
                        mangas.push(self.manga_for(&slug, version, tag).await?);
                    }
                    Ok::<_, AnyError>(mangas)
                }
                .await;
                (slug, mangas)
            })
            .buffer_unordered(MANGA_FETCH_CONCURRENCY)
            .collect()
            .await;

        let mut all = Vec::new();
        for (slug, result) in results {
            match result {
                Ok(mangas) => all.extend(mangas.into_iter().filter(|m| m.chapter_count > 0)),
                Err(e) if needs_clearance(&e) => return Err(e),
                Err(e) => eprintln!("skipping {}: {}", slug, e),
            }
        }
        Ok(all)
    }

    async fn get_manga_chapters(&self, manga_info: &AnimeSamaManga) -> Result<Vec<CfChapter>, AnyError> {
        let main_name = manga_info.scan_name.as_str();
        let main = self.chapter_sizes(main_name).await?;
        if main.is_empty() {
            return Err(format!("no scans hosted for {:?}", main_name).into());
        }

        let single_name = format!("{}{}", main_name, SINGLE_PAGES_SUFFIX);
        let single = match self.chapter_sizes(&single_name).await {
            Ok(sizes) => sizes,
            Err(e) if needs_clearance(&e) => return Err(e),
            // optional folder: anything else just means "use the main one"
            Err(_) => BTreeMap::new(),
        };

        main.iter()
            .enumerate()
            .map(|(idx, (&chapter, &size))| {
                let (name, size) = match single.get(&chapter) {
                    Some(&single_size) if single_size > 0 => (single_name.as_str(), single_size),
                    _ => (main_name, size),
                };
                let pages = (1..=size)
                    .map(|page| self.page_url(name, chapter, page))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(CfChapter::new(pages, idx, Arc::clone(&self.session)))
            })
            .collect()
    }
}

fn needs_clearance(e: &AnyError) -> bool {
    e.downcast_ref::<CfError>().is_some_and(CfError::needs_clearance)
}

fn split_tag(tag: &str) -> (&str, Option<String>) {
    let mut parts = tag.rsplitn(3, '.');
    if let (Some(lang), Some(dir), Some(slug)) = (parts.next(), parts.next(), parts.next()) {
        if dir.starts_with("scan") && !lang.is_empty() && !slug.is_empty() {
            return (slug, Some(format!("{}/{}", dir, lang)));
        }
    }
    (tag, None)
}

fn tag_for(slug: &str, version: &str, first: bool) -> String {
    if first {
        slug.to_string()
    } else {
        format!("{}.{}", slug, version.replace('/', "."))
    }
}

fn slug_from_url(href: &str) -> Option<String> {
    let slug = href.trim_end_matches('/').rsplit('/').next()?;
    (!slug.is_empty()).then(|| slug.to_string())
}

fn parse_chapter_sizes(body: &str) -> Result<BTreeMap<u32, usize>, AnyError> {
    let value: serde_json::Value = serde_json::from_str(body)?;
    let object = value.as_object().ok_or("chapter list is not a JSON object")?;
    if object.contains_key("error") {
        return Ok(BTreeMap::new());
    }
    Ok(object
        .iter()
        .filter_map(|(chapter, size)| {
            let chapter = chapter.parse().ok()?;
            let size = size
                .as_u64()
                .or_else(|| size.as_str()?.parse().ok())?;
            Some((chapter, size as usize))
        })
        .collect())
}

fn scan_panels(html: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    let script_selector = Selector::parse("script").expect("invalid script selector");
    let mut panels = Vec::new();

    for script in document.select(&script_selector) {
        let source = script.text().collect::<String>();
        if !source.contains("panneauScan") {
            continue;
        }
        let code = strip_js_comments(&source);
        let mut rest = code.as_str();
        while let Some(at) = rest.find("panneauScan") {
            rest = &rest[at + "panneauScan".len()..];
            if let Some(path) = call_second_string_arg(rest) {
                let path = path.trim_matches('/').to_string();
                if !path.is_empty() && !panels.contains(&path) {
                    panels.push(path);
                }
            }
        }
    }
    panels
}

fn call_second_string_arg(s: &str) -> Option<String> {
    let s = s.trim_start().strip_prefix('(')?;
    let (_, s) = string_literal(s.trim_start())?;
    let s = s.trim_start().strip_prefix(',')?;
    let (second, _) = string_literal(s.trim_start())?;
    Some(second)
}

fn string_literal(s: &str) -> Option<(String, &str)> {
    let quote = s.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let mut value = String::new();
    let mut chars = s[1..].char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => value.push(chars.next()?.1),
            c if c == quote => return Some((value, &s[1 + i + 1..])),
            c => value.push(c),
        }
    }
    None
}

/// Removes `/* */`, `//` and `<!--` comments, leaving string literals alone.
fn strip_js_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut quote: Option<u8> = None;
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            out.push(c);
            if c == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }

        let rest = &bytes[i..];
        if rest.starts_with(b"/*") {
            i = match src[i + 2..].find("*/") {
                Some(end) => i + 2 + end + 2,
                None => bytes.len(),
            };
            out.push(b' ');
        } else if rest.starts_with(b"//") || rest.starts_with(b"<!--") {
            i = match src[i..].find('\n') {
                Some(end) => i + end,
                None => bytes.len(),
            };
        } else {
            if matches!(c, b'"' | b'\'' | b'`') {
                quote = Some(c);
            }
            out.push(c);
            i += 1;
        }
    }
    // only whole ASCII-delimited ranges were dropped, so this stays valid UTF-8
    String::from_utf8(out).expect("comment stripping split a UTF-8 sequence")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::TChapter;

    /// Trimmed copy of the One Piece page: the doc comment, one version hidden by the
    /// team, and the function declarations must all be ignored.
    const WORK_PAGE: &str = r#"<html><body>
<script>
/* Cartes des saisons / scans :
   panneauAnime("nom", "saison1/vostfr"); panneauScan("nom", "scan/vf"); */
function panneauScan(nom, url) { oePanneau(nom, url, true); }
</script>
<script>
    panneauAnime("Saison 1", "saison1/vostfr");
</script>
<script>
    /*panneauScan("nom", "url"); -> on en met autant qu'on veut à la ligne*/
    panneauScan("Scans (couleur)", "scan/vf");
    // panneauScan("Scans (ancienne trad)", "scan_old/vf");
    <!--panneauScan("Cachés", "scan_cache/vf");-->
    panneauScan('Scans (noir et blanc)', "scan_noir-et-blanc/vf/");
    var x = "http://not-a-comment"; panneauScan("Après URL", "scan_apres/vf");
</script>
</body></html>"#;

    #[test]
    fn finds_visible_scan_panels_only() {
        assert_eq!(
            scan_panels(WORK_PAGE),
            vec!["scan/vf", "scan_noir-et-blanc/vf", "scan_apres/vf"]
        );
    }

    #[test]
    fn anime_only_page_has_no_panels() {
        let html = r#"<script>panneauAnime("Saison 1", "saison1/vostfr");
        /*panneauScan("nom", "url");*/</script>"#;
        assert!(scan_panels(html).is_empty());
    }

    #[test]
    fn comments_inside_strings_survive() {
        assert_eq!(
            strip_js_comments(r#"a("/* x */", '//y'); /* gone */ b(); // gone"#),
            r#"a("/* x */", '//y');   b(); "#
        );
        // non-ASCII text next to comments must not break UTF-8
        assert_eq!(strip_js_comments("é/*à*/œ"), "é œ");
    }

    #[test]
    fn tags_round_trip() {
        assert_eq!(split_tag("one-piece"), ("one-piece", None));
        let tag = tag_for("one-piece", "scan_noir-et-blanc/vf", false);
        assert_eq!(tag, "one-piece.scan_noir-et-blanc.vf");
        assert_eq!(split_tag(&tag), ("one-piece", Some("scan_noir-et-blanc/vf".to_string())));
        assert_eq!(tag_for("one-piece", "scan/vf", true), "one-piece");
    }

    #[test]
    fn dotted_slugs_are_not_mistaken_for_versions() {
        let slug = "2.43-seiin-koukou-danshi-volley-bu";
        assert_eq!(split_tag(slug), (slug, None));
        assert_eq!(
            split_tag("2.43-seiin-koukou-danshi-volley-bu.scan.vf"),
            (slug, Some("scan/vf".to_string()))
        );
    }

    #[test]
    fn chapter_sizes_are_sorted_numerically() {
        let sizes = parse_chapter_sizes(r#"{"10": 19, "2": "25", "1": 53, "extra": 4}"#).unwrap();
        assert_eq!(sizes.into_iter().collect::<Vec<_>>(), vec![(1, 53), (2, 25), (10, 19)]);
    }

    #[test]
    fn unknown_work_has_no_chapters() {
        assert!(parse_chapter_sizes(r#"{"error":"Oeuvre '07 Ghost' not found"}"#).unwrap().is_empty());
    }

    #[test]
    fn page_urls_encode_the_work_name() {
        let provider = AnimeSama::new("https://anime-sama.to/", None).unwrap();
        assert_eq!(
            provider.page_url("One Piece Couleur pp", 3, 12).unwrap(),
            "https://anime-sama.to/s2/scans/One%20Piece%20Couleur%20pp/3/12.jpg"
        );
    }

    /// Fake origin behaving like Cloudflare with the challenge on: a `cf-mitigated`
    /// 403 unless the request carries the expected cookie *and* User-Agent. Serves
    /// `n` connections, then returns every request head it saw.
    async fn fake_challenging_origin(n: usize) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let mut seen = Vec::new();
            for _ in 0..n {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut head = Vec::new();
                let mut buf = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    let read = socket.read(&mut buf).await.unwrap();
                    if read == 0 { break; }
                    head.extend_from_slice(&buf[..read]);
                }
                let head = String::from_utf8_lossy(&head).to_lowercase();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let cleared = head.contains("\r\ncookie: cf_clearance=good-token\r\n")
                    && head.contains("\r\nuser-agent: browser/1.0\r\n");

                let (status, extra, body): (&str, &str, Vec<u8>) = if !cleared {
                    ("403 Forbidden", "cf-mitigated: challenge\r\nserver: cloudflare\r\n", b"challenge".to_vec())
                } else if path.starts_with("/catalogue/x/scan/vf/") {
                    ("200 OK", "", br#"<h1 id="titreOeuvre">X Name</h1><img id="imgOeuvre" src="https://cdn.example/x.jpg">"#.to_vec())
                } else if path.contains("oeuvre=x+name+pp") {
                    ("200 OK", "", br#"{"error":"not found"}"#.to_vec())
                } else if path.contains("oeuvre=x+name") {
                    ("200 OK", "", br#"{"1": 2}"#.to_vec())
                } else if path.starts_with("/s2/scans/x%20name/1/") {
                    ("200 OK", "", path.into_bytes())
                } else {
                    ("404 Not Found", "", Vec::new())
                };
                let response = format!(
                    "HTTP/1.1 {}\r\n{}content-length: {}\r\nconnection: close\r\n\r\n",
                    status, extra, body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
                seen.push(head);
            }
            seen
        });
        (base, handle)
    }

    #[tokio::test]
    async fn without_clearance_the_challenge_reaches_the_caller() {
        let (base, server) = fake_challenging_origin(1).await;
        let provider = AnimeSama::new(&base, None).unwrap();

        let err = provider.get_manga_info("x.scan.vf").await.unwrap_err();
        assert!(needs_clearance(&err), "unexpected error: {}", err);
        assert!(err.to_string().contains(crate::utils::cloudflare::CLEARANCE_REQUIRED_MARKER));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn clearance_is_sent_on_every_request_pages_included() {
        // reader page, chapter list, " pp" chapter list, then the chapter's 2 pages
        let (base, server) = fake_challenging_origin(6).await;
        let clearance = Clearance::from_token("good-token", "browser/1.0");
        let provider = AnimeSama::new(&base, Some(&clearance)).unwrap();

        let info = provider.get_manga_info("x.scan.vf").await.unwrap();
        assert_eq!((info.name.as_str(), info.scan_name.as_str(), info.chapter_count), ("X Name", "X Name", 1));
        assert_eq!(info.cover_url, "https://cdn.example/x.jpg");

        let chapters = provider.get_manga_chapters(&info).await.unwrap();
        assert_eq!(chapters.len(), 1);
        let pages: Vec<bytes::Bytes> = chapters[0]
            .fetch_pages(&reqwest::Client::new())
            .collect()
            .await;
        assert_eq!(pages, vec![
            bytes::Bytes::from("/s2/scans/x%20name/1/1.jpg"),
            bytes::Bytes::from("/s2/scans/x%20name/1/2.jpg"),
        ]);

        let seen = server.await.unwrap();
        assert_eq!(seen.len(), 6);
        assert!(seen.iter().all(|head| head.contains("cookie: cf_clearance=good-token")));
    }

    #[test]
    fn slugs_come_from_data_url() {
        assert_eq!(slug_from_url("/catalogue/one-piece/").as_deref(), Some("one-piece"));
        assert_eq!(slug_from_url("/catalogue/one-piece").as_deref(), Some("one-piece"));
    }
}
