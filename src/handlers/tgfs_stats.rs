use askama::Template;
use axum::extract::State;
use axum::response::IntoResponse;
use axum_extra::extract::cookie::PrivateCookieJar;
use futures_util::TryStreamExt;
use mongodb::bson::{doc, Document};
use mongodb::Collection;
use std::collections::HashMap;

use crate::auth::{take_flash, AuthUser};
use crate::models::{Flash, ReviewerStats};
use crate::state::AppState;
use crate::tgfs_models::TgfsBotBreakdown;
use crate::util::{commas, fmt_pct1, pct, render};

#[derive(Template)]
#[template(path = "stats-tgfs.html")]
struct TgfsStatsTemplate {
    logged_in: bool,
    username: String,
    flashes: Vec<Flash>,

    total_fmt: String,
    pending: i64,
    pending_fmt: String,
    public_fmt: String,
    private_fmt: String,
    reviewed_fmt: String,
    progress_pct: f64,
    progress_pct_fmt: String,

    bar_accept_pct: f64,
    bar_reject_pct: f64,
    bar_pending_pct: f64,

    total_reviewed_fmt: String,
    accept_angle: f64,
    has_reviewed: bool,
    pie_accept_pct_fmt: String,
    pie_reject_pct_fmt: String,

    acceptance_rate_fmt: String,
    rejection_rate_fmt: String,
    ratio_str: String,
    sessions_left_fmt: String,

    nik: ReviewerStats,
    prdp: ReviewerStats,

    nik_reviews_bar_pct: f64,
    prdp_reviews_bar_pct: f64,
    nik_accept_rate: f64,
    prdp_accept_rate: f64,

    bots: Vec<TgfsBotBreakdown>,

    show_plgb_nav: bool,
    show_tgfs_nav: bool,
}

async fn count_across(colls: &[Collection<Document>], filter: Document) -> i64 {
    let mut total = 0i64;
    for c in colls {
        total += c.count_documents(filter.clone()).await.unwrap_or(0) as i64;
    }
    total
}

async fn reviewer_stats(blob_colls: &[Collection<Document>], name: &str, total_reviewed_all: i64) -> ReviewerStats {
    let total_reviewed = count_across(blob_colls, doc! { "reviewed_by": name }).await;
    let accepted = count_across(
        blob_colls,
        doc! { "reviewed_by": name, "is_restricted": false },
    )
    .await;
    let rejected = count_across(
        blob_colls,
        doc! { "reviewed_by": name, "is_restricted": true },
    )
    .await;

    ReviewerStats {
        total_reviewed_fmt: commas(total_reviewed),
        accepted_fmt: commas(accepted),
        rejected_fmt: commas(rejected),
        acceptance_rate_fmt: fmt_pct1(accepted, total_reviewed),
        contribution_fmt: fmt_pct1(total_reviewed, total_reviewed_all),
    }
}

/// Total `user_files` ("links generated") per bot, across every index cluster.
/// Index-level only (no blob join needed), so it's cheap even at scale.
async fn bot_breakdown(index_colls: &[Collection<Document>]) -> Vec<TgfsBotBreakdown> {
    let mut counts: HashMap<i64, i64> = HashMap::new();
    for coll in index_colls {
        let pipeline = vec![doc! { "$group": { "_id": "$bot_id", "count": { "$sum": 1 } } }];
        let cursor = match coll.aggregate(pipeline).await {
            Ok(c) => c,
            Err(err) => {
                tracing::error!("tgfs bot breakdown aggregate error: {err}");
                continue;
            }
        };
        let docs: Vec<Document> = cursor.try_collect().await.unwrap_or_default();
        for doc in docs {
            if let Some(bot_id) = crate::models::get_i64(&doc, "_id") {
                let count = crate::models::get_i64(&doc, "count").unwrap_or(0);
                *counts.entry(bot_id).or_insert(0) += count;
            }
        }
    }
    let mut rows: Vec<(i64, i64)> = counts.into_iter().collect();
    rows.sort_by_key(|(bot_id, _)| *bot_id);
    rows.into_iter()
        .map(|(bot_id, count)| TgfsBotBreakdown {
            bot_id: bot_id.to_string(),
            count_fmt: commas(count),
        })
        .collect()
}

