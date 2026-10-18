//! Plagiarism check: search distinctive phrases on the web, fetch candidates,
//! measure n-gram overlap, and build citations.

use crate::settings::SearchConfig;
use crate::publik::Credential;
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub url: String,
    pub title: String,
    pub site: String,
    pub matched_words: usize,
    pub matched_pct: f64,
    pub snippet: String,
    pub citations: HashMap<String, String>,
}

const NGRAM: usize = 7;
const QUERY_WORDS: usize = 9;
const MAX_QUERIES: usize = 6;
const MAX_FETCH: usize = 8;
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

fn normalize_words(text: &str) -> Vec<String> {
    text.unicode_words().map(|w| w.to_lowercase()).filter(|w| w.chars().any(|c| c.is_alphanumeric())).collect()
}

fn shingles(words: &[String], n: usize) -> HashSet<String> {
    if words.len() < n {
        return HashSet::new();
    }
    words.windows(n).map(|w| w.join(" ")).collect()
}

/// Pick a handful of distinctive query phrases spread through the document.
pub fn pick_queries(text: &str) -> Vec<String> {
    let sentences: Vec<&str> = text.unicode_sentences().map(|s| s.trim()).filter(|s| s.split_whitespace().count() >= QUERY_WORDS).collect();
    if sentences.is_empty() {
        let w: Vec<&str> = text.split_whitespace().collect();
        if w.len() >= 5 {
            return vec![w.iter().take(QUERY_WORDS).cloned().collect::<Vec<_>>().join(" ")];
        }
        return vec![];
    }
    let step = (sentences.len() as f64 / MAX_QUERIES as f64).ceil().max(1.0) as usize;
    let mut out = Vec::new();
    for s in sentences.iter().step_by(step).take(MAX_QUERIES) {
        let words: Vec<&str> = s.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).collect();
        // skip the first word (often "The"/"I") and take a mid-sentence run
        let start = if words.len() > QUERY_WORDS + 1 { 1 } else { 0 };
        let q: String = words.iter().skip(start).take(QUERY_WORDS).map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')).collect::<Vec<_>>().join(" ");
        if q.split_whitespace().count() >= 5 {
            out.push(q);
        }
    }
    out
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent(USER_AGENT).timeout(Duration::from_secs(20)).redirect(reqwest::redirect::Policy::limited(5)).build()?)
}

async fn search_duckduckgo(c: &reqwest::Client, q: &str) -> Result<Vec<(String, String)>> {
    let res = c.post("https://html.duckduckgo.com/html/").form(&[("q", format!("\"{q}\"")), ("kl", "us-en".into())]).send().await?;
    let html = res.text().await?;
    let doc = scraper::Html::parse_document(&html);
    let sel = scraper::Selector::parse("a.result__a").unwrap();
    let mut out = Vec::new();
    for a in doc.select(&sel) {
        let title: String = a.text().collect::<Vec<_>>().join(" ").trim().to_string();
        if let Some(href) = a.value().attr("href") {
            let url = if let Ok(u) = url::Url::parse(&format!("https:{href}")).or_else(|_| url::Url::parse(href)) {
                u.query_pairs().find(|(k, _)| k == "uddg").map(|(_, v)| v.to_string()).unwrap_or_else(|| u.to_string())
            } else {
                href.to_string()
            };
            if url.starts_with("http") {
                out.push((url, title));
            }
        }
    }
    Ok(out)
}

