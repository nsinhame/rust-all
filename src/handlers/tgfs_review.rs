use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use axum_extra::extract::cookie::PrivateCookieJar;
use mongodb::bson::{doc, DateTime as BsonDateTime, Document};
use serde::Deserialize;
use std::collections::HashMap;

use crate::auth::{take_flash, AuthUser};
use crate::models::{get_bool, get_i64, Flash};
use crate::query_filters::parse_page_size;
use crate::state::AppState;
use crate::tgfs_join;
use crate::tgfs_models::{decode_entry_id, TgfsFileCard, TgfsReviewStats};
use crate::tgfs_query_filters::{parse_tgfs_filters, TgfsRawFilterInput};
use crate::util::{commas, fmt_pct1, render};

#[derive(Deserialize, Default)]
pub struct TgfsReviewQuery {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub bot_id: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub forward_from: String,
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
    pub page_size: String,
}

#[derive(Template)]
#[template(path = "review-tgfs.html")]
struct TgfsReviewTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,
    files: Vec<TgfsFileCard>,
    search_user_id: String,
    search_bot_id: String,
    search_file_name: String,
    search_forward_from: String,
    search_file_id: String,
    search_size_min: i64,
    search_size_max: i64,
    size_filter_active: bool,
    search_date_from: String,
    search_date_to: String,
    date_filter_active: bool,
    has_any_filter: bool,
    page_heading: String,
    stats: Option<TgfsReviewStats>,
    page_size: i64,
    show_plgb_nav: bool,
    show_tgfs_nav: bool,
}

