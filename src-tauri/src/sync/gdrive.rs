use std::collections::HashMap;
use std::path::Path;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;
use url::Url;
use uuid::Uuid;

use crate::db::Database;
use crate::downloads::DownloadManager;
use crate::error::{AppError, AppResult};
use crate::models::{
    GoogleDriveOAuthStartResult, GoogleDriveSyncConfig, GoogleDriveSyncReport,
    GoogleDriveSyncStatus, LibraryExportBundle,
};
use crate::sync::{
    compute_novel_archive_digest, novel_archive_filename, pack_novel_archive,
    parse_google_drive_folder_id, unpack_novel_archive, GoogleDriveSyncCoordinator,
    PendingOAuthSession,
};

const GOOGLE_AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_ENDPOINT: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const GOOGLE_DRIVE_ABOUT_ENDPOINT: &str = "https://www.googleapis.com/drive/v3/about";
const GOOGLE_DRIVE_FILES_ENDPOINT: &str = "https://www.googleapis.com/drive/v3/files";
const GOOGLE_DRIVE_UPLOAD_ENDPOINT: &str = "https://www.googleapis.com/upload/drive/v3/files";
// Request ONLY the `drive` scope (without `userinfo.email` or `include_granted_scopes=true`)
// so Google OAuth does not issue a partial `userinfo.email`-only token that fails with
// 403 ACCESS_TOKEN_SCOPE_INSUFFICIENT on user-created Drive folders.
const GOOGLE_OAUTH_SCOPES: &str = "https://www.googleapis.com/auth/drive";