pub async fn stats(
    AuthUser(session): AuthUser,
    State(state): State<AppState>,
    jar: PrivateCookieJar,
) -> impl IntoResponse {
    let (jar, flash) = take_flash(jar);
    let flashes: Vec<Flash> = flash
        .map(|(category, message)| vec![Flash { category, message }])
        .unwrap_or_default();

    let blob_colls = &state.tgfs.blob_colls;
    let index_colls = &state.tgfs.index_colls;

    let total = count_across(blob_colls, doc! {}).await;
    let reviewed = count_across(blob_colls, doc! { "reviewed_at": { "$exists": true } }).await;
    let pending = total - reviewed;
    let public = count_across(
        blob_colls,
        doc! { "reviewed_at": { "$exists": true }, "is_restricted": false },
    )
    .await;
    let private = count_across(
        blob_colls,
        doc! { "reviewed_at": { "$exists": true }, "is_restricted": true },
    )
    .await;

    let nik = reviewer_stats(blob_colls, "nik", reviewed).await;
    let prdp = reviewer_stats(blob_colls, "prdp", reviewed).await;
    let nik_total = count_across(blob_colls, doc! { "reviewed_by": "nik" }).await;
    let prdp_total = count_across(blob_colls, doc! { "reviewed_by": "prdp" }).await;
    let nik_accepted = count_across(blob_colls, doc! { "reviewed_by": "nik", "is_restricted": false }).await;
    let prdp_accepted = count_across(blob_colls, doc! { "reviewed_by": "prdp", "is_restricted": false }).await;

    let progress_pct = pct(reviewed, total);
    let max_val = public.max(private).max(pending).max(1);
    let bar_accept_pct = pct(public, max_val);
    let bar_reject_pct = pct(private, max_val);
    let bar_pending_pct = pct(pending, max_val);

    let total_reviewed_pie = public + private;
    let accept_angle = if total_reviewed_pie > 0 {
        public as f64 / total_reviewed_pie as f64 * 360.0
    } else {
        0.0
    };

    let ratio_str = if public > 0 && private > 0 {
        format!("{:.2}:1", public as f64 / private as f64)
    } else if public > 0 {
        "\u{221e}:1".to_string()
    } else {
        "0:1".to_string()
    };
    let sessions_left_fmt = if pending > 0 {
        format!("{}+", (pending as f64 / 10.0).round() as i64)
    } else {
        "0".to_string()
    };

    let max_reviews = nik_total.max(prdp_total).max(1);
    let nik_reviews_bar_pct = pct(nik_total, max_reviews);
    let prdp_reviews_bar_pct = pct(prdp_total, max_reviews);
    let nik_accept_rate = pct(nik_accepted, nik_total);
    let prdp_accept_rate = pct(prdp_accepted, prdp_total);

    let bots = bot_breakdown(index_colls).await;

    let tmpl = TgfsStatsTemplate {
        logged_in: true,
        username: session.username,
        flashes,

        total_fmt: commas(total),
        pending,
        pending_fmt: commas(pending),
        public_fmt: commas(public),
        private_fmt: commas(private),
        reviewed_fmt: commas(reviewed),
        progress_pct,
        progress_pct_fmt: format!("{:.1}", progress_pct),

        bar_accept_pct,
        bar_reject_pct,
        bar_pending_pct,

        total_reviewed_fmt: commas(total_reviewed_pie),
        accept_angle,
        has_reviewed: total_reviewed_pie > 0,
        pie_accept_pct_fmt: fmt_pct1(public, total_reviewed_pie),
        pie_reject_pct_fmt: fmt_pct1(private, total_reviewed_pie),

        acceptance_rate_fmt: fmt_pct1(public, reviewed),
        rejection_rate_fmt: fmt_pct1(private, reviewed),
        ratio_str,
        sessions_left_fmt,

        nik,
        prdp,

        nik_reviews_bar_pct,
        prdp_reviews_bar_pct,
        nik_accept_rate,
        prdp_accept_rate,

        bots,

        show_plgb_nav: false,
        show_tgfs_nav: true,
    };

    (jar, render(tmpl))
}
