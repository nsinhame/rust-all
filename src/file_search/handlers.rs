use askama::Template;
use axum::extract::{Form, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, PrivateCookieJar};
use mongodb::bson::doc;
use serde::{Deserialize, Serialize};

use crate::file_search::models::{icon_for, FileDetail, ResultTile};
use crate::file_search::search::combined_search;
use crate::file_search::token::{decode_token, Source};
use crate::link_review::models::{get_bool, get_i64, FileCard};
use crate::link_review::tgfs_join;
use crate::link_review::tgfs_models::TgfsFileCard;
use crate::state::AppState;
use crate::util::{commas, render};

const PAGE_SIZE_MOBILE: i64 = 15;
const PAGE_SIZE_DESKTOP: i64 = 30;
const PAGE_WINDOW: i64 = 4;
/// Cookie holding the current search (query + resolved page size), kept out of the
/// URL entirely so it doesn't show up in the address bar/history. Encrypted via the
/// same `PrivateCookieJar`/`cookie_key` the review-system's session cookie uses.
const SEARCH_COOKIE: &str = "file_search_state";

pub struct PageLink {
    pub number: i64,
    pub href: String,
    pub active: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct SearchState {
    query: String,
    page_size: i64,
}

#[derive(Deserialize, Default)]
pub struct SubmitSearchForm {
    #[serde(default)]
    pub q: String,
    /// Set client-side (window width) so mobile gets fewer, desktop gets more per page.
    #[serde(default)]
    pub page_size: String,
}

#[derive(Deserialize, Default)]
pub struct SearchQuery {
    #[serde(default)]
    pub page: String,
}

#[derive(Template)]
#[template(path = "file_search/search.html")]
struct SearchTemplate {
    query: String,
    has_query: bool,
    tiles: Vec<ResultTile>,
    total_results_fmt: String,
    total_pages: i64,
    pages: Vec<PageLink>,
    prev_href: Option<String>,
    next_href: Option<String>,
    page_size: i64,
}

fn build_search_url(page: i64) -> String {
    format!("{}?page={}", crate::BASE_PATH_FILE_SEARCH, page)
}

/// Stores the submitted search (query + client-resolved page size) in an encrypted
/// cookie instead of a `?q=` URL param, then redirects to the plain results page —
/// keeps the search term out of the address bar, browser history, and any logs.
pub async fn submit_search(jar: PrivateCookieJar, Form(form): Form<SubmitSearchForm>) -> impl IntoResponse {
    let query = form.q.trim().to_string();
    let page_size = match form.page_size.trim().parse::<i64>() {
        Ok(PAGE_SIZE_DESKTOP) => PAGE_SIZE_DESKTOP,
        _ => PAGE_SIZE_MOBILE,
    };
    let value = serde_json::to_string(&SearchState { query, page_size }).unwrap_or_default();
    let jar = jar.add(Cookie::build((SEARCH_COOKIE, value)).path(crate::BASE_PATH_FILE_SEARCH).http_only(true));
    (jar, Redirect::to(crate::BASE_PATH_FILE_SEARCH))
}

/// Renders the single `/file-search` page: just the search bar when there's no saved
/// search, plus the result tiles/pagination once one has been submitted. The search
/// term itself comes from the `SEARCH_COOKIE`, not the URL — only the page number is.
pub async fn search(State(state): State<AppState>, jar: PrivateCookieJar, Query(q): Query<SearchQuery>) -> Response {
    let saved: SearchState = jar
        .get(SEARCH_COOKIE)
        .and_then(|c| serde_json::from_str(c.value()).ok())
        .unwrap_or_default();
    let query = saved.query;
    let has_query = !query.is_empty();
    let page_size = match saved.page_size {
        PAGE_SIZE_DESKTOP => PAGE_SIZE_DESKTOP,
        _ => PAGE_SIZE_MOBILE,
    };

    let all_results = if has_query {
        combined_search(&state, &query).await
    } else {
        Vec::new()
    };

    let total_results = all_results.len() as i64;
    let total_pages = if has_query {
        ((total_results + page_size - 1) / page_size).max(1)
    } else {
        1
    };
    let mut page: i64 = q.page.trim().parse().unwrap_or(1);
    page = page.clamp(1, total_pages);

    let start = ((page - 1) * page_size) as usize;
    let end = (start + page_size as usize).min(all_results.len());
    let tiles = if start < all_results.len() {
        all_results[start..end].to_vec()
    } else {
        Vec::new()
    };

    // Sliding window of PAGE_WINDOW page numbers (plus Prev/Next), same pattern as
    // the review-system's done pages use once results exceed one screen.
    let window_start = ((page - 1) / PAGE_WINDOW) * PAGE_WINDOW + 1;
    let window_end = (window_start + PAGE_WINDOW - 1).min(total_pages);
    let pages: Vec<PageLink> = (window_start..=window_end)
        .map(|p| PageLink {
            number: p,
            href: build_search_url(p),
            active: p == page,
        })
        .collect();
    let prev_href = (has_query && page > 1).then(|| build_search_url(page - 1));
    let next_href = (has_query && page < total_pages).then(|| build_search_url(page + 1));

    let tmpl = SearchTemplate {
        query,
        has_query,
        tiles,
        total_results_fmt: commas(total_results),
        total_pages,
        pages,
        prev_href,
        next_href,
        page_size,
    };

    render(tmpl)
}

/// Review-tool dl/watch links carry a `/a/` marker so the ad-serving load balancer
/// lets admins bypass ads. Public file-search visitors aren't admins, so strip it.
fn strip_admin_marker(url: String) -> String {
    url.replacen("/dl/a/", "/dl/", 1)
        .replacen("/watch/a/", "/watch/", 1)
        .replacen("/wt/a/", "/wt/", 1)
}

#[derive(Template)]
#[template(path = "file_search/detail.html")]
struct DetailTemplate {
    detail: FileDetail,
    back_href: String,
}

/// Looks up one file by its opaque `token` (see `file_search::token`) and renders its
/// detail/download page, or a bare 404 if it doesn't exist, isn't an already-accepted
/// /public file, or the token fails signature verification (e.g. tampered with).
pub async fn file_detail(Path(token): Path<String>, State(state): State<AppState>) -> Response {
    // Empty body, same as axum's default no-route-matched response — a visitor can't
    // tell this apart from a URL that was never registered at all.
    let not_found = || StatusCode::NOT_FOUND.into_response();

    let Some((source, oid)) = decode_token(state.access_key.as_bytes(), &token) else {
        return not_found();
    };

    let detail = match source {
        Source::Plgb => {
            let Ok(Some(doc)) = state.files.find_one(doc! { "_id": oid, "is_public": true }).await else {
                return not_found();
            };
            let card = FileCard::from_doc(&doc, 0, &state.fqdn);
            FileDetail {
                icon: icon_for(&card.mime_type, &card.file_name),
                file_name: card.file_name,
                file_size_fmt: card.file_size,
                mime_type: card.mime_type,
                dl_url: strip_admin_marker(card.dl_url),
                watch_url: strip_admin_marker(card.watch_url),
            }
        }
        Source::Tgfs { cluster_idx } => {
            let cluster_idx = cluster_idx as usize;
            let Some(index_coll) = state.tgfs.index_colls.get(cluster_idx) else {
                return not_found();
            };
            let Ok(Some(index_doc)) = index_coll.find_one(doc! { "_id": oid }).await else {
                return not_found();
            };
            let file_id = get_i64(&index_doc, "file_id").unwrap_or(0);
            let blob_cluster = tgfs_join::blob_cluster_of(&state, &index_doc);
            let Some(blob_coll) = state.tgfs.blob_colls.get(blob_cluster) else {
                return not_found();
            };
            let Ok(Some(blob_doc)) = blob_coll.find_one(doc! { "_id": file_id }).await else {
                return not_found();
            };
            if get_bool(&blob_doc, "is_restricted").unwrap_or(true) || !blob_doc.contains_key("reviewed_at") {
                return not_found();
            }
            let card = TgfsFileCard::from_docs(
                &index_doc,
                Some(&blob_doc),
                cluster_idx,
                0,
                &state.tgfs.link_secret,
                &state.tgfs.public_url,
            );
            FileDetail {
                icon: icon_for(&card.mime_type, &card.file_name),
                file_name: card.file_name,
                file_size_fmt: card.file_size,
                mime_type: card.mime_type,
                dl_url: strip_admin_marker(card.dl_url),
                watch_url: strip_admin_marker(card.watch_url),
            }
        }
    };

    render(DetailTemplate {
        detail,
        back_href: crate::BASE_PATH_FILE_SEARCH.to_string(),
    })
}