const LIBRARY_BUNDLE_FILENAME: &str = "novelium-library.json";
const BOOKS_SUBFOLDER_NAME: &str = "books";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const OAUTH_TIMEOUT_SECS: u64 = 180;

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenErrorResponse {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleUserInfo {
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DriveAboutResponse {
    #[serde(default)]
    user: Option<DriveAboutUser>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveAboutUser {
    #[serde(default)]
    email_address: Option<String>,
}

fn has_granted_drive_scope(scope_str: &str) -> bool {
    scope_str.split_whitespace().any(|s| {
        s == "https://www.googleapis.com/auth/drive"
            || s == "https://www.googleapis.com/auth/drive.file"
            || s.ends_with("/auth/drive")
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFileItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub trashed: Option<bool>,
    #[serde(default)]
    pub app_properties: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveFileListResponse {
    #[serde(default)]
    files: Vec<DriveFileItem>,
    #[serde(default)]
    next_page_token: Option<String>,
}

fn generate_pkce_verifier() -> String {
    let mut raw = Vec::with_capacity(48);
    raw.extend_from_slice(Uuid::new_v4().as_bytes());
    raw.extend_from_slice(Uuid::new_v4().as_bytes());
    raw.extend_from_slice(Uuid::new_v4().as_bytes());
    URL_SAFE_NO_PAD.encode(raw)
}

fn compute_pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn build_google_auth_url(
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
) -> AppResult<String> {
    let mut url = Url::parse(GOOGLE_AUTH_ENDPOINT)
        .map_err(|e| AppError::InvalidInput(format!("Invalid Google auth URL: {e}")))?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id.trim())
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", GOOGLE_OAUTH_SCOPES)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    Ok(url.to_string())
}

fn open_system_browser(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn().is_ok()
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .is_ok()
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios")))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn().is_ok()
    }
    #[cfg(target_os = "ios")]
    {
        let _ = url;
        false
    }
}

fn oauth_html_page(title: &str, detail: &str, is_error: bool) -> String {
    let accent = if is_error { "#f43f5e" } else { "#10b981" };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title>\
        <style>body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;\
        background:#0d0f13;color:#e5e7eb;display:flex;align-items:center;justify-content:center;\
        min-height:100vh;margin:0}} .card{{max-width:440px;padding:32px;border-radius:16px;\
        border:1px solid rgba(255,255,255,.12);background:#161922;text-align:center}}\
        h1{{margin:0 0 12px;font-size:20px;color:{accent}}} p{{margin:0;line-height:1.5;color:#9ca3af;font-size:14px}}</style>\
        </head><body><div class=\"card\"><h1>{title}</h1><p>{detail}</p></div></body></html>"
    )
}

fn parse_oauth_callback_input(raw_input: &str, expected_state: Option<&str>) -> AppResult<String> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(
            "Paste the Google redirect URL or authorization code first.".into(),
        ));
    }

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with('/') {
        let base_url = if trimmed.starts_with('/') {
            format!("http://127.0.0.1{trimmed}")
        } else {
            trimmed.to_string()
        };
        let parsed = Url::parse(&base_url).map_err(|e| {
            AppError::InvalidInput(format!("Invalid OAuth callback URL: {e}"))
        })?;
        let mut code = None;
        let mut state = None;
        let mut scope = None;
        let mut err = None;
        let mut err_desc = None;
        for (k, v) in parsed.query_pairs() {
            match k.as_ref() {
                "code" => code = Some(v.into_owned()),
                "state" => state = Some(v.into_owned()),
                "scope" => scope = Some(v.into_owned()),
                "error" => err = Some(v.into_owned()),
                "error_description" => err_desc = Some(v.into_owned()),
                _ => {}
            }
        }
        if let Some(e_code) = err {
            let msg = err_desc.unwrap_or_else(|| e_code.clone());
            return Err(AppError::InvalidInput(format!(
                "Google authorization failed: {msg}"
            )));
        }
        if let Some(expected) = expected_state {
            if state.as_deref() != Some(expected) {
                return Err(AppError::InvalidInput(
                    "OAuth state mismatch; please restart connecting Google Drive.".into(),
                ));
            }
        }
        if let Some(ref granted_scope) = scope {
            if !granted_scope.trim().is_empty() && !has_granted_drive_scope(granted_scope) {
                return Err(AppError::InvalidInput(
                    "Google Drive permission was not checked on Google's sign-in screen. Please click 'Connect Google Drive' again and check the box 'See, edit, create, and delete all of your Google Drive files' before clicking Continue.".into(),
                ));
            }
        }
        return code
            .filter(|c| !c.trim().is_empty())
            .ok_or_else(|| AppError::InvalidInput("Missing 'code' parameter in OAuth callback.".into()));
    }

    Ok(trimmed.to_string())
}

fn build_multipart_related_body(
    metadata_json: &str,
    mime_type: &str,
    content_bytes: &[u8],
    boundary: &str,
) -> Vec<u8> {
    let mut body = Vec::with_capacity(metadata_json.len() + content_bytes.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata_json}\r\n\
             --{boundary}\r\nContent-Type: {mime_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(content_bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

async fn post_token_form(
    http: &reqwest::Client,
    pairs: &[(&str, &str)],
    action_label: &str,
) -> AppResult<GoogleTokenResponse> {
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs.iter().copied())
        .finish();
    let resp = http
        .post(GOOGLE_TOKEN_ENDPOINT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(encoded)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        let detail = serde_json::from_str::<GoogleTokenErrorResponse>(&text)
            .ok()
            .and_then(|e| e.error_description.or(e.error))
            .unwrap_or_else(|| text.clone());
        return Err(AppError::Source {
            source_id: "gdrive".into(),
            message: format!("{action_label} ({status}): {detail}"),
        });
    }

    serde_json::from_str::<GoogleTokenResponse>(&text).map_err(|e| AppError::Source {
        source_id: "gdrive".into(),
        message: format!("Invalid Google token response: {e}"),
    })
}

async fn exchange_auth_code_for_tokens(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> AppResult<GoogleTokenResponse> {
    let mut form: Vec<(&str, &str)> = vec![
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", code_verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ];
    if !client_secret.trim().is_empty() {
        form.push(("client_secret", client_secret.trim()));
    }

    post_token_form(http, &form, "OAuth token exchange failed").await
}

async fn refresh_access_token(
    http: &reqwest::Client,
    cfg: &mut GoogleDriveSyncConfig,
    db: &Database,
) -> AppResult<String> {
    let refresh_token = cfg
        .refresh_token
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            AppError::InvalidInput("Google Drive session expired. Please reconnect Google Drive.".into())
        })?
        .to_string();

    if cfg.client_id.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Google OAuth Client ID is missing.".into(),
        ));
    }

    let mut form: Vec<(&str, &str)> = vec![
        ("client_id", cfg.client_id.trim()),
        ("refresh_token", &refresh_token),
        ("grant_type", "refresh_token"),
    ];
    if !cfg.client_secret.trim().is_empty() {
        form.push(("client_secret", cfg.client_secret.trim()));
    }

    let tok = post_token_form(http, &form, "Failed refreshing Google Drive token").await?;

    cfg.access_token = Some(tok.access_token.clone());
    if let Some(new_refresh) = tok.refresh_token {
        if !new_refresh.trim().is_empty() {
            cfg.refresh_token = Some(new_refresh);
        }
    }
    let expires_in = tok.expires_in.unwrap_or(3600).max(60);
    cfg.token_expires_at = Some((Utc::now() + Duration::seconds(expires_in)).to_rfc3339());
    db.save_gdrive_config(cfg)?;
    Ok(tok.access_token)
}

async fn ensure_valid_access_token(
    http: &reqwest::Client,
    cfg: &mut GoogleDriveSyncConfig,
    db: &Database,
) -> AppResult<String> {
    let not_expiring_soon = cfg
        .token_expires_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|exp| exp.with_timezone(&Utc) > Utc::now() + Duration::seconds(60))
        .unwrap_or(false);

    if not_expiring_soon {
        if let Some(ref tok) = cfg.access_token {
            if !tok.trim().is_empty() {
                return Ok(tok.clone());
            }
        }
    }

    refresh_access_token(http, cfg, db).await
}

async fn fetch_user_email(http: &reqwest::Client, access_token: &str) -> Option<String> {
    if let Ok(resp) = http
        .get(GOOGLE_DRIVE_ABOUT_ENDPOINT)
        .query(&[("fields", "user(emailAddress)")])
        .bearer_auth(access_token)
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(about) = resp.json::<DriveAboutResponse>().await {
                if let Some(email) = about.user.and_then(|u| u.email_address) {
                    if !email.trim().is_empty() {
                        return Some(email);
                    }
                }
            }
        }
    }

    let resp = http
        .get(GOOGLE_USERINFO_ENDPOINT)
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<GoogleUserInfo>().await.ok()?.email
}

