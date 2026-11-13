use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use chrono::Utc;
use scraper::{Html, Selector};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::formats::text_html::clean_html_fragment_excluding;
use crate::models::{
    Chapter, DiscoveryQuery, NovelRecord, NovelStatus, NovelSummary, SourceCapabilities,
};
use crate::net::SourceHttpClient;
use crate::sources::{make_chapter_id, make_novel_id, NovelSource};

const BASE_URL: &str = "https://standardebooks.org";

pub struct StandardEbooksSource {
    http: SourceHttpClient,
}

impl StandardEbooksSource {
    pub fn new(http: SourceHttpClient) -> Self {
        Self { http }
    }

    fn normalize_slug(raw: &str) -> String {
        let trimmed = raw
            .trim()
            .trim_start_matches(BASE_URL)
            .trim_start_matches('/');
        trimmed
            .strip_prefix("ebooks/")
            .unwrap_or(trimmed)
            .trim_matches('/')
            .to_string()
    }

    fn book_url(slug: &str) -> String {
        format!("{BASE_URL}/ebooks/{}", Self::normalize_slug(slug))
    }

    fn resolve_asset_url(src: &str) -> Option<String> {
        let s = src.trim();
        if s.is_empty() {
            return None;
        }
        if s.starts_with("http://") || s.starts_with("https://") {
            Some(s.to_string())
        } else if s.starts_with("//") {
            Some(format!("https:{s}"))
        } else if s.starts_with('/') {
            Some(format!("{BASE_URL}{s}"))
        } else {
            Some(format!("{BASE_URL}/{s}"))
        }
    }

