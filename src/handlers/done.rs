use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum_extra::extract::cookie::PrivateCookieJar;
use futures_util::TryStreamExt;
use mongodb::bson::{doc, Document};
use serde::Deserialize;

use crate::auth::{take_flash, AuthUser};
use crate::models::{DoneStats, FileCard, Flash};
use crate::query_filters::{parse_filters, parse_page_size, RawFilterInput};
use crate::state::AppState;
use crate::util::{
    commas, fmt_pct1, ist_date_end_epoch, ist_date_start_epoch, regex_escape, render, url_encode,
};

const DEFAULT_MAX_PAGES: i64 = 5;
const PAGE_WINDOW: i64 = 4;

#[derive(Deserialize, Default)]
pub struct DoneQuery {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub forward_from: String,
    #[serde(default)]
    pub reviewed_by: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub size_min: String,
    #[serde(default)]
    pub size_max: String,
    #[serde(default)]
    pub date_from: String,
    #[serde(default)]
    pub date_to: String,
    #[serde(default)]
    pub reviewed_date_from: String,
    #[serde(default)]
    pub reviewed_date_to: String,
    #[serde(default)]
    pub page: String,
    #[serde(default)]
    pub page_size: String,
}

pub struct PageLink {
    pub number: i64,
    pub href: String,
    pub active: bool,
}

#[derive(Template)]
#[template(path = "done.html")]
struct DoneTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,
    files: Vec<FileCard>,
    search_user_id: String,
    search_file_name: String,
    search_forward_from: String,
    search_reviewed_by: String,
    search_status: String,
    search_id: String,
    search_size_min: i64,
    search_size_max: i64,
    size_filter_active: bool,
    search_date_from: String,
    search_date_to: String,
    date_filter_active: bool,
    search_reviewed_date_from: String,
    search_reviewed_date_to: String,
    reviewed_date_filter_active: bool,
    page_heading: String,
    stats: Option<DoneStats>,
    total_pages: i64,
    pages: Vec<PageLink>,
    prev_href: Option<String>,
    next_href: Option<String>,
    page_size: i64,
    show_plgb_nav: bool,
    show_tgfs_nav: bool,
}

#[allow(clippy::too_many_arguments)]
fn build_done_url(
    page: i64,
    search_user_id: &str,
    search_file_name: &str,
    search_forward_from: &str,
    search_reviewed_by: &str,
    search_status: &str,
    size_min: i64,
    size_max: i64,
    search_id: &str,
    search_date_from: &str,
    search_date_to: &str,
    search_reviewed_date_from: &str,
    search_reviewed_date_to: &str,
    page_size: i64,
) -> String {
    let mut params: Vec<(String, String)> = vec![("page".into(), page.to_string())];
    if !search_user_id.is_empty() {
        params.push(("user_id".into(), search_user_id.to_string()));
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
    if !search_id.is_empty() {
        params.push(("id".into(), search_id.to_string()));
    }
    if !search_date_from.is_empty() {
        params.push(("date_from".into(), search_date_from.to_string()));
    }
    if !search_date_to.is_empty() {
        params.push(("date_to".into(), search_date_to.to_string()));
    }
    if !search_reviewed_date_from.is_empty() {
        params.push(("reviewed_date_from".into(), search_reviewed_date_from.to_string()));
    }
    if !search_reviewed_date_to.is_empty() {
        params.push(("reviewed_date_to".into(), search_reviewed_date_to.to_string()));
    }
    params.push(("page_size".into(), page_size.to_string()));
    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, url_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}/done-plgb?{}", crate::BASE_PATH, query)
}