fn check_insufficient_scope_error(
    status: reqwest::StatusCode,
    body: &str,
    cfg: &mut GoogleDriveSyncConfig,
    db: &Database,
) -> Option<AppError> {
    if status.as_u16() == 403
        && (body.contains("ACCESS_TOKEN_SCOPE_INSUFFICIENT")
            || body.contains("insufficientPermissions")
            || body.contains("Request had insufficient authentication scopes"))
    {
        cfg.access_token = None;
        cfg.refresh_token = None;
        cfg.token_expires_at = None;
        let _ = db.save_gdrive_config(cfg);
        return Some(AppError::InvalidInput(
            "Google sign-in did not grant Google Drive access (403 Insufficient Permission). Please click 'Connect Google Drive' again and check the box 'See, edit, create, and delete all of your Google Drive files' on Google's consent screen.".into(),
        ));
    }
    None
}

async fn verify_drive_folder(
    http: &reqwest::Client,
    cfg: &mut GoogleDriveSyncConfig,
    db: &Database,
    folder_id: &str,
) -> AppResult<DriveFileItem> {
    let mut token = ensure_valid_access_token(http, cfg, db).await?;
    let url = format!("{GOOGLE_DRIVE_FILES_ENDPOINT}/{folder_id}");

    for attempt in 0..2 {
        let resp = http
            .get(&url)
            .query(&[
                ("fields", "id,name,mimeType,trashed"),
                ("supportsAllDrives", "true"),
            ])
            .bearer_auth(&token)
            .send()
            .await?;

        if resp.status().as_u16() == 401 && attempt == 0 {
            token = refresh_access_token(http, cfg, db).await?;
            continue;
        }
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if let Some(scope_err) = check_insufficient_scope_error(status, &body, cfg, db) {
                return Err(scope_err);
            }
            return Err(AppError::Source {
                source_id: "gdrive".into(),
                message: format!(
                    "Cannot access Google Drive folder '{folder_id}' ({status}): {body}"
                ),
            });
        }
        let item: DriveFileItem = resp.json().await?;
        if item.trashed.unwrap_or(false) {
            return Err(AppError::InvalidInput(format!(
                "Google Drive folder '{}' is in Trash.",
                item.name
            )));
        }
        if item.mime_type != FOLDER_MIME {
            return Err(AppError::InvalidInput(format!(
                "Drive item '{}' is not a folder (mimeType={}).",
                item.name, item.mime_type
            )));
        }
        return Ok(item);
    }

    Err(AppError::Source {
        source_id: "gdrive".into(),
        message: "Failed authenticating Google Drive folder request.".into(),
    })
}

async fn list_drive_folder_files(
    http: &reqwest::Client,
    cfg: &mut GoogleDriveSyncConfig,
    db: &Database,
    parent_id: &str,
) -> AppResult<Vec<DriveFileItem>> {
    let mut token = ensure_valid_access_token(http, cfg, db).await?;
    let escaped_parent = parent_id.replace('\'', "\\'");
    let q = format!("'{escaped_parent}' in parents and trashed = false");
    let mut all = Vec::new();
    let mut page_token: Option<String> = None;

    loop {
        let mut resp_opt = None;
        for attempt in 0..2 {
            let mut req = http
                .get(GOOGLE_DRIVE_FILES_ENDPOINT)
                .query(&[
                    ("q", q.as_str()),
                    ("fields", "nextPageToken,files(id,name,mimeType,trashed,appProperties)"),
                    ("pageSize", "1000"),
                    ("supportsAllDrives", "true"),
                    ("includeItemsFromAllDrives", "true"),
                ])
                .bearer_auth(&token);
            if let Some(ref pt) = page_token {
                req = req.query(&[("pageToken", pt.as_str())]);
            }
            let resp = req.send().await?;
            if resp.status().as_u16() == 401 && attempt == 0 {
                token = refresh_access_token(http, cfg, db).await?;
                continue;
            }
            resp_opt = Some(resp);
            break;
        }
        let resp = resp_opt.ok_or_else(|| AppError::Source {
            source_id: "gdrive".into(),
            message: "Failed listing Google Drive folder.".into(),
        })?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Source {
                source_id: "gdrive".into(),
                message: format!("Google Drive list failed ({status}): {body}"),
            });
        }
        let parsed: DriveFileListResponse = resp.json().await?;
        all.extend(parsed.files);
        match parsed.next_page_token {
            Some(tok) if !tok.is_empty() => page_token = Some(tok),
            _ => break,
        }
    }

    Ok(all)
}

