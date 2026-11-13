use std::collections::HashSet;
use std::path::Path;
use futures::future::join_all;
use tauri::State;

use crate::downloads::StoredChapterPayload;
use crate::error::{AppError, AppResult};
use crate::models::{
    is_placeholder_author, Chapter, ChapterContent, DiscoveryPage, DiscoveryQuery,
    NovelDetailsResponse, NovelRecord, NovelSummary, NovelTranslation, SourceErrorBanner,
};
use crate::state::AppState;

#[tauri::command]
pub async fn discover_novels(
    state: State<'_, AppState>,
    query: DiscoveryQuery,
) -> AppResult<DiscoveryPage> {
    let active = state
        .sources
        .enabled_sources_with_config(&state.db, query.source_id.as_deref())
        .await?;

    if active.is_empty() {
        return Ok(DiscoveryPage {
            items: Vec::new(),
            page: query.page,
            has_more: false,
            total_estimate: Some(0),
            source_errors: vec![SourceErrorBanner {
                source_id: "none".into(),
                source_name: "Sources".into(),
                message: "No enabled sources match the current filter. Enable a source in Settings → Sources.".into(),
            }],
        });
    }

    let futures = active.into_iter().map(|(src, cfg)| {
        let q = query.clone();
        async move {
            let sid = src.id().to_string();
            let sname = src.name().to_string();
            let res = src.discover(&q, &cfg).await;
            (sid, sname, res)
        }
    });

    let results = join_all(futures).await;
    let mut all_items: Vec<NovelSummary> = Vec::new();
    let mut any_more = false;
    let mut total_est: Option<u64> = None;
    let mut source_errors = Vec::new();

    for (sid, sname, res) in results {
        match res {
            Ok((items, has_more, est)) => {
                let _ = state.db.update_source_health(&sid, "online");
                if has_more {
                    any_more = true;
                }
                if let Some(n) = est {
                    total_est = Some(total_est.unwrap_or(0) + n);
                }
                all_items.extend(items);
            }
            Err(e) => {
                let _ = state.db.update_source_health(&sid, "degraded");
                tracing::warn!("Source '{}' discovery error: {}", sid, e);
                source_errors.push(SourceErrorBanner {
                    source_id: sid,
                    source_name: sname,
                    message: e.to_string(),
                });
            }
        }
    }

    if let Some(req_lang) = query
        .language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("all"))
    {
        let req_low = req_lang.to_ascii_lowercase();
        let req_prefix = format!("{req_low}-");
        all_items.retain(|it| {
            it.language.as_deref().is_some_and(|l| {
                let low = l.trim().to_ascii_lowercase();
                low == req_low || low.starts_with(&req_prefix)
            })
        });
    }

    // Fill missing authors on browse cards from previously scraped SQLite records.
    for item in &mut all_items {
        if is_placeholder_author(&item.author) {
            if let Ok(Some(cached)) = state.db.get_novel(&item.id) {
                if !is_placeholder_author(&cached.author) {
                    item.author = cached.author;
                }
            }
        }
    }

    // Interleave or sort cleanly across sources
    match query.sort.as_deref() {
        Some("title") => all_items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        _ => all_items.sort_by(|a, b| b.popularity.cmp(&a.popularity)),
    }

    Ok(DiscoveryPage {
        items: all_items,
        page: query.page,
        has_more: any_more,
        total_estimate: total_est,
        source_errors,
    })
}

#[tauri::command]
pub async fn cache_novel_summary(
    state: State<'_, AppState>,
    summary: NovelSummary,
) -> AppResult<NovelRecord> {
    state.db.upsert_novel_from_summary(&summary)
}

