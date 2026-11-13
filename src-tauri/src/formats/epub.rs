use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

use scraper::{Html, Selector};
use zip::ZipArchive;

use crate::error::{AppError, AppResult};
use crate::formats::text_html::clean_html_fragment;
use crate::formats::{ParsedBook, ParsedChapter};

fn decode_percent(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn normalize_zip_path(base_dir: &str, href: &str) -> String {
    let without_frag = href.split('#').next().unwrap_or(href).trim();
    let decoded = decode_percent(without_frag);
    let combined = if base_dir.is_empty() || decoded.starts_with('/') {
        decoded.trim_start_matches('/').to_string()
    } else {
        format!("{}/{}", base_dir.trim_end_matches('/'), decoded)
    };

    let mut stack: Vec<&str> = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    stack.join("/")
}

fn read_zip_entry_bytes<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    target_path: &str,
) -> AppResult<Vec<u8>> {
    if let Ok(mut f) = archive.by_name(target_path) {
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        return Ok(buf);
    }

    let lower = target_path.to_ascii_lowercase();
    let mut found = None;
    for i in 0..archive.len() {
        if let Ok(f) = archive.by_index(i) {
            if let Ok(fname) = f.name() {
                if fname.to_ascii_lowercase() == lower {
                    found = Some(fname.to_string());
                    break;
                }
            }
        }
    }

    let name = found.ok_or_else(|| AppError::NotFound(format!("EPUB entry not found: {target_path}")))?;
    let mut file = archive.by_name(&name)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn parse_epub_bytes(bytes: &[u8], fallback_title: &str) -> AppResult<ParsedBook> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;

    // 1. Read META-INF/container.xml to locate the OPF file
    let container_bytes = read_zip_entry_bytes(&mut archive, "META-INF/container.xml")?;
    let container_xml = String::from_utf8_lossy(&container_bytes);
    let container_doc = roxmltree::Document::parse(&container_xml)?;
    let opf_path = container_doc
        .descendants()
        .find(|n| n.tag_name().name() == "rootfile")
        .and_then(|n| n.attribute("full-path"))
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::InvalidInput("Invalid EPUB: missing rootfile in container.xml".into()))?;

    let opf_dir = Path::new(&opf_path)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();

    // 2. Parse OPF XML (metadata, manifest, spine — including Calibre metadata!)
    let opf_bytes = read_zip_entry_bytes(&mut archive, &opf_path)?;
    let opf_xml = String::from_utf8_lossy(&opf_bytes);
    let opf_doc = roxmltree::Document::parse(&opf_xml)?;

    let mut titles = Vec::new();
    let mut authors = Vec::new();
    let mut descriptions = Vec::new();
    let mut genres = Vec::new();
    let mut tags = vec!["EPUB".to_string()];
    let mut language = None;
    let mut series = None;
    let mut series_index = None;
    let mut rating: Option<f64> = None;
    let mut cover_id: Option<String> = None;

    if let Some(metadata) = opf_doc.descendants().find(|n| n.tag_name().name() == "metadata") {
        for child in metadata.children().filter(|n| n.is_element()) {
            let tag = child.tag_name().name();
            let text = child.text().unwrap_or("").trim();
            match tag {
                "title" if !text.is_empty() => titles.push(text.to_string()),
                "creator" if !text.is_empty() => authors.push(text.to_string()),
                "description" if !text.is_empty() => descriptions.push(text.to_string()),
                "subject" if !text.is_empty() => genres.push(text.to_string()),
                "language" if !text.is_empty() && language.is_none() => {
                    language = Some(text.to_string())
                }
                "meta" => {
                    let name_attr = child.attribute("name").or_else(|| child.attribute("property")).unwrap_or("");
                    let content_attr = child.attribute("content").unwrap_or(text);
                    match name_attr {
                        "cover" if !content_attr.is_empty() => {
                            cover_id = Some(content_attr.to_string());
                        }
                        "calibre:series" if !content_attr.is_empty() => {
                            series = Some(content_attr.to_string());
                            tags.push(format!("Series: {content_attr}"));
                        }
                        "calibre:series_index" => {
                            if let Ok(idx) = content_attr.parse::<f64>() {
                                series_index = Some(idx);
                            }
                        }
                        "calibre:rating" => {
                            if let Ok(r) = content_attr.parse::<f64>() {
                                // Calibre stores 0-10 internally (2 = 1 star ... 10 = 5 stars)
                                rating = Some(if r > 5.0 { r / 2.0 } else { r });
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    let title = titles
        .into_iter()
        .next()
        .unwrap_or_else(|| fallback_title.to_string());
    let author = if authors.is_empty() {
        "Unknown Author".to_string()
    } else {
        authors.join(", ")
    };
    let description = descriptions
        .into_iter()
        .map(|d| {
            let (_, ps, _) = clean_html_fragment(&d);
            if ps.is_empty() {
                d
            } else {
                ps.join("\n\n")
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // Manifest map: id -> (resolved_href, media_type, properties)
    #[derive(Clone)]
    struct ManifestItem {
        id: String,
        href: String,
        media_type: String,
    }
    let mut manifest: HashMap<String, ManifestItem> = HashMap::new();
    let mut ncx_path: Option<String> = None;
    let mut nav_path: Option<String> = None;
    let mut cover_href: Option<String> = None;

    if let Some(manifest_node) = opf_doc.descendants().find(|n| n.tag_name().name() == "manifest") {
        for item in manifest_node.children().filter(|n| n.is_element() && n.tag_name().name() == "item") {
            let Some(id) = item.attribute("id") else { continue };
            let Some(href) = item.attribute("href") else { continue };
            let media_type = item.attribute("media-type").unwrap_or("").to_string();
            let properties = item.attribute("properties").unwrap_or("");
            let full_href = normalize_zip_path(&opf_dir, href);

            if properties.split_whitespace().any(|p| p == "cover-image") {
                cover_href = Some(full_href.clone());
            }
            if properties.split_whitespace().any(|p| p == "nav") {
                nav_path = Some(full_href.clone());
            }
            if media_type == "application/x-dtbncx+xml" {
                ncx_path = Some(full_href.clone());
            }
            manifest.insert(
                id.to_string(),
                ManifestItem {
                    id: id.to_string(),
                    href: full_href,
                    media_type,
                },
            );
        }
    }

    if cover_href.is_none() {
        if let Some(cid) = &cover_id {
            if let Some(item) = manifest.get(cid) {
                if item.media_type.starts_with("image/") {
                    cover_href = Some(item.href.clone());
                }
            }
        }
    }
    if cover_href.is_none() {
        cover_href = manifest
            .values()
            .find(|m| {
                m.media_type.starts_with("image/")
                    && (m.id.to_ascii_lowercase().contains("cover")
                        || m.href.to_ascii_lowercase().contains("cover"))
            })
            .map(|m| m.href.clone());
    }

    let (cover_bytes, cover_ext) = if let Some(chref) = cover_href {
        match read_zip_entry_bytes(&mut archive, &chref) {
            Ok(data) => {
                let ext = Path::new(&chref)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_else(|| "jpg".to_string());
                (Some(data), Some(ext))
            }
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };

    // 3. Parse TOC labels from NCX or EPUB3 nav
    let mut toc_labels: HashMap<String, String> = HashMap::new();
    if let Some(np) = nav_path {
        if let Ok(nav_bytes) = read_zip_entry_bytes(&mut archive, &np) {
            let nav_dir = Path::new(&np)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let nav_html = String::from_utf8_lossy(&nav_bytes);
            let doc = Html::parse_document(&nav_html);
            let a_sel = Selector::parse("nav a").unwrap();
            for a in doc.select(&a_sel) {
                if let Some(href) = a.value().attr("href") {
                    let label = a.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
                    if !label.is_empty() {
                        let norm = normalize_zip_path(&nav_dir, href);
                        toc_labels.entry(norm).or_insert(label);
                    }
                }
            }
        }
    }
    if let Some(ncx) = ncx_path {
        if let Ok(ncx_bytes) = read_zip_entry_bytes(&mut archive, &ncx) {
            let ncx_dir = Path::new(&ncx)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let ncx_xml = String::from_utf8_lossy(&ncx_bytes);
            if let Ok(ncx_doc) = roxmltree::Document::parse(&ncx_xml) {
                for nav_point in ncx_doc.descendants().filter(|n| n.tag_name().name() == "navPoint") {
                    let label = nav_point
                        .children()
                        .find(|n| n.tag_name().name() == "navLabel")
                        .and_then(|nl| nl.descendants().find(|t| t.tag_name().name() == "text"))
                        .and_then(|t| t.text())
                        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
                        .unwrap_or_default();
                    let src = nav_point
                        .children()
                        .find(|n| n.tag_name().name() == "content")
                        .and_then(|c| c.attribute("src"));
                    if !label.is_empty() {
                        if let Some(s) = src {
                            let norm = normalize_zip_path(&ncx_dir, s);
                            toc_labels.entry(norm).or_insert(label);
                        }
                    }
                }
            }
        }
    }

    // 4. Walk spine items
    let spine_node = opf_doc
        .descendants()
        .find(|n| n.tag_name().name() == "spine")
        .ok_or_else(|| AppError::InvalidInput("Invalid EPUB: missing spine".into()))?;

    let mut chapters = Vec::new();
    let heading_sel = Selector::parse("h1, h2, h3, title").unwrap();
    let body_sel = Selector::parse("body").unwrap();

    for itemref in spine_node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "itemref")
    {
        let Some(idref) = itemref.attribute("idref") else {
            continue;
        };
        let Some(item) = manifest.get(idref) else {
            continue;
        };
        if !item.media_type.contains("html") && !item.media_type.contains("xml") {
            continue;
        }
        let Ok(raw_bytes) = read_zip_entry_bytes(&mut archive, &item.href) else {
            continue;
        };
        let raw_html = String::from_utf8_lossy(&raw_bytes);
        let doc = Html::parse_document(&raw_html);

        let body_inner = doc
            .select(&body_sel)
            .next()
            .map(|b| b.inner_html())
            .unwrap_or_else(|| raw_html.to_string());

        let (clean_html, paragraphs, word_count) = clean_html_fragment(&body_inner);
        // Skip empty cover-image wrapper pages or blank spacers
        if word_count < 8 && paragraphs.is_empty() {
            continue;
        }

        let heading_fallback = doc
            .select(&heading_sel)
            .map(|e| e.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" "))
            .find(|s| !s.is_empty() && !s.eq_ignore_ascii_case("unknown"));

        let ch_title = toc_labels
            .get(&item.href)
            .cloned()
            .or(heading_fallback)
            .unwrap_or_else(|| format!("Section {}", chapters.len() + 1));

        let num = (chapters.len() + 1) as f64;
        chapters.push(ParsedChapter {
            chapter_number: num,
            title: ch_title,
            html: clean_html,
            paragraphs,
            word_count,
        });
    }

    if chapters.is_empty() {
        return Err(AppError::InvalidInput(
            "EPUB contained no readable text chapters".into(),
        ));
    }

    Ok(ParsedBook {
        title,
        author,
        description,
        genres,
        tags,
        rating,
        language,
        series,
        series_index,
        cover_bytes,
        cover_ext,
        chapters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn build_sample_calibre_epub() -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();

            zip.start_file("mimetype", opts).unwrap();
            zip.write_all(b"application/epub+zip").unwrap();

            zip.start_file("META-INF/container.xml", opts).unwrap();
            zip.write_all(
                br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();

            zip.start_file("OEBPS/content.opf", opts).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uuid_id" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>Starlight Chronicles</dc:title>
    <dc:creator opf:role="aut">Elena Vance</dc:creator>
    <dc:description>A journey across the outer rim.</dc:description>
    <dc:subject>Science Fiction</dc:subject>
    <dc:subject>Space Opera</dc:subject>
    <dc:language>en</dc:language>
    <meta name="calibre:series" content="Starlight Saga"/>
    <meta name="calibre:series_index" content="1.0"/>
    <meta name="calibre:rating" content="9"/>
    <meta name="cover" content="cover-img"/>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="cover-img" href="images/cover.png" media-type="image/png"/>
    <item id="ch1" href="text/ch01.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="text/ch02.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#,
            )
            .unwrap();

            zip.start_file("OEBPS/toc.ncx", opts).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="n1" playOrder="1">
      <navLabel><text>Prologue: Signal</text></navLabel>
      <content src="text/ch01.xhtml"/>
    </navPoint>
    <navPoint id="n2" playOrder="2">
      <navLabel><text>Chapter 1: Departure</text></navLabel>
      <content src="text/ch02.xhtml"/>
    </navPoint>
  </navMap>
</ncx>"#,
            )
            .unwrap();

            zip.start_file("OEBPS/images/cover.png", opts).unwrap();
            zip.write_all(b"\x89PNG\r\n\x1a\nFAKEPNG").unwrap();

            zip.start_file("OEBPS/text/ch01.xhtml", opts).unwrap();
            zip.write_all(
                br#"<html><body><h1>Ignored Heading</h1><p>First beacon pulsed across the void.</p><p>The crew listened quietly.</p></body></html>"#,
            )
            .unwrap();

            zip.start_file("OEBPS/text/ch02.xhtml", opts).unwrap();
            zip.write_all(
                br#"<html><body><p>Thrusters ignited at dawn.</p><script>evil()</script></body></html>"#,
            )
            .unwrap();

            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn test_parse_calibre_epub_metadata_toc_and_cover() {
        let epub_bytes = build_sample_calibre_epub();
        let parsed = parse_epub_bytes(&epub_bytes, "Fallback").unwrap();

        assert_eq!(parsed.title, "Starlight Chronicles");
        assert_eq!(parsed.author, "Elena Vance");
        assert_eq!(parsed.description, "A journey across the outer rim.");
        assert_eq!(parsed.genres, vec!["Science Fiction", "Space Opera"]);
        assert_eq!(parsed.series.as_deref(), Some("Starlight Saga"));
        assert_eq!(parsed.series_index, Some(1.0));
        assert_eq!(parsed.rating, Some(4.5));
        assert_eq!(parsed.cover_ext.as_deref(), Some("png"));
        assert!(parsed.cover_bytes.is_some());

        assert_eq!(parsed.chapters.len(), 2);
        assert_eq!(parsed.chapters[0].title, "Prologue: Signal");
        assert_eq!(parsed.chapters[0].paragraphs.len(), 3);
        assert_eq!(parsed.chapters[1].title, "Chapter 1: Departure");
        assert!(!parsed.chapters[1].html.contains("script"));
        assert!(!parsed.chapters[1].paragraphs.join(" ").contains("evil"));
    }

    #[test]
    fn test_decode_percent_preserves_plus_and_ampersand() {
        assert_eq!(
            decode_percent("OEBPS/Part+1%20(A&B)%2Fch1.xhtml"),
            "OEBPS/Part+1 (A&B)/ch1.xhtml"
        );
    }
}
