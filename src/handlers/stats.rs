use askama::Template;
use axum::extract::State;
use axum::response::IntoResponse;
use axum_extra::extract::cookie::PrivateCookieJar;
use mongodb::bson::doc;

use crate::auth::{take_flash, AuthUser};
use crate::models::{Flash, ReviewerStats};
use crate::state::AppState;
use crate::util::{commas, fmt_pct1, pct, render};

#[derive(Template)]
#[template(path = "stats.html")]
struct StatsTemplate {
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

    total_diff: i64,
    total_diff_class: String,
    accepted_diff: i64,
    accepted_diff_class: String,
    rejected_diff: i64,
    rejected_diff_class: String,
    rate_diff_fmt: String,
    rate_diff_class: String,
}

fn diff_class(a: i64, b: i64) -> String {
    if a > b {
        "positive".to_string()
    } else if a < b {
        "negative".to_string()
    } else {
        String::new()
    }
}

fn diff_class_f64(a: f64, b: f64) -> String {
    if a > b {
        "positive".to_string()
    } else if a < b {
        "negative".to_string()
    } else {
        String::new()
    }
}

async fn reviewer_stats(state: &AppState, name: &str, total_reviewed_all: i64) -> ReviewerStats {
    let total_reviewed = state
        .files
        .count_documents(doc! { "reviewed_by": name })
        .await
        .unwrap_or(0) as i64;
    let accepted = state
        .files
        .count_documents(doc! { "reviewed_by": name, "is_public": true })
        .await
        .unwrap_or(0) as i64;
    let rejected = state
        .files
        .count_documents(doc! { "reviewed_by": name, "is_public": false })
        .await
        .unwrap_or(0) as i64;

    ReviewerStats {
        total_reviewed_fmt: commas(total_reviewed),
        accepted_fmt: commas(accepted),
        rejected_fmt: commas(rejected),
        acceptance_rate_fmt: fmt_pct1(accepted, total_reviewed),
        contribution_fmt: fmt_pct1(total_reviewed, total_reviewed_all),
    }
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

    let total = state.files.count_documents(doc! {}).await.unwrap_or(0) as i64;
    let reviewed = state
        .files
        .count_documents(doc! { "is_public": { "$exists": true } })
        .await
        .unwrap_or(0) as i64;
    let pending = state
        .files
        .count_documents(doc! { "is_public": { "$exists": false } })
        .await
        .unwrap_or(0) as i64;
    let public = state
        .files
        .count_documents(doc! { "is_public": true })
        .await
        .unwrap_or(0) as i64;
    let private = state
        .files
        .count_documents(doc! { "is_public": false })
        .await
        .unwrap_or(0) as i64;

    let nik = reviewer_stats(&state, "nik", reviewed).await;
    let prdp = reviewer_stats(&state, "prdp", reviewed).await;

    // re-fetch the raw numeric totals needed for cross-reviewer comparisons
    let nik_total = state
        .files
        .count_documents(doc! { "reviewed_by": "nik" })
        .await
        .unwrap_or(0) as i64;
    let prdp_total = state
        .files
        .count_documents(doc! { "reviewed_by": "prdp" })
        .await
        .unwrap_or(0) as i64;
    let nik_accepted = state
        .files
        .count_documents(doc! { "reviewed_by": "nik", "is_public": true })
        .await
        .unwrap_or(0) as i64;
    let prdp_accepted = state
        .files
        .count_documents(doc! { "reviewed_by": "prdp", "is_public": true })
        .await
        .unwrap_or(0) as i64;
    let nik_rejected = state
        .files
        .count_documents(doc! { "reviewed_by": "nik", "is_public": false })
        .await
        .unwrap_or(0) as i64;
    let prdp_rejected = state
        .files
        .count_documents(doc! { "reviewed_by": "prdp", "is_public": false })
        .await
        .unwrap_or(0) as i64;

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

    let tmpl = StatsTemplate {
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

        total_diff: nik_total - prdp_total,
        total_diff_class: diff_class(nik_total, prdp_total),
        accepted_diff: nik_accepted - prdp_accepted,
        accepted_diff_class: diff_class(nik_accepted, prdp_accepted),
        rejected_diff: nik_rejected - prdp_rejected,
        rejected_diff_class: diff_class(nik_rejected, prdp_rejected),
        rate_diff_fmt: format!("{:.1}", nik_accept_rate - prdp_accept_rate),
        rate_diff_class: diff_class_f64(nik_accept_rate, prdp_accept_rate),
    };

    (jar, render(tmpl))
}
