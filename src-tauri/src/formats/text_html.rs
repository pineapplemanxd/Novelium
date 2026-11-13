use regex::Regex;
use scraper::{Html, Selector};

use crate::error::{AppError, AppResult};
use crate::formats::{ParsedBook, ParsedChapter};

pub fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn count_words(text: &str) -> i64 {
    text.split_whitespace().count() as i64
}

pub fn clean_html_fragment(raw_html: &str) -> (String, Vec<String>, i64) {
    clean_html_fragment_excluding(raw_html, &[])
}

pub fn clean_html_fragment_excluding(
    raw_html: &str,
    remove_selectors: &[&str],
) -> (String, Vec<String>, i64) {
    use std::collections::HashSet;

    let doc = Html::parse_fragment(raw_html);
    let mut removed_ids: HashSet<ego_tree::NodeId> = HashSet::new();
    for sel_str in remove_selectors {
        if let Ok(rsel) = Selector::parse(sel_str) {
            for node in doc.select(&rsel) {
                removed_ids.insert(node.id());
            }
        }
    }

    let is_removed = |el: &scraper::ElementRef<'_>| -> bool {
        removed_ids.contains(&el.id()) || el.ancestors().any(|a| removed_ids.contains(&a.id()))
    };

    let block_sel = Selector::parse("p, h1, h2, h3, h4, blockquote, li, pre").unwrap();
    let all_sel = Selector::parse("p, h1, h2, h3, h4, blockquote, li, pre, hr").unwrap();

    let mut html_out = String::new();
    let mut paragraphs = Vec::new();
    let mut total_words = 0i64;
    let mut matched_any = false;

    for el in doc.select(&all_sel) {
        if is_removed(&el) {
            continue;
        }
        let tag = el.value().name();
        // Skip outer container if it contains nested non-removed block elements
        if matches!(tag, "blockquote" | "li")
            && el
                .select(&block_sel)
                .any(|child| child.id() != el.id() && !is_removed(&child))
        {
            continue;
        }
        matched_any = true;
        if tag == "hr" {
            html_out.push_str("<hr />\n");
            continue;
        }

        let (safe_inner, visible_text) = sanitize_node_children(*el, &removed_ids);
        let normalized = visible_text.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            continue;
        }

        total_words += count_words(&normalized);
        paragraphs.push(normalized.clone());

        match tag {
            "h1" | "h2" => html_out.push_str(&format!("<h2>{safe_inner}</h2>\n")),
            "h3" | "h4" => html_out.push_str(&format!("<h3>{safe_inner}</h3>\n")),
            "blockquote" => html_out.push_str(&format!("<blockquote><p>{safe_inner}</p></blockquote>\n")),
            "pre" => html_out.push_str(&format!("<pre>{}</pre>\n", html_escape(&normalized))),
            _ => html_out.push_str(&format!("<p>{safe_inner}</p>\n")),
        }
    }

    if !matched_any || paragraphs.is_empty() {
        let (_, plain) = sanitize_node_children(*doc.root_element(), &removed_ids);
        for line in plain.lines() {
            let trimmed = line.split_whitespace().collect::<Vec<_>>().join(" ");
            if !trimmed.is_empty() {
                total_words += count_words(&trimmed);
                html_out.push_str(&format!("<p>{}</p>\n", html_escape(&trimmed)));
                paragraphs.push(trimmed);
            }
        }
    }

    (html_out, paragraphs, total_words)
}

fn sanitize_inline_html_and_text(input: &str) -> (String, String) {
    let frag = Html::parse_fragment(input);
    sanitize_node_children(*frag.root_element(), &std::collections::HashSet::new())
}

fn sanitize_node_children(
    parent: ego_tree::NodeRef<'_, scraper::Node>,
    removed_ids: &std::collections::HashSet<ego_tree::NodeId>,
) -> (String, String) {
    let mut out_html = String::new();
    let mut out_text = String::new();
    for child in parent.children() {
        write_sanitized_node(child, removed_ids, &mut out_html, &mut out_text);
    }
    if out_html.trim().is_empty() && !out_text.trim().is_empty() {
        out_html = html_escape(&out_text);
    }
    (out_html, out_text)
}