    fn parse_catalog_html(html: &str) -> (Vec<NovelSummary>, bool) {
        let doc = Html::parse_document(html);
        let Ok(li_sel) = Selector::parse("ol.ebooks-list li") else {
            return (Vec::new(), false);
        };
        let title_sel = Selector::parse("p:not(.author) a span[property='schema:name'], p:not(.author) a").ok();
        let author_sel =
            Selector::parse("p.author [property='schema:name'], p.author a, p.author").ok();
        let link_sel = Selector::parse("p a[href*='/ebooks/'], a[href*='/ebooks/']").ok();
        let img_sel = Selector::parse("img").ok();

        let mut seen = HashSet::new();
        let mut out = Vec::new();

        for card in doc.select(&li_sel) {
            let href = link_sel
                .as_ref()
                .and_then(|s| card.select(s).next())
                .and_then(|e| e.value().attr("href"))
                .or_else(|| card.value().attr("about"));
            let Some(href) = href else {
                continue;
            };
            let slug = Self::normalize_slug(href);
            if slug.is_empty() || !slug.contains('/') || !seen.insert(slug.clone()) {
                continue;
            }

            let title = title_sel
                .as_ref()
                .and_then(|s| card.select(s).next())
                .map(|e| {
                    e.text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }

            let author = author_sel
                .as_ref()
                .and_then(|s| card.select(s).next())
                .map(|e| {
                    e.text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Unknown Author".into());

            let cover_url = img_sel
                .as_ref()
                .and_then(|s| card.select(s).next())
                .and_then(|e| e.value().attr("src"))
                .and_then(Self::resolve_asset_url);

            out.push(NovelSummary {
                id: make_novel_id("standardebooks", &slug),
                source_id: "standardebooks".into(),
                external_id: slug,
                title,
                author,
                description: "Free public-domain edition from Standard Ebooks.".into(),
                cover_url,
                local_cover_path: None,
                status: NovelStatus::Completed,
                genres: vec!["Classic Literature".into()],
                rating: None,
                popularity: 0,
                chapter_count: None,
                last_updated: None,
                language: Some("en".into()),
            });
        }

        let has_more = Selector::parse("a[rel='next']")
            .ok()
            .and_then(|s| doc.select(&s).next())
            .is_some();

        (out, has_more)
    }

    fn parse_novel_detail_html(slug: &str, html: &str) -> NovelRecord {
        let doc = Html::parse_document(html);
        let text_of = |css: &str| -> String {
            Selector::parse(css)
                .ok()
                .and_then(|sel| doc.select(&sel).next())
                .map(|e| {
                    e.text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default()
        };

        let title = {
            let t = text_of("hgroup h1, article header h1, h1");
            if t.is_empty() {
                slug.rsplit('/').next().unwrap_or(slug).replace('-', " ")
            } else {
                t
            }
        };

        let author = {
            let a = text_of(
                "article header p a, [property='schema:author'] [property='schema:name'], p.author a, p.author",
            );
            if a.is_empty() {
                "Unknown Author".into()
            } else {
                a
            }
        };

        let description =
            Selector::parse("#description [property='schema:description'] p, #description p")
                .ok()
                .map(|sel| {
                    doc.select(&sel)
                        .map(|p| {
                            p.text()
                                .collect::<String>()
                                .split_whitespace()
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n\n")
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Public domain edition hosted on Standard Ebooks.".into());

        let mut genres = Vec::new();
        if let Ok(sel) = Selector::parse("ul.tags li a") {
            for el in doc.select(&sel) {
                let g = el
                    .text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if !g.is_empty() && !genres.contains(&g) {
                    genres.push(g);
                }
            }
        }
        if genres.is_empty() {
            genres.push("Fiction".into());
        }

        let cover_url = Selector::parse("picture img, article header img, img[property='schema:image']")
            .ok()
            .and_then(|sel| doc.select(&sel).next())
            .and_then(|img| img.value().attr("src"))
            .and_then(Self::resolve_asset_url)
            .or_else(|| {
                Selector::parse("meta[property='og:image']")
                    .ok()
                    .and_then(|sel| doc.select(&sel).next())
                    .and_then(|m| m.value().attr("content"))
                    .and_then(Self::resolve_asset_url)
            });

        let language = Selector::parse("meta[property='schema:inLanguage']")
            .ok()
            .and_then(|sel| doc.select(&sel).next())
            .and_then(|m| m.value().attr("content"))
            .and_then(|c| c.split(['-', '_']).next())
            .map(|c| c.trim().to_ascii_lowercase())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "en".into());

        let now = Utc::now().to_rfc3339();
        let norm_slug = Self::normalize_slug(slug);

        NovelRecord {
            id: make_novel_id("standardebooks", &norm_slug),
            source_id: "standardebooks".into(),
            external_id: norm_slug,
            title,
            author_id: None,
            author,
            description,
            cover_url,
            local_cover_path: None,
            status: NovelStatus::Completed,
            genres,
            tags: Vec::new(),
            rating: None,
            popularity: 0,
            chapter_count: 0,
            last_updated_remote: None,
            metadata: serde_json::json!({ "language": language }),
            is_local: false,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    fn parse_toc_html(slug: &str, html: &str) -> Vec<Chapter> {
        let doc = Html::parse_document(html);
        let Ok(sel) = Selector::parse("#toc ol li a, nav[id='toc'] a") else {
            return Vec::new();
        };
        let norm_slug = Self::normalize_slug(slug);
        let novel_id = make_novel_id("standardebooks", &norm_slug);
        let book_base = Self::book_url(&norm_slug);
        let mut seen = HashSet::new();
        let mut out = Vec::new();

        for a in doc.select(&sel) {
            let Some(href) = a.value().attr("href") else {
                continue;
            };
            let clean_href = href.split('#').next().unwrap_or(href).trim();
            if clean_href.is_empty() {
                continue;
            }
            let lower = clean_href.to_ascii_lowercase();
            if matches!(
                lower.rsplit('/').next().unwrap_or(""),
                "titlepage"
                    | "imprint"
                    | "halftitlepage"
                    | "colophon"
                    | "uncopyright"
                    | "loi"
                    | "endnotes"
            ) {
                continue;
            }
            let rel_path = clean_href
                .trim_start_matches('/')
                .strip_prefix(&format!("ebooks/{norm_slug}/"))
                .unwrap_or_else(|| clean_href.trim_start_matches('/'))
                .to_string();
            let ext_id = format!("{norm_slug}|{rel_path}");
            if !seen.insert(ext_id.clone()) {
                continue;
            }
            let title = a
                .text()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let idx = (out.len() + 1) as f64;
            out.push(Chapter {
                id: make_chapter_id(&novel_id, &ext_id),
                novel_id: novel_id.clone(),
                external_id: ext_id,
                chapter_number: idx,
                volume: None,
                title: if title.is_empty() {
                    format!("Section {}", idx as usize)
                } else {
                    title
                },
                url: Some(format!("{book_base}/{rel_path}")),
                word_count: 0,
                published_at: None,
                is_downloaded: false,
                local_path: None,
                size_bytes: 0,
            });
        }
        out
    }

    fn parse_chapter_html(html: &str) -> (String, Vec<String>, i64) {
        let doc = Html::parse_document(html);
        let body_html = Selector::parse("body main section, section[id], body main")
            .ok()
            .and_then(|s| doc.select(&s).next())
            .map(|e| e.inner_html())
            .unwrap_or_else(|| html.to_string());

        clean_html_fragment_excluding(&body_html, &["header", "nav", "script", "style"])
    }
}

#[async_trait]
impl NovelSource for StandardEbooksSource {
    fn id(&self) -> &str {
        "standardebooks"
    }

    fn name(&self) -> &str {
        "Standard Ebooks"
    }

    fn kind(&self) -> &str {
        "builtin"
    }

    fn base_url(&self) -> &str {
        BASE_URL
    }

    fn description(&self) -> &str {
        "Public domain literary editions formatted by Standard Ebooks with full chapter text."
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            search: true,
            browse_popular: true,
            browse_trending: false,
            browse_latest: false,
            browse_genre: false,
            filter_status: false,
            genres: vec!["Fiction".into(), "Adventure".into(), "Mystery".into()],
        }
    }

    async fn discover(
        &self,
        query: &DiscoveryQuery,
        _config: &HashMap<String, Value>,
    ) -> AppResult<(Vec<NovelSummary>, bool, Option<u64>)> {
        if let Some(lang) = query
            .language
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("all"))
        {
            if !lang.eq_ignore_ascii_case("en") && !lang.to_ascii_lowercase().starts_with("en-") {
                return Ok((Vec::new(), false, Some(0)));
            }
        }
        if matches!(query.status.as_deref(), Some("ongoing")) {
            return Ok((Vec::new(), false, Some(0)));
        }

        let page = query.page.max(1);
        let mut url = url::Url::parse(&format!("{BASE_URL}/ebooks"))?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("page", &page.to_string());
            if let Some(q) = query.query.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                qp.append_pair("query", q);
            }
        }

        let html = self.http.get_text(url.as_str()).await?;
        let (mut items, has_more) = Self::parse_catalog_html(&html);
        if query.sort.as_deref() == Some("title") {
            items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        }
        let limit = query.limit.clamp(1, 30) as usize;
        items.truncate(limit);
        Ok((items, has_more, None))
    }

    async fn get_novel(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<NovelRecord> {
        let url = Self::book_url(external_id);
        let html = self.http.get_text(&url).await?;
        Ok(Self::parse_novel_detail_html(external_id, &html))
    }

    async fn get_chapters(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<Vec<Chapter>> {
        let toc_url = format!("{}/text", Self::book_url(external_id));
        let html = self.http.get_text(&toc_url).await?;
        Ok(Self::parse_toc_html(external_id, &html))
    }

    async fn get_chapter_content(
        &self,
        external_novel_id: &str,
        external_chapter_id: &str,
        chapter_url: Option<&str>,
        _config: &HashMap<String, Value>,
    ) -> AppResult<(String, Vec<String>, i64)> {
        let target_url = if let Some(u) = chapter_url.filter(|u| u.starts_with("http")) {
            u.to_string()
        } else if let Some((slug, rel_path)) = external_chapter_id.split_once('|') {
            format!("{}/{rel_path}", Self::book_url(slug))
        } else if !external_novel_id.is_empty() {
            let rel = external_chapter_id.trim_start_matches('/');
            format!("{}/{rel}", Self::book_url(external_novel_id))
        } else {
            return Err(AppError::InvalidInput(
                "Missing Standard Ebooks chapter URL".into(),
            ));
        };

        let html = self.http.get_text(&target_url).await?;
        Ok(Self::parse_chapter_html(&html))
    }

    async fn health_check(&self, _config: &HashMap<String, Value>) -> AppResult<u64> {
        let start = std::time::Instant::now();
        let _ = self.http.get_text(&format!("{BASE_URL}/ebooks")).await?;
        Ok(start.elapsed().as_millis() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_standard_ebooks_catalog_and_toc() {
        let html = r#"
        <html><body>
          <ol class="ebooks-list grid">
            <li typeof="schema:Book" about="/ebooks/arthur-conan-doyle/sir-nigel">
              <div class="thumbnail-container">
                <a href="/ebooks/arthur-conan-doyle/sir-nigel" property="schema:url">
                  <img src="/images/covers/arthur-conan-doyle_sir-nigel/cover@2x.jpg"/>
                </a>
              </div>
              <p><a href="/ebooks/arthur-conan-doyle/sir-nigel" property="schema:url"><span property="schema:name">Sir Nigel</span></a></p>
              <p class="author"><a href="/ebooks/arthur-conan-doyle"><span property="schema:name">Arthur Conan Doyle</span></a></p>
            </li>
          </ol>
          <a rel="next" href="/ebooks?page=2">Next</a>
        </body></html>"#;

        let (items, has_more) = StandardEbooksSource::parse_catalog_html(html);
        assert!(has_more);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Sir Nigel");
        assert_eq!(items[0].author, "Arthur Conan Doyle");
        assert_eq!(items[0].external_id, "arthur-conan-doyle/sir-nigel");
        assert_eq!(items[0].language.as_deref(), Some("en"));
        assert_eq!(
            items[0].cover_url.as_deref(),
            Some("https://standardebooks.org/images/covers/arthur-conan-doyle_sir-nigel/cover@2x.jpg")
        );

        let toc_html = r#"
        <html><body><main>
          <nav id="toc" epub:type="toc">
            <ol>
              <li><a href="text/titlepage">Titlepage</a></li>
              <li><a href="text/imprint">Imprint</a></li>
              <li><a href="text/chapter-1">I: The House of Loring</a></li>
              <li><a href="text/chapter-2">II: How the Devil Came to Waverley</a></li>
              <li><a href="text/colophon">Colophon</a></li>
            </ol>
          </nav>
        </main></body></html>"#;
        let chapters =
            StandardEbooksSource::parse_toc_html("arthur-conan-doyle/sir-nigel", toc_html);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "I: The House of Loring");
        assert_eq!(
            chapters[0].external_id,
            "arthur-conan-doyle/sir-nigel|text/chapter-1"
        );
        assert_eq!(
            chapters[0].url.as_deref(),
            Some("https://standardebooks.org/ebooks/arthur-conan-doyle/sir-nigel/text/chapter-1")
        );
    }
}