async fn create_drive_subfolder(
    http: &reqwest::Client,
    cfg: &mut GoogleDriveSyncConfig,
    db: &Database,
    parent_id: &str,
    name: &str,
) -> AppResult<DriveFileItem> {
    let mut token = ensure_valid_access_token(http, cfg, db).await?;
    let payload = serde_json::json!({
        "name": name,
        "mimeType": FOLDER_MIME,
        "parents": [parent_id],
    });

    for attempt in 0..2 {
        let resp = http
            .post(GOOGLE_DRIVE_FILES_ENDPOINT)
            .query(&[
                ("supportsAllDrives", "true"),
                ("fields", "id,name,mimeType,trashed,appProperties"),
            ])
            .bearer_auth(&token)
            .json(&payload)
            .send()
            .await?;
        if resp.status().as_u16() == 401 && attempt == 0 {
            token = refresh_access_token(http, cfg, db).await?;
            continue;
        }
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Source {
                source_id: "gdrive".into(),
                message: format!("Failed creating subfolder '{name}' on Drive ({status}): {body}"),
            });
        }
        return Ok(resp.json().await?);
    }

    Err(AppError::Source {
        source_id: "gdrive".into(),
        message: format!("Failed creating subfolder '{name}' on Drive"),
    })
}

async fn download_drive_file(
    http: &reqwest::Client,
    cfg: &mut GoogleDriveSyncConfig,
    db: &Database,
    file_id: &str,
) -> AppResult<Vec<u8>> {
    let mut token = ensure_valid_access_token(http, cfg, db).await?;
    let url = format!("{GOOGLE_DRIVE_FILES_ENDPOINT}/{file_id}");

    for attempt in 0..2 {
        let resp = http
            .get(&url)
            .query(&[("alt", "media"), ("supportsAllDrives", "true")])
            .bearer_auth(&token)
            .send()
            .await?;
        if resp.status().as_u16() == 401 && attempt == 0 {
            token = refresh_access_token(http, cfg, db).await?;
            continue;
        }
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Source {
                source_id: "gdrive".into(),
                message: format!("Failed downloading Drive file '{file_id}' ({status}): {body}"),
            });
        }
        return Ok(resp.bytes().await?.to_vec());
    }

    Err(AppError::Source {
        source_id: "gdrive".into(),
        message: format!("Failed downloading Drive file '{file_id}'"),
    })
}

struct DriveUploadSpec<'a> {
    existing_file_id: Option<&'a str>,
    parent_id: &'a str,
    filename: &'a str,
    mime_type: &'a str,
    description: Option<&'a str>,
    app_properties: Option<serde_json::Value>,
    content_bytes: &'a [u8],
}

async fn upload_or_update_drive_file(
    http: &reqwest::Client,
    cfg: &mut GoogleDriveSyncConfig,
    db: &Database,
    spec: DriveUploadSpec<'_>,
) -> AppResult<DriveFileItem> {
    let mut token = ensure_valid_access_token(http, cfg, db).await?;

    let mut meta = serde_json::Map::new();
    meta.insert("name".into(), serde_json::json!(spec.filename));
    meta.insert("mimeType".into(), serde_json::json!(spec.mime_type));
    if spec.existing_file_id.is_none() {
        // Drive API forbids including `parents` on PATCH updates
        meta.insert("parents".into(), serde_json::json!([spec.parent_id]));
    }
    if let Some(desc) = spec.description {
        meta.insert("description".into(), serde_json::json!(desc));
    }
    if let Some(props) = spec.app_properties {
        meta.insert("appProperties".into(), props);
    }
    let meta_json = serde_json::to_string(&meta)?;

    let boundary = format!("novelium_sync_{}", Uuid::new_v4().simple());
    let body = build_multipart_related_body(
        &meta_json,
        spec.mime_type,
        spec.content_bytes,
        &boundary,
    );
    let content_type = format!("multipart/related; boundary={boundary}");

    for attempt in 0..2 {
        let req = match spec.existing_file_id {
            Some(fid) => http.patch(format!("{GOOGLE_DRIVE_UPLOAD_ENDPOINT}/{fid}")),
            None => http.post(GOOGLE_DRIVE_UPLOAD_ENDPOINT),
        };
        let resp = req
            .query(&[
                ("uploadType", "multipart"),
                ("supportsAllDrives", "true"),
                ("fields", "id,name,mimeType,trashed,appProperties"),
            ])
            .bearer_auth(&token)
            .header("Content-Type", &content_type)
            .body(body.clone())
            .send()
            .await?;

        if resp.status().as_u16() == 401 && attempt == 0 {
            token = refresh_access_token(http, cfg, db).await?;
            continue;
        }
        let status = resp.status();
        if !status.is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(AppError::Source {
                source_id: "gdrive".into(),
                message: format!(
                    "Failed uploading '{}' to Drive ({status}): {err_body}",
                    spec.filename
                ),
            });
        }
        return Ok(resp.json().await?);
    }

    Err(AppError::Source {
        source_id: "gdrive".into(),
        message: format!("Failed uploading '{}' to Drive", spec.filename),
    })
}

struct OAuthExchangeInput<'a> {
    code: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
    code_verifier: &'a str,
    redirect_uri: &'a str,
}