pub async fn review(
    AuthUser(session): AuthUser,
    State(state): State<AppState>,
    Query(q): Query<TgfsReviewQuery>,
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

    let page_size = parse_page_size(&q.page_size);
    let has_any_filter = !parsed.conditions.is_empty();
    let match_doc = if has_any_filter {
        doc! { "$and": parsed.conditions.clone() }
    } else {
        doc! {}
    };

    let sample = tgfs_join::sample_pending(&state, &match_doc, page_size).await;
    let wanted: Vec<(usize, i64)> = sample
        .iter()
        .filter_map(|(_, d)| get_i64(d, "file_id").map(|fid| (tgfs_join::blob_cluster_of(&state, d), fid)))
        .collect();
    let blob_map = tgfs_join::fetch_blob_map(&state, &wanted).await;

    let files: Vec<TgfsFileCard> = sample
        .iter()
        .enumerate()
        .map(|(i, (cluster, d))| {
            let fid = get_i64(d, "file_id").unwrap_or(0);
            TgfsFileCard::from_docs(
                d,
                blob_map.get(&fid),
                *cluster,
                i + 1,
                &state.tgfs.link_secret,
                &state.tgfs.public_url,
            )
        })
        .collect();

    let stats = if has_any_filter {
        let (candidates, cand_blob_map) = tgfs_join::candidates_with_blob(&state, &match_doc).await;
        let total = candidates.len() as i64;
        let mut reviewed = 0i64;
        let mut accepted = 0i64;
        let mut rejected = 0i64;
        for (_, d) in &candidates {
            let fid = get_i64(d, "file_id").unwrap_or(0);
            if let Some(b) = cand_blob_map.get(&fid) {
                if b.contains_key("reviewed_at") {
                    reviewed += 1;
                    if get_bool(b, "is_restricted").unwrap_or(false) {
                        rejected += 1;
                    } else {
                        accepted += 1;
                    }
                }
            }
        }
        let pending = total - reviewed;
        let sessions_left_fmt = if pending > 0 {
            format!("{}+", (pending as f64 / page_size as f64).round() as i64)
        } else {
            "0".to_string()
        };

        Some(TgfsReviewStats {
            search_user_id: parsed
                .actual_user_ids
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            search_bot_id: parsed.actual_bot_id.map(|v| v.to_string()).unwrap_or_default(),
            search_file_name: parsed.search_file_name.clone(),
            size_filter_active: parsed.size_filter_active,
            search_size_min: parsed.search_size_min,
            search_size_max: parsed.search_size_max,
            date_filter_active: parsed.date_filter_active,
            search_date_from: parsed.search_date_from.clone(),
            search_date_to: parsed.search_date_to.clone(),
            is_exclusion: parsed.is_exclusion,
            total_fmt: commas(total),
            reviewed_fmt: commas(reviewed),
            pending_fmt: commas(pending),
            accepted_fmt: commas(accepted),
            rejected_fmt: commas(rejected),
            progress_pct_fmt: fmt_pct1(reviewed, total),
            acceptance_rate_fmt: fmt_pct1(accepted, reviewed),
            rejection_rate_fmt: fmt_pct1(rejected, reviewed),
            sessions_left_fmt,
        })
    } else {
        None
    };

    // Human-readable description of whichever filters are active, shared between
    // the "no files found" flash and the page heading.
    let mut filter_parts: Vec<String> = Vec::new();
    if !parsed.search_user_id.is_empty() {
        filter_parts.push(if parsed.is_exclusion {
            format!(
                "excluding user {}",
                parsed
                    .actual_user_ids
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            format!("user {}", parsed.search_user_id)
        });
    }
    if !parsed.search_bot_id.is_empty() {
        filter_parts.push(if parsed.bot_is_exclusion {
            format!("excluding bot {}", parsed.actual_bot_id.unwrap_or(0))
        } else {
            format!("bot {}", parsed.search_bot_id)
        });
    }
    if !parsed.search_file_name.is_empty() {
        filter_parts.push(format!("matching \"{}\"", parsed.search_file_name));
    }

    if files.is_empty() {
        let message = if !filter_parts.is_empty() {
            format!("No pending files found for {}", filter_parts.join(", "))
        } else if has_any_filter {
            "No pending files found matching your filters".to_string()
        } else {
            "No files pending review!".to_string()
        };
        flashes.push(Flash {
            category: "info".into(),
            message,
        });
    }

    let page_heading = if !has_any_filter {
        format!("Review TGFS Files ({page_size} random entries)")
    } else if !filter_parts.is_empty() {
        format!(
            "Files: {}",
            filter_parts
                .iter()
                .map(|p| {
                    let mut c = p.chars();
                    match c.next() {
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" + ")
        )
    } else if parsed.size_filter_active {
        format!(
            "Review TGFS Files ({}\u{2013}{} MB)",
            parsed.search_size_min, parsed.search_size_max
        )
    } else {
        "Review TGFS Files (filtered)".to_string()
    };

    let tmpl = TgfsReviewTemplate {
        logged_in: true,
        username: session.username,
        flashes,
        files,
        search_user_id: parsed.search_user_id,
        search_bot_id: parsed.search_bot_id,
        search_file_name: parsed.search_file_name,
        search_forward_from: parsed.search_forward_from,
        search_file_id: parsed.search_file_id,
        search_size_min: parsed.search_size_min,
        search_size_max: parsed.search_size_max,
        size_filter_active: parsed.size_filter_active,
        search_date_from: parsed.search_date_from,
        search_date_to: parsed.search_date_to,
        date_filter_active: parsed.date_filter_active,
        has_any_filter,
        page_heading,
        stats,
        page_size,
        show_plgb_nav: false,
        show_tgfs_nav: true,
    };

    (jar, render(tmpl))
}

#[derive(Deserialize, Default)]
pub struct TgfsSubmitPayload {
    /// Keyed by `entry_id` (`"{index_cluster}:{objectid_hex}"`), see [`decode_entry_id`].
    #[serde(default)]
    pub decisions: HashMap<String, String>,
    #[serde(default)]
    pub file_names: HashMap<String, String>,
}

pub async fn submit(
    AuthUser(session): AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<TgfsSubmitPayload>,
) -> impl IntoResponse {
    if payload.decisions.is_empty() {
        return Json(serde_json::json!({
            "success": false,
            "message": "No decisions made"
        }));
    }

    let mut accepted_count = 0i64;
    let mut rejected_count = 0i64;
    let mut deleted_count = 0i64;
    let mut warned_count = 0i64;

    for (entry_id, decision) in payload.decisions.iter() {
        let Some((cluster_idx, oid)) = decode_entry_id(entry_id) else {
            continue;
        };
        let Some(index_coll) = state.tgfs.index_colls.get(cluster_idx) else {
            continue;
        };
        let index_doc = match index_coll.find_one(doc! { "_id": oid }).await {
            Ok(Some(d)) => d,
            _ => continue,
        };
        let user_id = get_i64(&index_doc, "user_id").unwrap_or(0);
        let file_id = get_i64(&index_doc, "file_id").unwrap_or(0);
        let blob_cluster = tgfs_join::blob_cluster_of(&state, &index_doc);
        let Some(blob_coll) = state.tgfs.blob_colls.get(blob_cluster) else {
            continue;
        };

        if decision == "delete" || decision == "delete_warn" {
            if index_coll.delete_one(doc! { "_id": oid }).await.is_ok() {
                deleted_count += 1;
            } else {
                continue;
            }

            // Garbage-collect the blob doc only if no index cluster references
            // this file_id anymore (dedup: the same physical file can be linked
            // by more than one user/bot).
            let mut still_referenced = false;
            for coll in &state.tgfs.index_colls {
                if coll
                    .count_documents(doc! { "file_id": file_id })
                    .await
                    .unwrap_or(0)
                    > 0
                {
                    still_referenced = true;
                    break;
                }
            }
            if !still_referenced {
                if let Err(err) = blob_coll.delete_one(doc! { "_id": file_id }).await {
                    tracing::error!("error deleting tgfs blob doc {file_id}: {err}");
                }
            }

            if decision == "delete_warn" {
                let warn_result = state
                    .tgfs
                    .primary_users
                    .update_one(doc! { "_id": user_id }, doc! { "$inc": { "warns": 1 } })
                    .upsert(true)
                    .await;
                match warn_result {
                    Ok(_) => warned_count += 1,
                    Err(err) => tracing::error!(
                        "error incrementing warns for user {user_id} (entry {entry_id}): {err}"
                    ),
                }
            }
            continue;
        }

        if decision != "accept" && decision != "reject" {
            continue;
        }
        let is_restricted = decision == "reject";

        let mut blob_set = doc! {
            "is_restricted": is_restricted,
            "reviewed_by": &session.username,
            "reviewed_at": BsonDateTime::now(),
        };
        let mut index_set = Document::new();
        if let Some(name) = payload.file_names.get(entry_id) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                blob_set.insert("file_name", trimmed);
                index_set.insert("file_name", trimmed);
            }
        }

        let result = blob_coll
            .update_one(doc! { "_id": file_id }, doc! { "$set": blob_set })
            .await;
        if !index_set.is_empty() {
            let _ = index_coll
                .update_one(doc! { "_id": oid }, doc! { "$set": index_set })
                .await;
        }
        match result {
            Ok(_) => {
                if is_restricted {
                    rejected_count += 1;
                } else {
                    accepted_count += 1;
                }
            }
            Err(err) => tracing::error!("error updating tgfs file {file_id}: {err}"),
        }
    }

    let mut suffix = String::new();
    if warned_count > 0 {
        suffix.push_str(&format!(
            " ({warned_count} user{} warned)",
            if warned_count == 1 { "" } else { "s" }
        ));
    }

    Json(serde_json::json!({
        "success": true,
        "accepted": accepted_count,
        "rejected": rejected_count,
        "deleted": deleted_count,
        "warned": warned_count,
        "message": format!(
            "Processed {} files successfully!{}",
            accepted_count + rejected_count + deleted_count,
            suffix
        ),
    }))
}
