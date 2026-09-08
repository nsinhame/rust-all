use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum_extra::extract::cookie::PrivateCookieJar;
use mongodb::bson::{doc, Bson, Document};
use serde::Deserialize;

use crate::auth::{take_flash, AuthUser};
use crate::handlers::done::PageLink;
use crate::models::{get_bool, get_i64, get_str, Flash};
use crate::query_filters::parse_page_size;
use crate::state::AppState;
use crate::tgfs_join;
use crate::tgfs_models::{TgfsDoneStats, TgfsFileCard};
use crate::tgfs_query_filters::{parse_tgfs_filters, TgfsRawFilterInput};
use crate::util::{commas, fmt_pct1, render, url_encode};

const MAX_PAGES: i64 = 5;

#[derive(Deserialize, Default)]
pub struct TgfsDoneQuery {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub bot_id: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub forward_from: String,
    #[serde(default)]
    pub reviewed_by: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub size_min: String,
    #[serde(default)]
    pub size_max: String,
    #[serde(default)]
    pub date_from: String,
    #[serde(default)]
    pub date_to: String,
    #[serde(default)]
    pub page: String,
    #[serde(default)]
    pub page_size: String,
}

#[derive(Template)]
#[template(path = "done-tgfs.html")]
struct TgfsDoneTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,
    files: Vec<TgfsFileCard>,
    search_user_id: String,
    search_bot_id: String,
    search_file_name: String,
    search_forward_from: String,
    search_reviewed_by: String,
    search_status: String,
    search_file_id: String,
    search_size_min: i64,
    search_size_max: i64,
    size_filter_active: bool,
    search_date_from: String,
    search_date_to: String,
    date_filter_active: bool,
    page_heading: String,
    stats: Option<TgfsDoneStats>,
    total_pages: i64,
    pages: Vec<PageLink>,
    page_size: i64,
    show_plgb_nav: bool,
    show_tgfs_nav: bool,
}

#[allow(clippy::too_many_arguments)]
fn build_tgfs_done_url(
    page: i64,
    search_user_id: &str,
    search_bot_id: &str,
    search_file_name: &str,
    search_forward_from: &str,
    search_reviewed_by: &str,
    search_status: &str,
    size_min: i64,
    size_max: i64,
    search_file_id: &str,
    search_date_from: &str,
    search_date_to: &str,
    page_size: i64,
) -> String {
    let mut params: Vec<(String, String)> = vec![("page".into(), page.to_string())];
    if !search_user_id.is_empty() {
        params.push(("user_id".into(), search_user_id.to_string()));
    }
    if !search_bot_id.is_empty() {
        params.push(("bot_id".into(), search_bot_id.to_string()));
    }
    if !search_file_name.is_empty() {
        params.push(("file_name".into(), search_file_name.to_string()));
    }
    if !search_forward_from.is_empty() {
        params.push(("forward_from".into(), search_forward_from.to_string()));
    }
    if !search_reviewed_by.is_empty() {
        params.push(("reviewed_by".into(), search_reviewed_by.to_string()));
    }
    if !search_status.is_empty() {
        params.push(("status".into(), search_status.to_string()));
    }
    params.push(("size_min".into(), size_min.to_string()));
    params.push(("size_max".into(), size_max.to_string()));
    if !search_file_id.is_empty() {
        params.push(("file_id".into(), search_file_id.to_string()));
    }
    if !search_date_from.is_empty() {
        params.push(("date_from".into(), search_date_from.to_string()));
    }
    if !search_date_to.is_empty() {
        params.push(("date_to".into(), search_date_to.to_string()));
    }
    params.push(("page_size".into(), page_size.to_string()));
    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, url_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}/done-tgfs?{}", crate::BASE_PATH, query)
}