#[tauri::command]
pub async fn get_novel_details(
    state: State<'_, AppState>,
    novel_id: String,
    refresh_remote: Option<bool>,
) -> AppResult<NovelDetailsResponse> {
    let should_refresh = refresh_remote.unwrap_or(false);
    let mut novel = state.db.get_novel(&novel_id)?;
    let mut chapters = state.db.get_chapters(&novel_id)?;

    // Parse source_id & external_id if not yet cached in SQLite
    let (source_id, external_id) = if let Some(ref n) = novel {
        (n.source_id.clone(), n.external_id.clone())
    } else if let Some((s, ext)) = novel_id.split_once(':') {
        (s.to_string(), ext.to_string())
    } else {
        return Err(AppError::NotFound(format!("Novel '{novel_id}' not found")));
    };

    let is_local = novel.as_ref().map(|n| n.is_local).unwrap_or(false);
    let missing_author = novel
        .as_ref()
        .is_some_and(|n| is_placeholder_author(&n.author));

    if !is_local && (novel.is_none() || chapters.is_empty() || should_refresh || missing_author) {
        if let Some(src) = state.sources.get_source(&source_id).await {
            let cfg = state
                .sources
                .get_source_info(&state.db, &source_id)
                .await?
                .map(|i| i.config)
                .unwrap_or_default();

            if novel.is_none() || chapters.is_empty() || should_refresh || missing_author {
                match src.get_novel(&external_id, &cfg).await {
                    Ok(remote_novel) => {
                        novel = Some(state.db.upsert_novel(&remote_novel)?);
                    }
                    Err(e) => {
                        if novel.is_none() {
                            return Err(e);
                        }
                        tracing::warn!("Could not refresh remote novel metadata: {}", e);
                    }
                }
            }

            if chapters.is_empty() || should_refresh {
                match src.get_chapters(&external_id, &cfg).await {
                    Ok(remote_chapters) if !remote_chapters.is_empty() => {
                        state.db.upsert_chapters(&novel_id, &remote_chapters)?;
                        chapters = state.db.get_chapters(&novel_id)?;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        if chapters.is_empty() {
                            return Err(e);
                        }
                        tracing::warn!("Could not refresh chapters for {}: {}", novel_id, e);
                    }
                }
            }
        }
    }

    let novel = novel.ok_or_else(|| AppError::NotFound(format!("Novel '{novel_id}' not found")))?;

    // Background cover cache for fast offline display
    if novel.local_cover_path.is_none() {
        if let Some(ref curl) = novel.cover_url {
            let mgr = state.downloads.clone();
            let nid = novel.id.clone();
            let u = curl.clone();
            tokio::spawn(async move {
                let _ = mgr.cache_novel_cover(&nid, &u).await;
            });
        }
    }

    let library_entry = state.db.get_library_entry(&novel.id)?;
    let reading_progress = state.db.get_reading_progress(&novel.id)?;
    let bookmarks = state.db.list_bookmarks(Some(&novel.id))?;
    let highlights = state.db.list_highlights(&novel.id, None)?;
    let collection_ids = state.db.get_novel_collection_ids(&novel.id)?;

    let mut translations: Vec<NovelTranslation> = Vec::new();
    let mut seen_ids = HashSet::new();
    if !novel.is_local {
        if let Some(src) = state.sources.get_source(&novel.source_id).await {
            let cfg = state
                .sources
                .get_source_info(&state.db, &novel.source_id)
                .await?
                .map(|i| i.config)
                .unwrap_or_default();
            if let Ok(remote_trans) = src.get_translations(&novel.external_id, &cfg).await {
                for t in remote_trans {
                    if seen_ids.insert(t.novel_id.clone()) {
                        translations.push(t);
                    }
                }
            }
        }
    }
    if let Ok(local_trans) = state.db.find_local_translations(&novel) {
        for t in local_trans {
            if seen_ids.insert(t.novel_id.clone()) {
                translations.push(t);
            }
        }
    }
    if !translations.is_empty() && seen_ids.insert(novel.id.clone()) {
        let lang = novel
            .metadata
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("en")
            .to_ascii_lowercase();
        translations.insert(
            0,
            NovelTranslation {
                language_label: lang.to_uppercase(),
                language_code: lang,
                novel_id: novel.id.clone(),
                source_id: novel.source_id.clone(),
                external_id: novel.external_id.clone(),
                title: novel.title.clone(),
            },
        );
    }

    let downloaded_chapters = chapters.iter().filter(|c| c.is_downloaded).count() as i64;
    let total_size_bytes: i64 = chapters.iter().map(|c| c.size_bytes).sum();

    Ok(NovelDetailsResponse {
        novel,
        chapters,
        library_entry,
        reading_progress,
        bookmarks,
        highlights,
        translations,
        collection_ids,
        downloaded_chapters,
        total_size_bytes,
    })
}

