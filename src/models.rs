use mongodb::bson::oid::ObjectId;
use mongodb::bson::{Bson, Document};

use crate::util::format_file_size;

// ---------------------------------------------------------------------
// Loose BSON field accessors.
//
// The `file` collection is populated by an external Telegram bot, so field
// types are not strictly guaranteed (e.g. `file_size` might be an Int32,
// Int64 or Double depending on how it was inserted). These helpers coerce
// leniently instead of failing deserialization, mirroring Python's
// dynamically-typed `doc.get(key, default)` access pattern.
// ---------------------------------------------------------------------

pub fn get_str(doc: &Document, key: &str) -> Option<String> {
    match doc.get(key) {
        Some(Bson::String(s)) => Some(s.clone()),
        _ => None,
    }
}

pub fn get_i64(doc: &Document, key: &str) -> Option<i64> {
    match doc.get(key) {
        Some(Bson::Int32(i)) => Some(*i as i64),
        Some(Bson::Int64(i)) => Some(*i),
        Some(Bson::Double(d)) => Some(*d as i64),
        _ => None,
    }
}

pub fn get_bool(doc: &Document, key: &str) -> Option<bool> {
    match doc.get(key) {
        Some(Bson::Boolean(b)) => Some(*b),
        _ => None,
    }
}

pub fn get_f64(doc: &Document, key: &str) -> Option<f64> {
    match doc.get(key) {
        Some(Bson::Double(d)) => Some(*d),
        Some(Bson::Int32(i)) => Some(*i as f64),
        Some(Bson::Int64(i)) => Some(*i as f64),
        _ => None,
    }
}

pub fn get_object_id(doc: &Document) -> Option<ObjectId> {
    match doc.get("_id") {
        Some(Bson::ObjectId(oid)) => Some(*oid),
        _ => None,
    }
}

/// A single flash message rendered once at the top of a page.
#[derive(Clone)]
pub struct Flash {
    pub category: String,
    pub message: String,
}

/// View model for one file card shown on the review/done pages.
#[derive(Clone, Default)]
pub struct FileCard {
    pub index: usize,
    pub id: String,
    pub user_id: String,
    pub file_name: String,
    pub file_size: String,
    pub mime_type: String,
    pub username: String,
    pub forward_first_name: String,
    pub forward_username: String,
    pub forward_chat_title: String,
    pub forward_chat_username: String,
    pub has_forward_info: bool,
    pub dl_url: String,
    pub watch_url: String,
    // done-page only
    pub is_public: bool,
    pub special_type: String,
    pub reviewed_by: String,
    pub reviewed_at: String,
}

impl FileCard {
    pub fn from_doc(doc: &Document, index: usize, fqdn: &str) -> Self {
        let id = get_object_id(doc).map(|o| o.to_hex()).unwrap_or_default();
        let username = get_str(doc, "username").unwrap_or_default();
        let forward_first_name = get_str(doc, "forward_first_name").unwrap_or_default();
        let forward_username = get_str(doc, "forward_username").unwrap_or_default();
        let forward_chat_title = get_str(doc, "forward_chat_title").unwrap_or_default();
        let forward_chat_username = get_str(doc, "forward_chat_username").unwrap_or_default();
        let has_forward_info = !username.is_empty()
            || !forward_first_name.is_empty()
            || !forward_username.is_empty()
            || !forward_chat_title.is_empty()
            || !forward_chat_username.is_empty();

        let reviewed_at = get_f64(doc, "reviewed_at")
            .map(crate::util::format_ist)
            .unwrap_or_default();

        let file_name = get_str(doc, "file_name").unwrap_or_else(|| "Unknown".to_string());
        let fqdn = fqdn.trim().trim_end_matches('/');
        let dl_url = format!("https://{fqdn}/dl/{id}/{file_name}");
        let watch_url = format!("https://{fqdn}/watch/{id}/{file_name}");

        FileCard {
            index,
            id,
            user_id: get_i64(doc, "user_id")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "N/A".to_string()),
            file_name,
            file_size: format_file_size(get_i64(doc, "file_size").unwrap_or(0)),
            mime_type: get_str(doc, "mime_type").unwrap_or_else(|| "Unknown".to_string()),
            username,
            forward_first_name,
            forward_username,
            forward_chat_title,
            forward_chat_username,
            has_forward_info,
            dl_url,
            watch_url,
            is_public: get_bool(doc, "is_public").unwrap_or(false),
            special_type: get_str(doc, "special_type").unwrap_or_default(),
            reviewed_by: get_str(doc, "reviewed_by").unwrap_or_default(),
            reviewed_at,
        }
    }
}

/// Combined search statistics shown in the review page's right sidebar.
#[derive(Clone, Default)]
pub struct ReviewStats {
    pub search_user_id: String,
    pub search_file_name: String,
    pub size_filter_active: bool,
    pub search_size_min: i64,
    pub search_size_max: i64,
    pub is_exclusion: bool,
    pub total_fmt: String,
    pub reviewed_fmt: String,
    pub pending_fmt: String,
    pub accepted_fmt: String,
    pub rejected_fmt: String,
    pub excluded_count_fmt: String,
    pub has_excluded_count: bool,
    pub progress_pct_fmt: String,
    pub acceptance_rate_fmt: String,
    pub rejection_rate_fmt: String,
    pub sessions_left_fmt: String,
}

/// Combined search statistics shown in the done page's right sidebar.
#[derive(Clone, Default)]
pub struct DoneStats {
    pub search_user_id: String,
    pub search_file_name: String,
    pub search_reviewed_by: String,
    pub size_filter_active: bool,
    pub search_size_min: i64,
    pub search_size_max: i64,
    pub is_exclusion: bool,
    pub total_fmt: String,
    pub accepted_fmt: String,
    pub rejected_fmt: String,
    pub acceptance_rate_fmt: String,
    pub rejection_rate_fmt: String,
}

/// Per-reviewer stats block used on the stats dashboard.
#[derive(Clone, Default)]
pub struct ReviewerStats {
    pub total_reviewed_fmt: String,
    pub accepted_fmt: String,
    pub rejected_fmt: String,
    pub acceptance_rate_fmt: String,
    pub contribution_fmt: String,
}