async fn finish_oauth_exchange(
    coordinator: &GoogleDriveSyncCoordinator,
    db: &Database,
    input: OAuthExchangeInput<'_>,
    app_handle: Option<&AppHandle>,
) -> AppResult<GoogleDriveSyncStatus> {
    let tok = exchange_auth_code_for_tokens(
        &coordinator.http,
        input.client_id,
        input.client_secret,
        input.code,
        input.code_verifier,
        input.redirect_uri,
    )
    .await?;

    if let Some(ref granted_scope) = tok.scope {
        if !granted_scope.trim().is_empty() && !has_granted_drive_scope(granted_scope) {
            return Err(AppError::InvalidInput(
                "Google Drive permission was not checked on Google's sign-in screen. Please click 'Connect Google Drive' again and check the box 'See, edit, create, and delete all of your Google Drive files' before clicking Continue.".into(),
            ));
        }
    }

    let email = fetch_user_email(&coordinator.http, &tok.access_token).await;

    let mut cfg = db.load_gdrive_config()?;
    cfg.client_id = input.client_id.trim().to_string();
    if !input.client_secret.trim().is_empty() {
        cfg.client_secret = input.client_secret.trim().to_string();
    }
    cfg.access_token = Some(tok.access_token);
    if let Some(rt) = tok.refresh_token {
        if !rt.trim().is_empty() {
            cfg.refresh_token = Some(rt);
        }
    }
    let expires_in = tok.expires_in.unwrap_or(3600).max(60);
    cfg.token_expires_at = Some((Utc::now() + Duration::seconds(expires_in)).to_rfc3339());
    if email.is_some() {
        cfg.connected_email = email;
    }

    // Verify configured Drive folder right away if present
    if !cfg.folder_id.trim().is_empty() {
        let fid = cfg.folder_id.clone();
        if let Ok(folder) = verify_drive_folder(&coordinator.http, &mut cfg, db, &fid).await {
            cfg.folder_name = Some(folder.name);
        }
    }

    db.save_gdrive_config(&cfg)?;
    let status = GoogleDriveSyncCoordinator::redact_status(&cfg);
    if let Some(app) = app_handle {
        let _ = app.emit("gdrive-sync-updated", &status);
    }
    Ok(status)
}

async fn write_http_response(
    stream: &mut tokio::net::TcpStream,
    status_line: &str,
    content_type: &str,
    body: &str,
) {
    let raw = format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(raw.as_bytes()).await;
    let _ = stream.flush().await;
}

impl GoogleDriveSyncCoordinator {
    pub async fn start_oauth(
        &self,
        db: Database,
        app_handle: Option<AppHandle>,
    ) -> AppResult<GoogleDriveOAuthStartResult> {
        let cfg = db.load_gdrive_config()?;
        let client_id = cfg.client_id.trim().to_string();
        if client_id.is_empty() {
            return Err(AppError::InvalidInput(
                "Enter and save your Google OAuth Client ID before connecting.".into(),
            ));
        }

        // Cancel any previous pending listener cleanly
        if let Some(prev) = self.pending_oauth.lock().await.take() {
            let _ = prev.cancel_tx.send(true);
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| {
            AppError::InvalidInput(format!("Failed starting local OAuth callback listener: {e}"))
        })?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}/oauth2callback");

        let code_verifier = generate_pkce_verifier();
        let code_challenge = compute_pkce_challenge(&code_verifier);
        let state = Uuid::new_v4().simple().to_string();
        let auth_url = build_google_auth_url(&client_id, &redirect_uri, &code_challenge, &state)?;

        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        {
            let mut guard = self.pending_oauth.lock().await;
            *guard = Some(PendingOAuthSession {
                state: state.clone(),
                code_verifier: code_verifier.clone(),
                redirect_uri: redirect_uri.clone(),
                client_id: client_id.clone(),
                client_secret: cfg.client_secret.clone(),
                cancel_tx,
            });
        }

        let coordinator = self.clone();
        let client_secret = cfg.client_secret.clone();
        let expected_state = state.clone();
        let redir = redirect_uri.clone();