fn write_sanitized_node(
    node: ego_tree::NodeRef<'_, scraper::Node>,
    removed_ids: &std::collections::HashSet<ego_tree::NodeId>,
    out_html: &mut String,
    out_text: &mut String,
) {
    if removed_ids.contains(&node.id()) {
        return;
    }
    match node.value() {
        scraper::Node::Text(t) => {
            out_html.push_str(&html_escape(&t.text));
            out_text.push_str(&t.text);
        }
        scraper::Node::Element(el) => {
            let tag = el.name().to_ascii_lowercase();
            match tag.as_str() {
                "script" | "style" | "iframe" | "object" | "embed" | "svg" => {}
                "br" => {
                    out_html.push_str("<br />");
                    out_text.push(' ');
                }
                "em" | "i" | "strong" | "b" | "u" | "s" | "sup" | "sub" | "code" | "small" => {
                    out_html.push('<');
                    out_html.push_str(&tag);
                    out_html.push('>');
                    for child in node.children() {
                        write_sanitized_node(child, removed_ids, out_html, out_text);
                    }
                    out_html.push_str("</");
                    out_html.push_str(&tag);
                    out_html.push('>');
                }
                _ => {
                    for child in node.children() {
                        write_sanitized_node(child, removed_ids, out_html, out_text);
                    }
                }
            }
        }
        _ => {
            for child in node.children() {
                write_sanitized_node(child, removed_ids, out_html, out_text);
            }
        }
    }
}