pub async fn done(
    AuthUser(session): AuthUser,
    State(state): State<AppState>,
    Query(q): Query<DoneQuery>,
    jar: PrivateCookieJar,
) -> impl IntoResponse {
    let (jar, flash) = take_flash(jar);
    let mut flashes: Vec<Flash> = flash
        .map(|(category, message)| vec![Flash { category, message }])
        .unwrap_or_default();

    let parsed = parse_filters(RawFilterInput {
        user_id: &q.user_id,
        file_name: &q.file_name,
        forward_from: &q.forward_from,
        size_min: &q.size_min,
        size_max: &q.size_max,
        id: &q.id,
        date_from: &q.date_from,
        date_to: &q.date_to,
    });

    if parsed.invalid_user_id {
        flashes.push(Flash {
            category: "error".into(),
            message: "Invalid user ID. Please enter a valid number.".into(),
        });
    }
    if parsed.invalid_file_id {
        flashes.push(Flash {
            category: "error".into(),
            message: "Invalid File ID format. Please enter a valid 24-character hex ID.".into(),
        });
    }
    if parsed.invalid_date_range {
        flashes.push(Flash {
            category: "error".into(),
            message: "Invalid date range. Please use valid dates.".into(),
        });
    }

    // --- reviewed-at date range (IST calendar days, `reviewed_at` is an epoch-seconds
    // float on this collection, just like `time`) ---
    let mut search_reviewed_date_from = q.reviewed_date_from.trim().to_string();
    let mut search_reviewed_date_to = q.reviewed_date_to.trim().to_string();
    let mut invalid_reviewed_date_range = false;
    let mut reviewed_from_epoch = if search_reviewed_date_from.is_empty() {
        None
    } else {
        match ist_date_start_epoch(&search_reviewed_date_from) {
            Some(epoch) => Some(epoch),
            None => {
                invalid_reviewed_date_range = true;
                search_reviewed_date_from.clear();
                None
            }
        }
    };
    let mut reviewed_to_epoch = if search_reviewed_date_to.is_empty() {
        None
    } else {
        match ist_date_end_epoch(&search_reviewed_date_to) {
            Some(epoch) => Some(epoch),
            None => {
                invalid_reviewed_date_range = true;
                search_reviewed_date_to.clear();
                None
            }
        }
    };
    if let (Some(from), Some(to)) = (reviewed_from_epoch, reviewed_to_epoch) {
        if from > to {
            std::mem::swap(&mut reviewed_from_epoch, &mut reviewed_to_epoch);
            std::mem::swap(&mut search_reviewed_date_from, &mut search_reviewed_date_to);
        }
    }
    let reviewed_date_filter_active = reviewed_from_epoch.is_some() || reviewed_to_epoch.is_some();
    if invalid_reviewed_date_range {
        flashes.push(Flash {
            category: "error".into(),
            message: "Invalid reviewed-at date range. Please use valid dates.".into(),
        });
    }

    let search_reviewed_by = q.reviewed_by.trim().to_string();
    let search_status = q.status.trim().to_string();

    let has_extra_filter = !search_reviewed_by.is_empty()
        || !search_status.is_empty()
        || !parsed.search_user_id.is_empty()
        || !parsed.search_file_name.is_empty()
        || !parsed.search_forward_from.is_empty()
        || parsed.size_filter_active
        || parsed.date_filter_active
        || reviewed_date_filter_active
        || !parsed.search_id.is_empty();

    // Base condition: only reviewed files, plus whatever the shared parser found.
    let mut conditions = vec![doc! { "is_public": { "$exists": true } }];
    conditions.extend(parsed.conditions.clone());

    if !search_reviewed_by.is_empty() {
        conditions.push(doc! { "reviewed_by": { "$regex": regex_escape(&search_reviewed_by), "$options": "i" } });
    }
    if search_status == "accepted" {
        conditions.push(doc! { "is_public": true });
    } else if search_status == "rejected" {
        conditions.push(doc! { "is_public": false });
    }
    if reviewed_date_filter_active {
        let mut range = Document::new();
        if let Some(from) = reviewed_from_epoch {
            range.insert("$gte", from);
        }
        if let Some(to) = reviewed_to_epoch {
            range.insert("$lte", to);
        }
        conditions.push(doc! { "reviewed_at": range });
    }

    let page_size = parse_page_size(&q.page_size);

    let base_match = doc! { "$and": conditions.clone() };
    let total_docs = state.files.count_documents(base_match).await.unwrap_or(0) as i64;
    let real_total_pages = ((total_docs + page_size - 1) / page_size).max(1);
    // Unfiltered "most recent" browsing stays capped at DEFAULT_MAX_PAGES; an active
    // search needs the full result set reachable so it can actually be reviewed.
    let total_pages = if has_extra_filter {
        real_total_pages
    } else {
        DEFAULT_MAX_PAGES.min(real_total_pages)
    };

    let mut page: i64 = q.page.trim().parse().unwrap_or(1);
    page = page.clamp(1, total_pages);

    let pipeline = vec![
        doc! { "$match": { "$and": conditions.clone() } },
        doc! { "$sort": { "reviewed_at": -1 } },
        doc! { "$skip": (page - 1) * page_size },
        doc! { "$limit": page_size },
    ];

    let docs: Vec<Document> = match state.files.aggregate(pipeline).await {
        Ok(cursor) => cursor.try_collect().await.unwrap_or_default(),
        Err(err) => {
            tracing::error!("aggregate error: {err}");
            Vec::new()
        }
    };

    let stats = if has_extra_filter {
        let mut accepted_conditions = conditions.clone();
        accepted_conditions.push(doc! { "is_public": true });
        let accepted = state
            .files
            .count_documents(doc! { "$and": accepted_conditions })
            .await
            .unwrap_or(0) as i64;

        let mut rejected_conditions = conditions.clone();
        rejected_conditions.push(doc! { "is_public": false });
        let rejected = state
            .files
            .count_documents(doc! { "$and": rejected_conditions })
            .await
            .unwrap_or(0) as i64;

        Some(DoneStats {
            search_user_id: parsed
                .actual_user_ids
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            search_file_name: parsed.search_file_name.clone(),
            search_reviewed_by: search_reviewed_by.clone(),
            size_filter_active: parsed.size_filter_active,
            search_size_min: parsed.search_size_min,
            search_size_max: parsed.search_size_max,
            date_filter_active: parsed.date_filter_active,
            search_date_from: parsed.search_date_from.clone(),
            search_date_to: parsed.search_date_to.clone(),
            reviewed_date_filter_active,
            search_reviewed_date_from: search_reviewed_date_from.clone(),
            search_reviewed_date_to: search_reviewed_date_to.clone(),
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

    let files: Vec<FileCard> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| FileCard::from_doc(d, i + 1, &state.fqdn))
        .collect();

    if files.is_empty() {
        flashes.push(Flash {
            category: "info".into(),
            message: "No reviewed files found matching your search.".into(),
        });
    }

    let page_heading = if has_extra_filter {
        let mut heading = "\u{2705} Reviewed Files".to_string();
        if !parsed.search_user_id.is_empty() {
            heading.push_str(&format!(" \u{2013} User {}", parsed.search_user_id));
        }
        if !parsed.search_file_name.is_empty() {
            heading.push_str(&format!(" + \"{}\"", parsed.search_file_name));
        }
        heading
    } else {
        format!("\u{2705} Reviewed Files (most recent {page_size})")
    };

    // When a search is active, show a sliding window of PAGE_WINDOW page numbers
    // (plus Prev/Next arrows) instead of every page, since total_pages is no longer capped.
    let window_start = if has_extra_filter {
        ((page - 1) / PAGE_WINDOW) * PAGE_WINDOW + 1
    } else {
        1
    };
    let window_end = if has_extra_filter {
        (window_start + PAGE_WINDOW - 1).min(total_pages)
    } else {
        total_pages
    };

    let pages: Vec<PageLink> = (window_start..=window_end)
        .map(|p| PageLink {
            number: p,
            href: build_done_url(
                p,
                &parsed.search_user_id,
                &parsed.search_file_name,
                &parsed.search_forward_from,
                &search_reviewed_by,
                &search_status,
                parsed.search_size_min,
                parsed.search_size_max,
                &parsed.search_id,
                &parsed.search_date_from,
                &parsed.search_date_to,
                &search_reviewed_date_from,
                &search_reviewed_date_to,
                page_size,
            ),
            active: p == page,
        })
        .collect();

    let prev_href = (has_extra_filter && page > 1).then(|| {
        build_done_url(
            page - 1,
            &parsed.search_user_id,
            &parsed.search_file_name,
            &parsed.search_forward_from,
            &search_reviewed_by,
            &search_status,
            parsed.search_size_min,
            parsed.search_size_max,
            &parsed.search_id,
            &parsed.search_date_from,
            &parsed.search_date_to,
            &search_reviewed_date_from,
            &search_reviewed_date_to,
            page_size,
        )
    });
    let next_href = (has_extra_filter && page < total_pages).then(|| {
        build_done_url(
            page + 1,
            &parsed.search_user_id,
            &parsed.search_file_name,
            &parsed.search_forward_from,
            &search_reviewed_by,
            &search_status,
            parsed.search_size_min,
            parsed.search_size_max,
            &parsed.search_id,
            &parsed.search_date_from,
            &parsed.search_date_to,
            &search_reviewed_date_from,
            &search_reviewed_date_to,
            page_size,
        )
    });

    let tmpl = DoneTemplate {
        logged_in: true,
        username: session.username,
        flashes,
        files,
        search_user_id: parsed.search_user_id,
        search_file_name: parsed.search_file_name,
        search_forward_from: parsed.search_forward_from,
        search_reviewed_by,
        search_status,
        search_id: parsed.search_id,
        search_size_min: parsed.search_size_min,
        search_size_max: parsed.search_size_max,
        size_filter_active: parsed.size_filter_active,
        search_date_from: parsed.search_date_from,
        search_date_to: parsed.search_date_to,
        date_filter_active: parsed.date_filter_active,
        search_reviewed_date_from,
        search_reviewed_date_to,
        reviewed_date_filter_active,
        page_heading,
        stats,
        total_pages,
        pages,
        prev_href,
        next_href,
        page_size,
        show_plgb_nav: true,
        show_tgfs_nav: false,
    };

    (jar, render(tmpl))
}
