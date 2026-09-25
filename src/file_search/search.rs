use futures_util::TryStreamExt;
use mongodb::bson::oid::ObjectId;
use mongodb::bson::{doc, Document};

use crate::link_review::models::{get_bool, get_i64, FileCard};
use crate::link_review::tgfs_join;
use crate::link_review::tgfs_models::{decode_entry_id, TgfsFileCard};
use crate::file_search::models::{icon_for, ResultTile};
use crate::file_search::token::{encode_token, Source};
use crate::state::AppState;
use crate::util::regex_escape;

/// Cap on how many raw candidates are pulled per source before filtering/sorting —
/// keeps a single search fast without needing full-text/indexed search infrastructure.
/// A generous bound for a self-hosted tool's scale; see `tgfs_join::TGFS_CANDIDATE_LIMIT`
/// for the equivalent tradeoff already made on the review side.
const CANDIDATE_LIMIT: i64 = 150;

/// Builds the `file_name` half of a search filter requiring every whitespace-separated
/// word in `query` to appear somewhere in the name, in any order — matching how the
/// review system's own "File Name" advanced-search field behaves (see
/// `link_review::query_filters::parse_filters`), instead of requiring the whole query
/// to appear as one literal substring in that exact order.
fn file_name_word_conditions(query: &str) -> Vec<Document> {
    query
        .split_whitespace()
        .map(|word| doc! { "file_name": { "$regex": regex_escape(word), "$options": "i" } })
        .collect()
}

async fn search_plgb(state: &AppState, query: &str) -> Vec<ResultTile> {
    let mut conditions = vec![doc! { "is_public": true }];
    conditions.extend(file_name_word_conditions(query));
    let filter = doc! { "$and": conditions };
    let docs: Vec<Document> = match state.files.find(filter).limit(CANDIDATE_LIMIT).await {
        Ok(cursor) => cursor.try_collect().await.unwrap_or_default(),
        Err(err) => {
            tracing::error!("file-search plgb search error: {err}");
            Vec::new()
        }
    };

    docs.iter()
        .filter_map(|d| {
            let card = FileCard::from_doc(d, 0, &state.fqdn);
            let oid = ObjectId::parse_str(&card.id).ok()?;
            Some(ResultTile {
                token: encode_token(state.access_key.as_bytes(), Source::Plgb, oid),
                icon: icon_for(&card.mime_type, &card.file_name),
                file_name: card.file_name,
                file_size_fmt: card.file_size,
            })
        })
        .collect()
}

async fn search_tgfs(state: &AppState, query: &str) -> Vec<ResultTile> {
    let filter = doc! { "$and": file_name_word_conditions(query) };
    let mut candidates: Vec<(usize, Document)> = Vec::new();
    for (i, coll) in state.tgfs.index_colls.iter().enumerate() {
        let docs: Vec<Document> = match coll.find(filter.clone()).limit(CANDIDATE_LIMIT).await {
            Ok(cursor) => cursor.try_collect().await.unwrap_or_default(),
            Err(err) => {
                tracing::error!("file-search tgfs search error (cluster {i}): {err}");
                Vec::new()
            }
        };
        candidates.extend(docs.into_iter().map(|d| (i, d)));
    }

    let wanted: Vec<(usize, i64)> = candidates
        .iter()
        .filter_map(|(_, d)| get_i64(d, "file_id").map(|fid| (tgfs_join::blob_cluster_of(state, d), fid)))
        .collect();
    let blob_map = tgfs_join::fetch_blob_map(state, &wanted).await;

    candidates
        .iter()
        .filter_map(|(cluster, index_doc)| {
            let file_id = get_i64(index_doc, "file_id").unwrap_or(0);
            let blob_doc = blob_map.get(&file_id)?;
            // Public search only ever surfaces already-reviewed & accepted links.
            if get_bool(blob_doc, "is_restricted").unwrap_or(true) || !blob_doc.contains_key("reviewed_at") {
                return None;
            }
            let card = TgfsFileCard::from_docs(
                index_doc,
                Some(blob_doc),
                *cluster,
                0,
                &state.tgfs.link_secret,
                &state.tgfs.public_url,
            );
            let (cluster_idx, oid) = decode_entry_id(&card.entry_id)?;
            Some(ResultTile {
                token: encode_token(
                    state.access_key.as_bytes(),
                    Source::Tgfs { cluster_idx: cluster_idx as u8 },
                    oid,
                ),
                icon: icon_for(&card.mime_type, &card.file_name),
                file_name: card.file_name,
                file_size_fmt: card.file_size,
            })
        })
        .collect()
}

/// Searches both databases by case-insensitive file-name substring, combining only
/// already-public/accepted PLGB files and already-accepted TGFS links into one list.
pub async fn combined_search(state: &AppState, query: &str) -> Vec<ResultTile> {
    let (mut plgb, mut tgfs) = (search_plgb(state, query).await, search_tgfs(state, query).await);
    let mut results = Vec::with_capacity(plgb.len() + tgfs.len());
    results.append(&mut plgb);
    results.append(&mut tgfs);
    results.sort_by(|a, b| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()));
    results
}