pub fn paragraphs_to_html(paragraphs: &[String]) -> String {
    paragraphs
        .iter()
        .map(|p| {
            let t = p.trim();
            if t == "***" || t == "* * *" || t == "---" {
                "<hr />".to_string()
            } else {
                format!("<p>{}</p>", html_escape(t))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_txt_bytes(bytes: &[u8], fallback_title: &str) -> AppResult<ParsedBook> {
    let raw = String::from_utf8_lossy(bytes).replace("\r\n", "\n").replace('\r', "\n");
    if raw.trim().is_empty() {
        return Err(AppError::InvalidInput("Text file is empty".into()));
    }

    // Try extracting Gutenberg / plain-text metadata if present in the first 60 lines
    let mut title = fallback_title.to_string();
    let mut author = "Unknown Author".to_string();
    for line in raw.lines().take(60) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Title:") {
            let t = rest.trim();
            if !t.is_empty() {
                title = t.to_string();
            }
        } else if let Some(rest) = trimmed.strip_prefix("Author:") {
            let a = rest.trim();
            if !a.is_empty() {
                author = a.to_string();
            }
        }
    }

    // Strip Project Gutenberg start/end boilerplate markers if present
    let content = strip_gutenberg_markers(&raw);
    let chapters = split_plain_text_into_chapters(content, &title);

    Ok(ParsedBook {
        title,
        author,
        description: String::new(),
        genres: Vec::new(),
        tags: vec!["TXT".into()],
        rating: None,
        language: None,
        series: None,
        series_index: None,
        cover_bytes: None,
        cover_ext: None,
        chapters,
    })
}

pub fn strip_gutenberg_markers(text: &str) -> &str {
    let start_idx = text
        .find("*** START OF THE PROJECT GUTENBERG")
        .or_else(|| text.find("*** START OF THIS PROJECT GUTENBERG"))
        .and_then(|pos| text[pos..].find('\n').map(|nl| pos + nl + 1))
        .unwrap_or(0);

    let end_idx = text
        .find("*** END OF THE PROJECT GUTENBERG")
        .or_else(|| text.find("*** END OF THIS PROJECT GUTENBERG"))
        .unwrap_or(text.len());

    if end_idx > start_idx {
        &text[start_idx..end_idx]
    } else {
        text
    }
}

pub fn split_plain_text_into_chapters(text: &str, book_title: &str) -> Vec<ParsedChapter> {
    let heading_re = Regex::new(
        r"(?i)^(chapter\s+([0-9ivxlcdm]+|one|two|three|four|five|six|seven|eight|nine|ten)\b.*|prologue\b.*|epilogue\b.*|part\s+[0-9ivxlcdm]+\b.*|book\s+[0-9ivxlcdm]+\b.*|letter\s+[0-9ivxlcdm]+\b.*)$",
    )
    .unwrap();

    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_buf: Vec<String> = Vec::new();

    // Split into blank-line-separated paragraphs
    let mut para_acc = Vec::new();
    let flush_para = |acc: &mut Vec<String>, target: &mut Vec<String>| {
        if !acc.is_empty() {
            let joined = acc.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
            if !joined.is_empty() {
                target.push(joined);
            }
            acc.clear();
        }
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush_para(&mut para_acc, &mut current_buf);
            continue;
        }
        if para_acc.is_empty() && trimmed.len() <= 100 && heading_re.is_match(trimmed) {
            flush_para(&mut para_acc, &mut current_buf);
            if !current_buf.is_empty() {
                let sec_title = current_title
                    .take()
                    .unwrap_or_else(|| format!("Introduction — {book_title}"));
                // Ignore tiny pre-chapter TOC stubs if under 25 words and not first real content
                let wc: i64 = current_buf.iter().map(|p| count_words(p)).sum();
                if wc >= 25 || sections.is_empty() {
                    sections.push((sec_title, std::mem::take(&mut current_buf)));
                } else {
                    current_buf.clear();
                }
            }
            current_title = Some(trimmed.to_string());
        } else {
            para_acc.push(trimmed.to_string());
        }
    }
    flush_para(&mut para_acc, &mut current_buf);
    if !current_buf.is_empty() {
        let sec_title = current_title.unwrap_or_else(|| {
            if sections.is_empty() {
                book_title.to_string()
            } else {
                format!("Chapter {}", sections.len() + 1)
            }
        });
        sections.push((sec_title, current_buf));
    }

    // If no chapter headings matched and file is huge (> 4500 words), split cleanly at ~3000-word boundaries
    if sections.len() <= 1 {
        let all_paras: Vec<String> = sections
            .into_iter()
            .flat_map(|(_, ps)| ps)
            .collect();
        let total_words: i64 = all_paras.iter().map(|p| count_words(p)).sum();
        if total_words > 4500 {
            let mut chunked = Vec::new();
            let mut cur = Vec::new();
            let mut cur_words = 0i64;
            for p in all_paras {
                let w = count_words(&p);
                cur.push(p);
                cur_words += w;
                if cur_words >= 3000 {
                    let idx = chunked.len() + 1;
                    chunked.push((format!("Part {idx}"), std::mem::take(&mut cur)));
                    cur_words = 0;
                }
            }
            if !cur.is_empty() {
                let idx = chunked.len() + 1;
                chunked.push((format!("Part {idx}"), cur));
            }
            return chunked
                .into_iter()
                .enumerate()
                .map(|(i, (t, ps))| {
                    let wc: i64 = ps.iter().map(|p| count_words(p)).sum();
                    let html = paragraphs_to_html(&ps);
                    ParsedChapter {
                        chapter_number: (i + 1) as f64,
                        title: t,
                        html,
                        paragraphs: ps,
                        word_count: wc,
                    }
                })
                .collect();
        } else {
            return vec![ParsedChapter {
                chapter_number: 1.0,
                title: book_title.to_string(),
                html: paragraphs_to_html(&all_paras),
                word_count: total_words,
                paragraphs: all_paras,
            }];
        }
    }

    sections
        .into_iter()
        .enumerate()
        .map(|(i, (title, paras))| {
            let wc = paras.iter().map(|p| count_words(p)).sum();
            let html = paragraphs_to_html(&paras);
            ParsedChapter {
                chapter_number: (i + 1) as f64,
                title,
                html,
                paragraphs: paras,
                word_count: wc,
            }
        })
        .collect()
}

pub fn parse_html_bytes(bytes: &[u8], fallback_title: &str) -> AppResult<ParsedBook> {
    let raw = String::from_utf8_lossy(bytes);
    if raw.trim().is_empty() {
        return Err(AppError::InvalidInput("HTML file is empty".into()));
    }

    let doc = Html::parse_document(&raw);
    let title_sel = Selector::parse("title").unwrap();
    let author_sel = Selector::parse("meta[name='author'], meta[name='dc.creator']").unwrap();
    let desc_sel = Selector::parse("meta[name='description']").unwrap();

    let title = doc
        .select(&title_sel)
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback_title.to_string());

    let author = doc
        .select(&author_sel)
        .next()
        .and_then(|e| e.value().attr("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown Author".to_string());

    let description = doc
        .select(&desc_sel)
        .next()
        .and_then(|e| e.value().attr("content"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Try splitting by <h1> / <h2> sections inside body
    let body_sel = Selector::parse("body").unwrap();
    let body_html = doc
        .select(&body_sel)
        .next()
        .map(|b| b.inner_html())
        .unwrap_or_else(|| raw.to_string());

    let chapters = split_html_by_headings(&body_html, &title);

    Ok(ParsedBook {
        title,
        author,
        description,
        genres: Vec::new(),
        tags: vec!["HTML".into()],
        rating: None,
        language: None,
        series: None,
        series_index: None,
        cover_bytes: None,
        cover_ext: None,
        chapters,
    })
}

pub fn split_html_by_headings(body_html: &str, fallback_title: &str) -> Vec<ParsedChapter> {
    let frag = Html::parse_fragment(body_html);
    let block_sel = Selector::parse("h1, h2, p, blockquote, pre, hr, li").unwrap();

    let mut sections: Vec<(String, Vec<String>, String)> = Vec::new();
    let mut cur_title = fallback_title.to_string();
    let mut cur_paras: Vec<String> = Vec::new();
    let mut cur_html = String::new();
    let mut seen_heading = false;

    for el in frag.select(&block_sel) {
        let tag = el.value().name();
        if tag == "h1" || tag == "h2" {
            let heading = el.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
            if heading.is_empty() {
                continue;
            }
            if !cur_paras.is_empty() {
                let wc: i64 = cur_paras.iter().map(|p| count_words(p)).sum();
                if seen_heading || wc >= 15 {
                    sections.push((
                        cur_title.clone(),
                        std::mem::take(&mut cur_paras),
                        std::mem::take(&mut cur_html),
                    ));
                } else {
                    cur_paras.clear();
                    cur_html.clear();
                }
            }
            cur_title = heading;
            seen_heading = true;
            continue;
        }

        if tag == "hr" {
            cur_html.push_str("<hr />\n");
            continue;
        }

        let (safe, raw_text) = sanitize_inline_html_and_text(&el.inner_html());
        let text = raw_text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        // Skip Project Gutenberg boilerplate paragraphs
        if text.contains("Project Gutenberg") && (text.contains("eBook") || text.contains("END OF THE") || text.contains("START OF THE")) {
            continue;
        }

        cur_paras.push(text);
        match tag {
            "blockquote" => cur_html.push_str(&format!("<blockquote><p>{safe}</p></blockquote>\n")),
            "pre" => cur_html.push_str(&format!("<pre>{safe}</pre>\n")),
            _ => cur_html.push_str(&format!("<p>{safe}</p>\n")),
        }
    }

    if !cur_paras.is_empty() {
        sections.push((cur_title, cur_paras, cur_html));
    }

    if sections.is_empty() {
        let (html, paras, wc) = clean_html_fragment(body_html);
        if paras.is_empty() {
            return Vec::new();
        }
        return vec![ParsedChapter {
            chapter_number: 1.0,
            title: fallback_title.to_string(),
            html,
            paragraphs: paras,
            word_count: wc,
        }];
    }

    sections
        .into_iter()
        .enumerate()
        .map(|(i, (title, paras, html))| {
            let wc = paras.iter().map(|p| count_words(p)).sum();
            ParsedChapter {
                chapter_number: (i + 1) as f64,
                title,
                html,
                paragraphs: paras,
                word_count: wc,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_txt_chapter_splitting_and_gutenberg_markers() {
        let sample = "\
Title: Test Odyssey
Author: Ada Lovelace

*** START OF THE PROJECT GUTENBERG EBOOK TEST ODYSSEY ***

Chapter 1: The Dawn
Morning broke over the sea.

The ships set sail at noon.

Chapter 2: The Storm
Dark clouds gathered above the mast.

*** END OF THE PROJECT GUTENBERG EBOOK TEST ODYSSEY ***
End license text should be ignored.
";
        let book = parse_txt_bytes(sample.as_bytes(), "Fallback").unwrap();
        assert_eq!(book.title, "Test Odyssey");
        assert_eq!(book.author, "Ada Lovelace");
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "Chapter 1: The Dawn");
        assert_eq!(book.chapters[0].paragraphs.len(), 2);
        assert_eq!(book.chapters[1].title, "Chapter 2: The Storm");
        assert!(!book.chapters[1].html.contains("End license text"));
    }

    #[test]
    fn test_html_sanitization_strips_script() {
        let raw = r#"<div><p>Hello <em>world</em><script>alert(1)</script></p></div>"#;
        let (html, paras, words) = clean_html_fragment(raw);
        assert!(!html.contains("script"));
        assert!(!html.contains("alert"));
        assert!(html.contains("<em>world</em>"));
        assert_eq!(paras, vec!["Hello world"]);
        assert_eq!(words, 2);
    }

    #[test]
    fn test_html_multi_heading_chapter_splitting() {
        let html = br#"
        <html>
          <head>
            <title>Glass Mountain</title>
            <meta name="author" content="J. Kepler" />
          </head>
          <body>
            <h2>Chapter 1: Ascent</h2>
            <p>Snow covered the steep ridge.</p>
            <p>Lanterns flickered below.</p>
            <h2>Chapter 2: Summit</h2>
            <p>At midnight the summit shone clear.<script>bad()</script></p>
          </body>
        </html>"#;

        let book = parse_html_bytes(html, "Fallback").unwrap();
        assert_eq!(book.title, "Glass Mountain");
        assert_eq!(book.author, "J. Kepler");
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "Chapter 1: Ascent");
        assert_eq!(book.chapters[0].paragraphs.len(), 2);
        assert_eq!(book.chapters[1].title, "Chapter 2: Summit");
        assert_eq!(book.chapters[1].paragraphs, vec!["At midnight the summit shone clear."]);
        assert!(!book.chapters[1].html.contains("bad()"));
    }

    #[test]
    fn test_clean_html_fragment_excluding_and_nested_li() {
        let raw = r#"
        <div class="ws-noexport" data-foo="a > b"><p>Navigation Banner</p></div>
        <ul><li><p>First bullet<span class="mw-editsection">[edit]</span></p></li></ul>
        <p>Main prose with <em>emphasis</em>.</p>
        "#;
        let (html, paras, wc) =
            clean_html_fragment_excluding(raw, &[".ws-noexport", ".mw-editsection"]);
        assert_eq!(paras, vec!["First bullet", "Main prose with emphasis."]);
        assert!(!html.contains("Navigation Banner"));
        assert!(!html.contains("[edit]"));
        assert!(html.contains("<em>emphasis</em>"));
        assert_eq!(wc, 6);
    }
}