pub async fn done(
    AuthUser(session): AuthUser,
    State(state): State<AppState>,
    Query(q): Query<TgfsDoneQuery>,
    jar: PrivateCookieJar,
) -> impl IntoResponse {
    let (jar, flash) = take_flash(jar);
    let mut flashes: Vec<Flash> = flash
        .map(|(category, message)| vec![Flash { category, message }])
        .unwrap_or_default();

    let parsed = parse_tgfs_filters(TgfsRawFilterInput {
        user_id: &q.user_id,
        bot_id: &q.bot_id,
        file_name: &q.file_name,
        forward_from: &q.forward_from,
        size_min: &q.size_min,
        size_max: &q.size_max,
        file_id: &q.file_id,
        date_from: &q.date_from,
        date_to: &q.date_to,
    });

    if parsed.invalid_user_id {
        flashes.push(Flash {
            category: "error".into(),
            message: "Invalid user ID. Please enter a valid number.".into(),
        });
    }
    if parsed.invalid_bot_id {
        flashes.push(Flash {
            category: "error".into(),
            message: "Invalid bot ID. Please enter a valid number.".into(),
        });
    }
    if parsed.invalid_file_id {
        flashes.push(Flash {
            category: "error".into(),
            message: "Invalid file ID. Please enter a valid number.".into(),
        });
    }
    if parsed.invalid_date_range {
        flashes.push(Flash {
            category: "error".into(),
            message: "Invalid date range. Please use valid dates.".into(),
        });
    }

    let search_reviewed_by = q.reviewed_by.trim().to_string();
    let search_status = q.status.trim().to_string();
    let search_reviewed_by_lower = search_reviewed_by.to_lowercase();

    let match_doc = if parsed.conditions.is_empty() {
        doc! {}
    } else {
        doc! { "$and": parsed.conditions.clone() }
    };

    // telethon-plgb's review status (`reviewed_at`/`is_restricted`) lives on the
    // blob doc, a different DB deployment than `user_files` — so reviewed-only
    // filtering, status, and reviewed_by all have to be applied in-process after
    // joining, rather than pushed into the index cluster's `$match`.
    let (candidates, blob_map) = tgfs_join::candidates_with_blob(&state, &match_doc).await;
    let mut reviewed: Vec<(usize, Document, Document)> = Vec::new();
    for (cluster, index_doc) in candidates {
        let file_id = get_i64(&index_doc, "file_id").unwrap_or(0);
        let Some(blob_doc) = blob_map.get(&file_id) else {
            continue;
        };
        if !blob_doc.contains_key("reviewed_at") {
            continue;
        }
        let is_restricted = get_bool(blob_doc, "is_restricted").unwrap_or(false);
        if search_status == "accepted" && is_restricted {
            continue;
        }
        if search_status == "rejected" && !is_restricted {
            continue;
        }
        if !search_reviewed_by.is_empty() {
            let rb = get_str(blob_doc, "reviewed_by").unwrap_or_default().to_lowercase();
            if !rb.contains(&search_reviewed_by_lower) {
                continue;
            }
        }
        reviewed.push((cluster, index_doc, blob_doc.clone()));
    }

    // Most-recently-reviewed first.
    reviewed.sort_by(|a, b| {
        let ts = |d: &Document| match d.get("reviewed_at") {
            Some(Bson::DateTime(dt)) => dt.timestamp_millis(),
            _ => 0,
        };
        ts(&b.2).cmp(&ts(&a.2))
    });

    let total_docs = reviewed.len() as i64;
    let page_size = parse_page_size(&q.page_size);
    let mut page: i64 = q.page.trim().parse().unwrap_or(1);
    page = page.clamp(1, MAX_PAGES);
    let total_pages = MAX_PAGES.min(((total_docs + page_size - 1) / page_size).max(1));

    let start = ((page - 1) * page_size) as usize;
    let end = (start + page_size as usize).min(reviewed.len());
    let page_slice = if start < reviewed.len() {
        &reviewed[start..end]
    } else {
        &[]
    };

    let files: Vec<TgfsFileCard> = page_slice
        .iter()
        .enumerate()
        .map(|(i, (cluster, index_doc, blob_doc))| {
            TgfsFileCard::from_docs(
                index_doc,
                Some(blob_doc),
                *cluster,
                start + i + 1,
                &state.tgfs.link_secret,
                &state.tgfs.public_url,
            )
        })
        .collect();

    let has_extra_filter = !search_reviewed_by.is_empty()
        || !search_status.is_empty()
        || !parsed.search_user_id.is_empty()
        || !parsed.search_bot_id.is_empty()
        || !parsed.search_file_name.is_empty()
        || !parsed.search_forward_from.is_empty()
        || parsed.size_filter_active
        || parsed.date_filter_active
        || !parsed.search_file_id.is_empty();

    let stats = if has_extra_filter {
        let accepted = reviewed
            .iter()
            .filter(|(_, _, b)| !get_bool(b, "is_restricted").unwrap_or(false))
            .count() as i64;
        let rejected = total_docs - accepted;
        Some(TgfsDoneStats {
            search_user_id: parsed
                .actual_user_ids
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            search_bot_id: parsed.actual_bot_id.map(|v| v.to_string()).unwrap_or_default(),
            search_file_name: parsed.search_file_name.clone(),
            search_reviewed_by: search_reviewed_by.clone(),
            size_filter_active: parsed.size_filter_active,
            search_size_min: parsed.search_size_min,
            search_size_max: parsed.search_size_max,
            date_filter_active: parsed.date_filter_active,
            search_date_from: parsed.search_date_from.clone(),
            search_date_to: parsed.search_date_to.clone(),
            is_exclusion: parsed.is_exclusion,
            total_fmt: commas(total_docs),
            accepted_fmt: commas(accepted),
            rejected_fmt: commas(rejected),
            acceptance_rate_fmt: fmt_pct1(accepted, total_docs),
            rejection_rate_fmt: fmt_pct1(rejected, total_docs),
        })
    } else {
        None
    };

    if files.is_empty() {
        flashes.push(Flash {
            category: "info".into(),
            message: "No reviewed files found matching your search.".into(),
        });
    }

    let page_heading = if has_extra_filter {
        let mut heading = "\u{2705} Reviewed TGFS Files".to_string();
        if !parsed.search_user_id.is_empty() {
            heading.push_str(&format!(" \u{2013} User {}", parsed.search_user_id));
        }
        if !parsed.search_bot_id.is_empty() {
            heading.push_str(&format!(" \u{2013} Bot {}", parsed.search_bot_id));
        }
        if !parsed.search_file_name.is_empty() {
            heading.push_str(&format!(" + \"{}\"", parsed.search_file_name));
        }
        heading
    } else {
        format!("\u{2705} Reviewed TGFS Files (most recent {page_size})")
    };

    let pages: Vec<PageLink> = (1..=total_pages)
        .map(|p| PageLink {
            number: p,
            href: build_tgfs_done_url(
                p,
                &parsed.search_user_id,
                &parsed.search_bot_id,
                &parsed.search_file_name,
                &parsed.search_forward_from,
                &search_reviewed_by,
                &search_status,
                parsed.search_size_min,
                parsed.search_size_max,
                &parsed.search_file_id,
                &parsed.search_date_from,
                &parsed.search_date_to,
                page_size,
            ),
            active: p == page,
        })
        .collect();

    let tmpl = TgfsDoneTemplate {
        logged_in: true,
        username: session.username,
        flashes,
        files,
        search_user_id: parsed.search_user_id,
        search_bot_id: parsed.search_bot_id,
        search_file_name: parsed.search_file_name,
        search_forward_from: parsed.search_forward_from,
        search_reviewed_by,
        search_status,
        search_file_id: parsed.search_file_id,
        search_size_min: parsed.search_size_min,
        search_size_max: parsed.search_size_max,
        size_filter_active: parsed.size_filter_active,
        search_date_from: parsed.search_date_from,
        search_date_to: parsed.search_date_to,
        date_filter_active: parsed.date_filter_active,
        page_heading,
        stats,
        total_pages,
        pages,
        page_size,
        show_plgb_nav: false,
        show_tgfs_nav: true,
    };

    (jar, render(tmpl))
}