        tokio::spawn(async move {
            let deadline = tokio::time::sleep(std::time::Duration::from_secs(OAUTH_TIMEOUT_SECS));
            tokio::pin!(deadline);

            loop {
                tokio::select! {
                    _ = &mut deadline => {
                        let _ = coordinator.pending_oauth.lock().await.take();
                        break;
                    }
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            break;
                        }
                    }
                    accepted = listener.accept() => {
                        let Ok((mut stream, _)) = accepted else {
                            break;
                        };
                        let mut buf = vec![0u8; 8192];
                        let n = stream.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            continue;
                        }
                        let req_str = String::from_utf8_lossy(&buf[..n]);
                        let first_line = req_str.lines().next().unwrap_or_default();
                        let path_and_query = first_line
                            .split_whitespace()
                            .nth(1)
                            .unwrap_or("/");

                        // Ignore browser favicon or prefetch noise so the single-callback flow survives
                        if !path_and_query.starts_with("/oauth2callback") {
                            write_http_response(&mut stream, "204 No Content", "text/plain", "").await;
                            continue;
                        }

                        match parse_oauth_callback_input(path_and_query, Some(&expected_state)) {
                            Ok(code) => {
                                let res = finish_oauth_exchange(
                                    &coordinator,
                                    &db,
                                    OAuthExchangeInput {
                                        code: &code,
                                        client_id: &client_id,
                                        client_secret: &client_secret,
                                        code_verifier: &code_verifier,
                                        redirect_uri: &redir,
                                    },
                                    app_handle.as_ref(),
                                )
                                .await;
                                match res {
                                    Ok(st) => {
                                        let who = st
                                            .connected_email
                                            .as_deref()
                                            .unwrap_or("your Google account");
                                        let html = oauth_html_page(
                                            "Google Drive Connected",
                                            &format!("Signed in as {who}. You can close this browser tab and return to Novelium."),
                                            false,
                                        );
                                        write_http_response(&mut stream, "200 OK", "text/html; charset=utf-8", &html).await;
                                    }
                                    Err(err) => {
                                        let html = oauth_html_page(
                                            "Connection Failed",
                                            &err.to_string(),
                                            true,
                                        );
                                        write_http_response(&mut stream, "400 Bad Request", "text/html; charset=utf-8", &html).await;
                                        if let Some(ref app) = app_handle {
                                            let _ = app.emit("gdrive-oauth-error", err.to_string());
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                let html = oauth_html_page("Authorization Error", &err.to_string(), true);
                                write_http_response(&mut stream, "400 Bad Request", "text/html; charset=utf-8", &html).await;
                                if let Some(ref app) = app_handle {
                                    let _ = app.emit("gdrive-oauth-error", err.to_string());
                                }
                            }
                        }

                        let _ = coordinator.pending_oauth.lock().await.take();
                        break;
                    }
                }
            }
        });