#[tauri::command]
pub async fn get_chapter_content(
    state: State<'_, AppState>,
    novel_id: String,
    chapter_id: String,
) -> AppResult<ChapterContent> {
    let novel = state
        .db
        .get_novel(&novel_id)?
        .ok_or_else(|| AppError::NotFound(format!("Novel '{novel_id}' not found")))?;

    let chapters: Vec<Chapter> = state.db.get_chapters(&novel_id)?;
    let idx = chapters
        .iter()
        .position(|c| c.id == chapter_id)
        .ok_or_else(|| AppError::NotFound(format!("Chapter '{chapter_id}' not found")))?;
    let ch = &chapters[idx];

    let prev_chapter_id = if idx > 0 {
        Some(chapters[idx - 1].id.clone())
    } else {
        None
    };
    let next_chapter_id = if idx + 1 < chapters.len() {
        Some(chapters[idx + 1].id.clone())
    } else {
        None
    };

    // 1. Offline-first: read local downloaded file if available
    if ch.is_downloaded {
        if let Some(ref lp) = ch.local_path {
            if Path::new(lp).exists() {
                let payload = state.downloads.read_offline_chapter(lp)?;
                return Ok(ChapterContent {
                    chapter_id: ch.id.clone(),
                    novel_id,
                    chapter_number: ch.chapter_number,
                    title: ch.title.clone(),
                    html: payload.html,
                    paragraphs: payload.paragraphs,
                    word_count: payload.word_count,
                    is_downloaded: true,
                    prev_chapter_id,
                    next_chapter_id,
                });
            }
        }
    }

    // 1.5. Check disk cache from previous online reading session
    if let Some(payload) = state.downloads.read_cached_chapter(&novel_id, &ch.id) {
        return Ok(ChapterContent {
            chapter_id: ch.id.clone(),
            novel_id,
            chapter_number: ch.chapter_number,
            title: ch.title.clone(),
            html: payload.html,
            paragraphs: payload.paragraphs,
            word_count: payload.word_count,
            is_downloaded: false,
            prev_chapter_id,
            next_chapter_id,
        });
    }

    // 2. Otherwise fetch online from source
    let src = state
        .sources
        .get_source(&novel.source_id)
        .await
        .ok_or_else(|| {
            AppError::Source {
                source_id: novel.source_id.clone(),
                message: "Chapter is not downloaded locally and source is unavailable offline.".into(),
            }
        })?;

    let cfg = state
        .sources
        .get_source_info(&state.db, &novel.source_id)
        .await?
        .map(|i| i.config)
        .unwrap_or_default();

    let (html, paragraphs, word_count) = src
        .get_chapter_content(&novel.external_id, &ch.external_id, ch.url.as_deref(), &cfg)
        .await?;

    state.downloads.write_cached_chapter(&StoredChapterPayload {
        chapter_id: ch.id.clone(),
        novel_id: novel_id.clone(),
        chapter_number: ch.chapter_number,
        title: ch.title.clone(),
        html: html.clone(),
        paragraphs: paragraphs.clone(),
        word_count,
    });

    Ok(ChapterContent {
        chapter_id: ch.id.clone(),
        novel_id,
        chapter_number: ch.chapter_number,
        title: ch.title.clone(),
        html,
        paragraphs,
        word_count,
        is_downloaded: ch.is_downloaded,
        prev_chapter_id,
        next_chapter_id,
    })
}

#[tauri::command]
pub async fn cache_cover_image(
    state: State<'_, AppState>,
    url: String,
    novel_id: Option<String>,
) -> AppResult<Option<String>> {
    let clean_url = url.trim();
    if clean_url.is_empty()
        || (!clean_url.starts_with("http://") && !clean_url.starts_with("https://"))
    {
        return Ok(None);
    }
    if let Some(nid) = novel_id.as_deref().filter(|s| !s.trim().is_empty()) {
        if let Some(existing) = state.db.get_novel(nid)?.and_then(|n| n.local_cover_path) {
            if Path::new(&existing).exists() {
                return Ok(Some(existing));
            }
        }
        return Ok(state.downloads.cache_novel_cover(nid, clean_url).await);
    }
    Ok(state.downloads.cache_image_url(clean_url).await)
}

#[tauri::command]
pub async fn search_local_novels(
    state: State<'_, AppState>,
    query: String,
) -> AppResult<Vec<NovelRecord>> {
    state.db.search_local_novels(&query, 20)
}
