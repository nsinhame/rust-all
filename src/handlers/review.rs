use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use axum_extra::extract::cookie::PrivateCookieJar;
use futures_util::TryStreamExt;
use mongodb::bson::{doc, oid::ObjectId, Document};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::auth::{take_flash, AuthUser};
use crate::models::{Flash, FileCard, ReviewStats};
use crate::query_filters::{parse_filters, RawFilterInput};
use crate::state::AppState;
use crate::util::{commas, fmt_pct1, render};

#[derive(Deserialize, Default)]
pub struct ReviewQuery {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub forward_from: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub size_min: String,
    #[serde(default)]
    pub size_max: String,
}

#[derive(Template)]
#[template(path = "review.html")]
struct ReviewTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,
    files: Vec<FileCard>,
    search_user_id: String,
    search_file_name: String,
    search_forward_from: String,
    search_id: String,
    search_size_min: i64,
    search_size_max: i64,
    size_filter_active: bool,
    has_any_filter: bool,
    page_heading: String,
    stats: Option<ReviewStats>,
}

/// Combines a set of pre-parsed search conditions with one extra condition,
/// producing a `{"$and": [...]}` document for a `count_documents` call.
fn and_with(conditions: &[Document], extra: Document) -> Document {
    let mut all = conditions.to_vec();
    all.push(extra);
    doc! { "$and": all }
}

pub async fn review(
    AuthUser(session): AuthUser,
    State(state): State<AppState>,
    Query(q): Query<ReviewQuery>,
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

    let has_any_filter = !parsed.conditions.is_empty();

    let (pipeline, stats) = if has_any_filter {
        let mut match_conditions = parsed.conditions.clone();
        match_conditions.push(doc! { "is_public": { "$exists": false } });
        let pipeline = vec![
            doc! { "$match": { "$and": match_conditions } },
            doc! { "$sample": { "size": 10 } },
        ];

        let base_match = doc! { "$and": parsed.conditions.clone() };
        let total = state
            .files
            .count_documents(base_match)
            .await
            .unwrap_or(0) as i64;
        let reviewed = state
            .files
            .count_documents(and_with(&parsed.conditions, doc! { "is_public": { "$exists": true } }))
            .await
            .unwrap_or(0) as i64;
        let pending = state
            .files
            .count_documents(and_with(&parsed.conditions, doc! { "is_public": { "$exists": false } }))
            .await
            .unwrap_or(0) as i64;
        let accepted = state
            .files
            .count_documents(and_with(&parsed.conditions, doc! { "is_public": true }))
            .await
            .unwrap_or(0) as i64;
        let rejected = state
            .files
            .count_documents(and_with(&parsed.conditions, doc! { "is_public": false }))
            .await
            .unwrap_or(0) as i64;

        let excluded_count = if parsed.is_exclusion {
            if let Some(uid) = parsed.actual_user_id {
                let count = state
                    .files
                    .count_documents(and_with(
                        &[doc! { "user_id": uid }],
                        doc! { "is_public": { "$exists": false } },
                    ))
                    .await
                    .unwrap_or(0) as i64;
                Some(count)
            } else {
                None
            }
        } else {
            None
        };

        let sessions_left_fmt = if pending > 0 {
            format!("{}+", (pending as f64 / 10.0).round() as i64)
        } else {
            "0".to_string()
        };

        let stats = ReviewStats {
            search_user_id: parsed.actual_user_id.map(|v| v.to_string()).unwrap_or_default(),
            search_file_name: parsed.search_file_name.clone(),
            size_filter_active: parsed.size_filter_active,
            search_size_min: parsed.search_size_min,
            search_size_max: parsed.search_size_max,
            is_exclusion: parsed.is_exclusion,
            total_fmt: commas(total),
            reviewed_fmt: commas(reviewed),
            pending_fmt: commas(pending),
            accepted_fmt: commas(accepted),
            rejected_fmt: commas(rejected),
            excluded_count_fmt: excluded_count.map(commas).unwrap_or_default(),
            has_excluded_count: excluded_count.is_some(),
            progress_pct_fmt: fmt_pct1(reviewed, total),
            acceptance_rate_fmt: fmt_pct1(accepted, reviewed),
            rejection_rate_fmt: fmt_pct1(rejected, reviewed),
            sessions_left_fmt,
        };

        (pipeline, Some(stats))
    } else {
        let pipeline = vec![
            doc! { "$match": { "is_public": { "$exists": false } } },
            doc! { "$sample": { "size": 10 } },
        ];
        (pipeline, None)
    };

    let docs: Vec<Document> = match state.files.aggregate(pipeline).await {
        Ok(cursor) => cursor.try_collect().await.unwrap_or_default(),
        Err(err) => {
            tracing::error!("aggregate error: {err}");
            Vec::new()
        }
    };

    let files: Vec<FileCard> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| FileCard::from_doc(d, i + 1))
        .collect();

    if files.is_empty() {
        let message = if !parsed.search_user_id.is_empty() && !parsed.search_file_name.is_empty() {
            if parsed.is_exclusion {
                format!(
                    "No pending files found matching \"{}\" (excluding user {})",
                    parsed.search_file_name,
                    parsed.actual_user_id.unwrap_or(0)
                )
            } else {
                format!(
                    "No pending files found for user {} matching \"{}\"",
                    parsed.search_user_id, parsed.search_file_name
                )
            }
        } else if !parsed.search_user_id.is_empty() {
            if parsed.is_exclusion {
                format!(
                    "No pending files found (excluding user ID: {})",
                    parsed.actual_user_id.unwrap_or(0)
                )
            } else {
                format!("No pending files found for user ID: {}", parsed.search_user_id)
            }
        } else if !parsed.search_file_name.is_empty() {
            format!("No pending files found matching: {}", parsed.search_file_name)
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
        "Review Files (10 random entries)".to_string()
    } else if !parsed.search_user_id.is_empty() && !parsed.search_file_name.is_empty() {
        if parsed.is_exclusion {
            format!(
                "Files: \"{}\" (Excluding User {})",
                parsed.search_file_name,
                parsed.actual_user_id.unwrap_or(0)
            )
        } else {
            format!("Files: User {} + \"{}\"", parsed.search_user_id, parsed.search_file_name)
        }
    } else if !parsed.search_user_id.is_empty() {
        if parsed.is_exclusion {
            format!("Review Files (Excluding User {})", parsed.actual_user_id.unwrap_or(0))
        } else {
            format!("Review Files for User {}", parsed.search_user_id)
        }
    } else if !parsed.search_file_name.is_empty() {
        format!("Review Files matching \"{}\"", parsed.search_file_name)
    } else if parsed.size_filter_active {
        format!(
            "Review Files ({}\u{2013}{} MB)",
            parsed.search_size_min, parsed.search_size_max
        )
    } else {
        "Review Files (filtered)".to_string()
    };

    let tmpl = ReviewTemplate {
        logged_in: true,
        username: session.username,
        flashes,
        files,
        search_user_id: parsed.search_user_id,
        search_file_name: parsed.search_file_name,
        search_forward_from: parsed.search_forward_from,
        search_id: parsed.search_id,
        search_size_min: parsed.search_size_min,
        search_size_max: parsed.search_size_max,
        size_filter_active: parsed.size_filter_active,
        has_any_filter,
        page_heading,
        stats,
    };

    (jar, render(tmpl))
}