/// Brave Search's HTML results page answers plain clients and honours quoted phrases.
async fn search_brave_html(c: &reqwest::Client, q: &str) -> Result<Vec<(String, String)>> {
    let res = c
        .get("https://search.brave.com/search")
        .query(&[("q", format!("\"{q}\"")), ("source", "web".into())])
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await?;
    let html = res.text().await?;
    let doc = scraper::Html::parse_document(&html);
    let snip = scraper::Selector::parse(r#"div.snippet[data-type="web"]"#).unwrap();
    let link = scraper::Selector::parse(r#"a[href^="http"]"#).unwrap();
    let title_sel = scraper::Selector::parse(".title").unwrap();
    let mut out = Vec::new();
    for sn in doc.select(&snip) {
        let Some(a) = sn.select(&link).next() else { continue };
        let Some(href) = a.value().attr("href") else { continue };
        let url = strip_fragment(href);
        if url.contains("brave.com") {
            continue;
        }
        let title = sn.select(&title_sel).next().map(|t| t.text().collect::<Vec<_>>().join(" ").trim().to_string()).unwrap_or_default();
        out.push((url, title));
    }
    Ok(out)
}

fn strip_fragment(url: &str) -> String {
    url.split('#').next().unwrap_or(url).to_string()
}

/// Bing's HTML results page. Links are often `bing.com/ck/a?...&u=a1<base64url>` redirects.
async fn search_bing(c: &reqwest::Client, q: &str) -> Result<Vec<(String, String)>> {
    let res = c
        .get("https://www.bing.com/search")
        .query(&[("q", format!("\"{q}\"")), ("setlang", "en".into()), ("count", "10".into())])
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await?;
    let html = res.text().await?;
    let doc = scraper::Html::parse_document(&html);
    let sel = scraper::Selector::parse("li.b_algo h2 a").unwrap();
    let mut out = Vec::new();
    for a in doc.select(&sel) {
        let title: String = a.text().collect::<Vec<_>>().join(" ").trim().to_string();
        let Some(href) = a.value().attr("href") else { continue };
        let url = strip_fragment(&decode_bing_link(href));
        if url.starts_with("http") && !url.contains("bing.com/") {
            out.push((url, title));
        }
    }
    Ok(out)
}

pub fn decode_bing_link(href: &str) -> String {
    if href.contains("bing.com/ck/a") {
        if let Ok(u) = url::Url::parse(href) {
            if let Some((_, v)) = u.query_pairs().find(|(k, _)| k == "u") {
                let b64 = v.trim_start_matches("a1");
                let mut norm: String = b64.replace('-', "+").replace('_', "/");
                while norm.len() % 4 != 0 {
                    norm.push('=');
                }
                if let Some(bytes) = base64_decode(&norm) {
                    if let Ok(s) = String::from_utf8(bytes) {
                        if s.starts_with("http") {
                            return s;
                        }
                    }
                }
            }
        }
    }
    href.to_string()
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        let v = T.iter().position(|&t| t == c)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

async fn search_brave(c: &reqwest::Client, key: &str, q: &str) -> Result<Vec<(String, String)>> {
    let v: Value = c
        .get("https://api.search.brave.com/res/v1/web/search")
        .query(&[("q", format!("\"{q}\"")), ("count", "8".into())])
        .header("Accept", "application/json")
        .header("X-Subscription-Token", key)
        .send()
        .await?
        .json()
        .await?;
    Ok(v["web"]["results"]
        .as_array()
        .map(|a| a.iter().filter_map(|r| Some((r["url"].as_str()?.to_string(), r["title"].as_str().unwrap_or("").to_string()))).collect())
        .unwrap_or_default())
}

fn page_text(html: &str) -> (String, String) {
    let doc = scraper::Html::parse_document(html);
    let title = scraper::Selector::parse("title").ok().and_then(|s| doc.select(&s).next()).map(|t| t.text().collect::<String>().trim().to_string()).unwrap_or_default();
    let body_sel = scraper::Selector::parse("p, h1, h2, h3, li, blockquote, td").unwrap();
    let mut text = String::new();
    for el in doc.select(&body_sel) {
        let t: String = el.text().collect::<Vec<_>>().join(" ");
        let t = t.trim();
        if t.len() > 20 {
            text.push_str(t);
            text.push('\n');
        }
    }
    (title, text)
}

fn site_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_default()
}

pub fn citations(title: &str, url: &str, site: &str) -> HashMap<String, String> {
    let year = chrono::Utc::now().format("%Y").to_string();
    let today_apa = chrono::Utc::now().format("%B %-d, %Y").to_string();
    let today_mla = chrono::Utc::now().format("%-d %b. %Y").to_string();
    let t = if title.is_empty() { "Untitled" } else { title };
    let mut m = HashMap::new();
    m.insert("APA".into(), format!("{site}. ({year}). {t}. Retrieved {today_apa}, from {url}"));
    m.insert("MLA".into(), format!("“{t}.” {site}, {url}. Accessed {today_mla}."));
    m.insert("Chicago".into(), format!("{site}. “{t}.” Accessed {today_mla}. {url}."));
    m
}

/// Where a plagiarism search goes: the user's own Brave key first, then publik API when
/// it is the chosen provider (one published price per search), else the keyless chain.
pub fn search_route(cfg: &SearchConfig, publik_on: bool) -> &'static str {
    if !cfg.brave_key.trim().is_empty() {
        "brave"
    } else if publik_on {
        "publik"
    } else {
        "web"
    }
}

/// Keyless chain: Brave's HTML page honours quoted phrases for plain clients; Bing tends to
/// drop the quotes; DuckDuckGo often serves a bot challenge.
async fn search_keyless(c: &reqwest::Client, q: &str) -> Vec<(String, String)> {
    let mut r = search_brave_html(c, q).await.unwrap_or_default();
    if r.is_empty() {
        r = search_bing(c, q).await.unwrap_or_default();
    }
    if r.is_empty() {
        r = search_duckduckgo(c, q).await.unwrap_or_default();
    }
    r
}

pub async fn check(cfg: &SearchConfig, publik: Option<&Credential>, text: &str) -> Result<Value> {
    let words = normalize_words(text);
    if words.len() < 20 {
        return Ok(json!({"too_short": true, "min_words": 20}));
    }
    let queries = pick_queries(text);
    if queries.is_empty() {
        return Err(anyhow!("Could not form search queries from this text."));
    }
    let c = client()?;
    let mut candidates: Vec<(String, String)> = Vec::new();
    let mut seen = HashSet::new();
    let mut route = search_route(cfg, publik.is_some());
    let mut search_note: Option<String> = None;
    let mut searches = 0usize;
    for q in queries.iter().take(MAX_QUERIES) {
        searches += 1;
        let results = match (route, publik) {
            ("brave", _) => search_brave(&c, cfg.brave_key.trim(), q).await.unwrap_or_default(),
            ("publik", Some(cred)) => match crate::publik::search(cred, q, 8).await {
                Ok(r) => r,
                Err(e) => {
                    // One refusal (no credit, unreachable) ends the publik leg: the rest of the
                    // check searches without a key, and the card says why.
                    route = "web";
                    search_note = Some(format!("{} Searched the web without a key instead.", e.message));
                    search_keyless(&c, q).await
                }
            },
            _ => search_keyless(&c, q).await,
        };
        for (url, title) in results.into_iter().take(4) {
            if seen.insert(url.clone()) {
                candidates.push((url, title));
            }
        }
        if candidates.len() >= MAX_FETCH * 2 {
            break;
        }
    }
    let my_shingles = shingles(&words, NGRAM);
    let mut sources: Vec<Source> = Vec::new();
    for (url, title) in candidates.into_iter().take(MAX_FETCH) {
        let html = match c.get(&url).send().await {
            Ok(r) => r.text().await.unwrap_or_default(),
            Err(_) => continue,
        };
        let (ptitle, ptext) = page_text(&html);
        let pw = normalize_words(&ptext);
        if pw.len() < NGRAM {
            continue;
        }
        let ps = shingles(&pw, NGRAM);
        let overlap: Vec<&String> = my_shingles.iter().filter(|s| ps.contains(*s)).collect();
        if overlap.is_empty() {
            continue;
        }
        // approximate matched words: distinct shingles overlap × 1 word each, plus window tail
        let matched_words = (overlap.len() + NGRAM - 1).min(words.len());
        let matched_pct = matched_words as f64 * 100.0 / words.len() as f64;
        let snippet = overlap.iter().take(1).map(|s| s.to_string()).next().unwrap_or_default();
        let title = if ptitle.is_empty() { title } else { ptitle };
        let site = site_of(&url);
        sources.push(Source { citations: citations(&title, &url, &site), url, title, site, matched_words, matched_pct, snippet });
    }
    sources.sort_by(|a, b| b.matched_words.cmp(&a.matched_words));
    let overall = sources.iter().map(|s| s.matched_pct).fold(0.0, f64::max).min(100.0);
    Ok(json!({"sources": sources, "overall_pct": overall, "queries": queries, "searches": searches, "search_via": route, "search_note": search_note, "words": words.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plagiarism_search_goes_own_key_then_publik_then_keyless() {
        let own = SearchConfig { brave_key: "BSA-x".into() };
        let none = SearchConfig::default();
        assert_eq!(search_route(&own, true), "brave");
        assert_eq!(search_route(&own, false), "brave");
        assert_eq!(search_route(&none, true), "publik");
        assert_eq!(search_route(&none, false), "web");
    }

    #[test]
    fn queries_are_distinctive_mid_sentence_runs() {
        let t = "The Great Barrier Reef is the world's largest coral reef system composed of over 2,900 individual reefs. It stretches for over 2,300 kilometres off the coast of Queensland. Scientists have documented significant bleaching events in recent decades across the reef.";
        let q = pick_queries(t);
        assert!(!q.is_empty());
        for s in &q {
            assert!(s.split_whitespace().count() >= 5, "{s}");
        }
    }

    #[test]
    fn shingle_overlap_detects_copied_passages() {
        let a = normalize_words("the quick brown fox jumps over the lazy dog near the river bank today");
        let b = normalize_words("yesterday the quick brown fox jumps over the lazy dog near the river bank again");
        let sa = shingles(&a, NGRAM);
        let sb = shingles(&b, NGRAM);
        assert!(sa.iter().filter(|s| sb.contains(*s)).count() >= 5);
    }

    #[test]
    fn citations_have_three_styles() {
        let c = citations("A Title", "https://example.com/x", "example.com");
        assert!(c["APA"].contains("example.com"));
        assert!(c["MLA"].contains("A Title"));
        assert!(c["Chicago"].contains("Accessed"));
    }

    #[test]
    fn page_text_strips_markup() {
        let (t, body) = page_text("<html><head><title>Hi</title></head><body><p>Paragraph one is long enough to count.</p><script>x()</script></body></html>");
        assert_eq!(t, "Hi");
        assert!(body.contains("Paragraph one"));
        assert!(!body.contains("x()"));
    }

    #[test]
    fn bing_redirect_links_decode_to_the_target_url() {
        let href = "https://www.bing.com/ck/a?!&&p=abc&u=a1aHR0cHM6Ly9lbi53aWtpcGVkaWEub3JnL3dpa2kvR3JlYXRfQmFycmllcl9SZWVm&ntb=1";
        assert_eq!(decode_bing_link(href), "https://en.wikipedia.org/wiki/Great_Barrier_Reef");
        assert_eq!(decode_bing_link("https://example.com/x"), "https://example.com/x");
        assert_eq!(strip_fragment("https://a.b/c#frag"), "https://a.b/c");
    }
}