        let browser_opened = open_system_browser(&auth_url);
        Ok(GoogleDriveOAuthStartResult {
            auth_url,
            redirect_uri,
            browser_opened,
        })
    }

    pub async fn complete_oauth_manual(
        &self,
        db: &Database,
        raw_input: &str,
        app_handle: Option<&AppHandle>,
    ) -> AppResult<GoogleDriveSyncStatus> {
        let pending = self.pending_oauth.lock().await.take().ok_or_else(|| {
            AppError::InvalidInput(
                "No active OAuth session. Click 'Connect Google Drive' first.".into(),
            )
        })?;
        let _ = pending.cancel_tx.send(true);
        let code = parse_oauth_callback_input(raw_input, Some(&pending.state))?;

        finish_oauth_exchange(
            self,
            db,
            OAuthExchangeInput {
                code: &code,
                client_id: &pending.client_id,
                client_secret: &pending.client_secret,
                code_verifier: &pending.code_verifier,
                redirect_uri: &pending.redirect_uri,
            },
            app_handle,
        )
        .await
    }

    pub async fn cancel_oauth(&self) {
        if let Some(prev) = self.pending_oauth.lock().await.take() {
            let _ = prev.cancel_tx.send(true);
        }
    }

    pub async fn verify_configured_folder(&self, db: &Database) -> AppResult<GoogleDriveSyncStatus> {
        let mut cfg = db.load_gdrive_config()?;
        let folder_id = if !cfg.folder_id.trim().is_empty() {
            cfg.folder_id.clone()
        } else if !cfg.folder_input.trim().is_empty() {
            parse_google_drive_folder_id(&cfg.folder_input)?
        } else {
            return Err(AppError::InvalidInput(
                "Enter a Google Drive folder URL or folder ID first.".into(),
            ));
        };

        let folder = verify_drive_folder(&self.http, &mut cfg, db, &folder_id).await?;
        cfg.folder_id = folder.id;
        cfg.folder_name = Some(folder.name);
        db.save_gdrive_config(&cfg)?;
        Ok(Self::redact_status(&cfg))
    }

    pub async fn disconnect(&self, db: &Database) -> AppResult<GoogleDriveSyncStatus> {
        self.cancel_oauth().await;
        let mut cfg = db.load_gdrive_config()?;
        cfg.access_token = None;
        cfg.refresh_token = None;
        cfg.token_expires_at = None;
        cfg.connected_email = None;
        cfg.folder_name = None;
        cfg.enabled = false;
        db.save_gdrive_config(&cfg)?;
        Ok(Self::redact_status(&cfg))
    }

    pub async fn run_sync(
        &self,
        db: &Database,
        downloads: &DownloadManager,
        default_download_dir: &Path,
        app_handle: Option<&AppHandle>,
    ) -> AppResult<GoogleDriveSyncReport> {
        let _sync_guard = self.sync_lock.try_lock().map_err(|_| {
            AppError::InvalidInput("Google Drive sync is already in progress.".into())
        })?;

        let mut cfg = db.load_gdrive_config()?;
        if cfg.client_id.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "Configure your Google OAuth Client ID and connect Google Drive first.".into(),
            ));
        }

        let folder_id = if !cfg.folder_id.trim().is_empty() {
            cfg.folder_id.clone()
        } else {
            parse_google_drive_folder_id(&cfg.folder_input)?
        };
        if cfg.folder_id != folder_id {
            cfg.folder_id = folder_id.clone();
            db.save_gdrive_config(&cfg)?;
        }

        let folder_item = verify_drive_folder(&self.http, &mut cfg, db, &folder_id).await?;
        cfg.folder_name = Some(folder_item.name.clone());

        let root_files = list_drive_folder_files(&self.http, &mut cfg, db, &folder_id).await?;
        let remote_library_file = root_files
            .iter()
            .find(|f| f.name == LIBRARY_BUNDLE_FILENAME && f.mime_type != FOLDER_MIME)
            .cloned();
        let existing_books_folder = root_files
            .iter()
            .find(|f| f.name == BOOKS_SUBFOLDER_NAME && f.mime_type == FOLDER_MIME)
            .cloned();

        let mut report = GoogleDriveSyncReport {
            folder_id: folder_id.clone(),
            folder_name: Some(folder_item.name.clone()),
            synced_at: Utc::now().to_rfc3339(),
            ..Default::default()
        };

        // Step 1: Pull remote library bundle & two-way merge metadata, progress, bookmarks & highlights
        if let Some(ref lib_file) = remote_library_file {
            let bytes = download_drive_file(&self.http, &mut cfg, db, &lib_file.id).await?;
            let remote_bundle: LibraryExportBundle = serde_json::from_slice(&bytes).map_err(|e| {
                AppError::InvalidInput(format!("Invalid remote {LIBRARY_BUNDLE_FILENAME}: {e}"))
            })?;
            let (novels_n, entries_n, settings_applied) =
                db.merge_bundle(&remote_bundle, cfg.sync_settings)?;
            report.novels_merged = novels_n;
            report.library_entries_merged = entries_n;
            report.settings_applied = settings_applied;
        }

        let download_dir = downloads.current_download_dir();
        std::fs::create_dir_all(&download_dir)?;

        // Step 2 & 3: Two-way sync of downloaded book archives inside `<folder>/books/`
        if cfg.sync_downloaded_books {
            let books_folder_id = match existing_books_folder {
                Some(f) => f.id,
                None => {
                    create_drive_subfolder(&self.http, &mut cfg, db, &folder_id, BOOKS_SUBFOLDER_NAME)
                        .await?
                        .id
                }
            };

            let remote_book_files =
                list_drive_folder_files(&self.http, &mut cfg, db, &books_folder_id).await?;
            let mut remote_by_name: HashMap<String, DriveFileItem> = HashMap::new();
            let mut remote_by_novel_id: HashMap<String, DriveFileItem> = HashMap::new();
            for f in remote_book_files {
                if !f.name.ends_with(".novelium.zip") {
                    continue;
                }
                if let Some(nid) = f
                    .app_properties
                    .as_ref()
                    .and_then(|m| m.get("noveliumNovelId"))
                    .cloned()
                {
                    remote_by_novel_id.insert(nid, f.clone());
                }
                remote_by_name.insert(f.name.clone(), f);
            }

            // Step 2: Pull remote book archives that are missing or differ locally
            let local_novels = db.list_all_novels()?;
            let mut chapters_cache: HashMap<String, Vec<crate::models::Chapter>> = HashMap::new();
            for novel in &local_novels {
                chapters_cache.insert(novel.id.clone(), db.get_chapters(&novel.id)?);
            }

            for novel in &local_novels {
                let archive_name = novel_archive_filename(&novel.id);
                let remote_file = remote_by_novel_id
                    .get(&novel.id)
                    .or_else(|| remote_by_name.get(&archive_name));
                let Some(remote_item) = remote_file else {
                    continue;
                };
                let remote_digest = remote_item
                    .app_properties
                    .as_ref()
                    .and_then(|m| m.get("noveliumDigest"))
                    .map(|s| s.as_str());
                let chs = chapters_cache
                    .get(&novel.id)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                let local_digest = compute_novel_archive_digest(novel, chs, &download_dir);

                if local_digest.as_deref() == remote_digest && local_digest.is_some() {
                    continue;
                }

                let zip_bytes =
                    download_drive_file(&self.http, &mut cfg, db, &remote_item.id).await?;
                let unpacked = unpack_novel_archive(&zip_bytes, db, &download_dir)?;
                if unpacked.chapters_restored > 0 || unpacked.cover_restored {
                    report.books_downloaded += 1;
                    report.chapters_downloaded += unpacked.chapters_restored;
                }
            }

            // Step 3: Push local books whose packed digest differs from Google Drive
            let updated_novels = db.list_all_novels()?;
            for novel in &updated_novels {
                let chs = db.get_chapters(&novel.id)?;
                let Some((zip_bytes, local_digest, _packed_n)) =
                    pack_novel_archive(novel, &chs, &download_dir)?
                else {
                    continue;
                };

                let archive_name = novel_archive_filename(&novel.id);
                let existing = remote_by_novel_id
                    .get(&novel.id)
                    .or_else(|| remote_by_name.get(&archive_name));
                let remote_digest = existing.and_then(|f| {
                    f.app_properties
                        .as_ref()
                        .and_then(|m| m.get("noveliumDigest"))
                        .map(|s| s.as_str())
                });

                if remote_digest == Some(local_digest.as_str()) {
                    continue;
                }

                let props = serde_json::json!({
                    "noveliumNovelId": novel.id,
                    "noveliumDigest": local_digest,
                    "noveliumUpdatedAt": Utc::now().to_rfc3339(),
                });
                let desc = format!("Novelium offline book package: {}", novel.title);
                upload_or_update_drive_file(
                    &self.http,
                    &mut cfg,
                    db,
                    DriveUploadSpec {
                        existing_file_id: existing.map(|f| f.id.as_str()),
                        parent_id: &books_folder_id,
                        filename: &archive_name,
                        mime_type: "application/zip",
                        description: Some(&desc),
                        app_properties: Some(props),
                        content_bytes: &zip_bytes,
                    },
                )
                .await?;
                report.books_uploaded += 1;
            }
        }

        // Step 4: Upload merged library state JSON (books metadata, bookmarks, highlights, progress, shelves)
        let merged_bundle = db.export_bundle(&default_download_dir.to_string_lossy())?;
        let bundle_json = serde_json::to_vec_pretty(&merged_bundle)?;
        upload_or_update_drive_file(
            &self.http,
            &mut cfg,
            db,
            DriveUploadSpec {
                existing_file_id: remote_library_file.as_ref().map(|f| f.id.as_str()),
                parent_id: &folder_id,
                filename: LIBRARY_BUNDLE_FILENAME,
                mime_type: "application/json",
                description: Some(
                    "Novelium synced library, bookmarks, highlights, reading progress, and settings",
                ),
                app_properties: Some(serde_json::json!({
                    "noveliumType": "library_bundle",
                    "noveliumUpdatedAt": report.synced_at,
                })),
                content_bytes: &bundle_json,
            },
        )
        .await?;

        let summary = format!(
            "Merged {} book(s) · downloaded {} book(s) ({} ch) · uploaded {} book(s)",
            report.novels_merged,
            report.books_downloaded,
            report.chapters_downloaded,
            report.books_uploaded,
        );
        cfg.last_sync_at = Some(report.synced_at.clone());
        cfg.last_sync_summary = Some(summary);
        db.save_gdrive_config(&cfg)?;

        if let Some(app) = app_handle {
            let status = Self::redact_status(&cfg);
            let _ = app.emit("gdrive-sync-updated", &status);
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_and_auth_url_construction() {
        let verifier = generate_pkce_verifier();
        assert!(verifier.len() >= 43);
        let challenge = compute_pkce_challenge(&verifier);
        let url_str = build_google_auth_url(
            "123-abc.apps.googleusercontent.com",
            "http://127.0.0.1:49152/oauth2callback",
            &challenge,
            "state-xyz",
        )
        .unwrap();
        let parsed = Url::parse(&url_str).unwrap();
        let pairs: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("client_id").unwrap(), "123-abc.apps.googleusercontent.com");
        assert_eq!(pairs.get("redirect_uri").unwrap(), "http://127.0.0.1:49152/oauth2callback");
        assert_eq!(pairs.get("response_type").unwrap(), "code");
        assert_eq!(pairs.get("access_type").unwrap(), "offline");
        assert_eq!(pairs.get("prompt").unwrap(), "consent");
        assert_eq!(pairs.get("code_challenge_method").unwrap(), "S256");
        assert_eq!(pairs.get("code_challenge").unwrap(), &challenge);
        assert!(pairs.get("scope").unwrap().contains("auth/drive"));
        assert_eq!(pairs.get("state").unwrap(), "state-xyz");
    }

    #[test]
    fn test_parse_oauth_callback_input_handles_redirect_url_and_errors() {
        let ok_url = "http://127.0.0.1:51234/oauth2callback?code=4/0AX4XfWh&state=abc123&scope=https://www.googleapis.com/auth/drive";
        assert_eq!(
            parse_oauth_callback_input(ok_url, Some("abc123")).unwrap(),
            "4/0AX4XfWh"
        );
        assert!(parse_oauth_callback_input(ok_url, Some("wrong-state")).is_err());

        let missing_drive_scope = "http://127.0.0.1:51234/oauth2callback?code=4/0AX4XfWh&state=abc123&scope=email+openid";
        let scope_err = parse_oauth_callback_input(missing_drive_scope, Some("abc123"))
            .unwrap_err()
            .to_string();
        assert!(scope_err.contains("Google Drive permission was not checked"));

        let denied = "/oauth2callback?error=access_denied&error_description=User+denied+access";
        let err = parse_oauth_callback_input(denied, None).unwrap_err().to_string();
        assert!(err.contains("User denied access"));

        assert_eq!(
            parse_oauth_callback_input("raw-auth-code-value", Some("s")).unwrap(),
            "raw-auth-code-value"
        );
    }

    #[test]
    fn test_multipart_related_body_format() {
        let body = build_multipart_related_body(
            r#"{"name":"novelium-library.json"}"#,
            "application/json",
            br#"{"version":1}"#,
            "test_boundary",
        );
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("--test_boundary\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{\"name\":\"novelium-library.json\"}"));
        assert!(text.contains("\r\n--test_boundary\r\nContent-Type: application/json\r\n\r\n{\"version\":1}\r\n--test_boundary--\r\n"));
    }
}