#[derive(Deserialize, Default)]
pub struct SubmitPayload {
    #[serde(default)]
    pub decisions: HashMap<String, String>,
    #[serde(default)]
    pub file_names: HashMap<String, String>,
    #[serde(default)]
    pub special_hash: HashMap<String, bool>,
}

fn now_unix_secs_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub async fn submit(
    AuthUser(session): AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<SubmitPayload>,
) -> impl IntoResponse {
    if payload.decisions.is_empty() {
        return Json(serde_json::json!({
            "success": false,
            "message": "No decisions made"
        }));
    }

    let mut accepted_count = 0i64;
    let mut rejected_count = 0i64;

    for (file_id, decision) in payload.decisions.iter() {
        let oid = match ObjectId::parse_str(file_id) {
            Ok(o) => o,
            Err(_) => continue,
        };
        let is_public = decision == "accept";

        let mut set_doc = doc! {
            "is_public": is_public,
            "reviewed_by": &session.username,
            "reviewed_at": now_unix_secs_f64(),
        };

        if let Some(name) = payload.file_names.get(file_id) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                set_doc.insert("file_name", trimmed);
            }
        }

        if payload.special_hash.get(file_id).copied().unwrap_or(false) {
            set_doc.insert("special_type", "infinite_ads_loop");
        }

        let result = state
            .files
            .update_one(doc! { "_id": oid }, doc! { "$set": set_doc })
            .await;

        match result {
            Ok(_) => {
                if is_public {
                    accepted_count += 1;
                } else {
                    rejected_count += 1;
                }
            }
            Err(err) => tracing::error!("error updating file {file_id}: {err}"),
        }
    }

    Json(serde_json::json!({
        "success": true,
        "accepted": accepted_count,
        "rejected": rejected_count,
        "message": format!("Updated {} files successfully!", accepted_count + rejected_count),
    }))
}
