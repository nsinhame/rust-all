use mongodb::bson::{oid::ObjectId, Bson, Document};

use crate::models::{get_bool, get_i64, get_str};
use crate::tgfs_token::{dl_url, watch_url};
use crate::util::format_ist;

/// Reads the `_id` field of a `user_files` doc as an `ObjectId` (they're always
/// server-generated, never a custom `_id`, unlike blob `files` docs).
fn get_index_oid(doc: &Document) -> Option<ObjectId> {
    match doc.get("_id") {
        Some(Bson::ObjectId(oid)) => Some(*oid),
        _ => None,
    }
}

/// Reads a BSON `DateTime` field as IST-formatted unix seconds, mirroring
/// `models::get_i64`/`get_f64` for telethon-plgb's `added_at` field.
pub fn get_datetime_ist(doc: &Document, key: &str) -> Option<String> {
    match doc.get(key) {
        Some(Bson::DateTime(dt)) => Some(format_ist(dt.timestamp_millis() as f64 / 1000.0)),
        _ => None,
    }
}

/// Encodes which index cluster a `user_files` document came from into its
/// review-page identifier, since the same Mongo ObjectId space isn't shared
/// across telethon-plgb's separate index cluster deployments.
pub fn encode_entry_id(cluster_idx: usize, oid: ObjectId) -> String {
    format!("{cluster_idx}:{}", oid.to_hex())
}

/// Inverse of [`encode_entry_id`].
pub fn decode_entry_id(s: &str) -> Option<(usize, ObjectId)> {
    let (cluster_str, oid_str) = s.split_once(':')?;
    let cluster_idx: usize = cluster_str.parse().ok()?;
    let oid = ObjectId::parse_str(oid_str).ok()?;
    Some((cluster_idx, oid))
}

/// View model for one reviewable "link" (a `user_files` entry joined with its
/// underlying blob `files` document) shown on the TGFS review/done pages.
#[derive(Clone, Default)]
pub struct TgfsFileCard {
    pub index: usize,
    /// `"{index_cluster}:{objectid_hex}"`, used as the DOM/decision key and in `/submit-tgfs`.
    pub entry_id: String,
    pub file_id: i64,
    pub bot_id: String,
    pub user_id: String,
    pub file_name: String,
    pub file_size: String,
    pub mime_type: String,
    pub username: String,
    pub forward_name: String,
    pub forward_username: String,
    pub has_forward_info: bool,
    pub is_owner: bool,
    pub dl_url: String,
    pub watch_url: String,
    pub added_at: String,
    // done-page only
    pub is_restricted: bool,
    pub reviewed_by: String,
    pub reviewed_at: String,
}

impl TgfsFileCard {
    /// Builds a card from an index (`user_files`) doc plus its (possibly missing,
    /// e.g. if orphaned) blob (`files`) doc.
    pub fn from_docs(
        index_doc: &Document,
        blob_doc: Option<&Document>,
        cluster_idx: usize,
        index: usize,
        secret: &[u8],
        public_url: &str,
    ) -> Self {
        let oid = get_index_oid(index_doc).unwrap_or_else(ObjectId::new);
        let entry_id = encode_entry_id(cluster_idx, oid);

        let user_id = get_i64(index_doc, "user_id").unwrap_or(0);
        let bot_id = get_i64(index_doc, "bot_id").unwrap_or(0);
        let file_id = get_i64(index_doc, "file_id").unwrap_or(0);

        let username = get_str(index_doc, "username").unwrap_or_default();
        let forward_name = get_str(index_doc, "forward_name").unwrap_or_default();
        let forward_username = get_str(index_doc, "forward_username").unwrap_or_default();
        let has_forward_info =
            !username.is_empty() || !forward_name.is_empty() || !forward_username.is_empty();

        let file_name = get_str(index_doc, "file_name")
            .or_else(|| blob_doc.and_then(|d| get_str(d, "file_name")))
            .unwrap_or_else(|| "Unknown".to_string());

        let file_size_bytes = get_i64(index_doc, "file_size")
            .or_else(|| blob_doc.and_then(|d| get_i64(d, "size")))
            .unwrap_or(0);

        let mime_type = blob_doc
            .and_then(|d| get_str(d, "mime_type"))
            .unwrap_or_else(|| "Unknown".to_string());
        let is_owner = blob_doc.map(|d| get_bool(d, "is_owner").unwrap_or(false)).unwrap_or(false);
        let is_restricted = blob_doc
            .map(|d| get_bool(d, "is_restricted").unwrap_or(false))
            .unwrap_or(false);
        let reviewed_by = blob_doc
            .and_then(|d| get_str(d, "reviewed_by"))
            .unwrap_or_default();
        let reviewed_at = blob_doc
            .and_then(|d| get_datetime_ist(d, "reviewed_at").or_else(|| get_str(d, "reviewed_at")))
            .unwrap_or_default();

        let added_at = get_datetime_ist(index_doc, "added_at").unwrap_or_else(|| "N/A".to_string());

        TgfsFileCard {
            index,
            entry_id,
            file_id,
            bot_id: bot_id.to_string(),
            user_id: user_id.to_string(),
            file_name,
            file_size: crate::util::format_file_size(file_size_bytes),
            mime_type,
            username,
            forward_name,
            forward_username,
            has_forward_info,
            is_owner,
            dl_url: dl_url(public_url, secret, user_id, file_id),
            watch_url: watch_url(public_url, secret, user_id, file_id),
            added_at,
            is_restricted,
            reviewed_by,
            reviewed_at,
        }
    }
}

/// Combined search statistics shown in the TGFS review page's right sidebar.
#[derive(Clone, Default)]
pub struct TgfsReviewStats {
    pub search_user_id: String,
    pub search_bot_id: String,
    pub search_file_name: String,
    pub size_filter_active: bool,
    pub search_size_min: i64,
    pub search_size_max: i64,
    pub date_filter_active: bool,
    pub search_date_from: String,
    pub search_date_to: String,
    pub is_exclusion: bool,
    pub total_fmt: String,
    pub reviewed_fmt: String,
    pub pending_fmt: String,
    pub accepted_fmt: String,
    pub rejected_fmt: String,
    pub progress_pct_fmt: String,
    pub acceptance_rate_fmt: String,
    pub rejection_rate_fmt: String,
    pub sessions_left_fmt: String,
}

/// Combined search statistics shown in the TGFS done page's right sidebar.
#[derive(Clone, Default)]
pub struct TgfsDoneStats {
    pub search_user_id: String,
    pub search_bot_id: String,
    pub search_file_name: String,
    pub search_reviewed_by: String,
    pub size_filter_active: bool,
    pub search_size_min: i64,
    pub search_size_max: i64,
    pub date_filter_active: bool,
    pub search_date_from: String,
    pub search_date_to: String,
    pub is_exclusion: bool,
    pub total_fmt: String,
    pub accepted_fmt: String,
    pub rejected_fmt: String,
    pub acceptance_rate_fmt: String,
    pub rejection_rate_fmt: String,
}

/// One row of the TGFS stats page's per-bot breakdown table.
#[derive(Clone, Default)]
pub struct TgfsBotBreakdown {
    pub bot_id: String,
    pub count_fmt: String,
}
