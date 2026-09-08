use std::collections::HashMap;

use futures_util::TryStreamExt;
use mongodb::bson::{doc, Document};
use mongodb::Collection;

use crate::models::get_i64;
use crate::state::AppState;

/// Working-set cap for operations that must materialize every `user_files`
/// document matching a structural filter before it can join against the blob
/// cluster for review status (telethon-plgb's index/blob split means that
/// join can't be pushed down into a single Mongo query). Fine for a
/// self-hosted bot's scale; extremely large filtered result sets are simply
/// truncated rather than scanned in full.
pub const TGFS_CANDIDATE_LIMIT: i64 = 5000;

/// Reads which blob cluster an index (`user_files`) doc's file lives in, clamped
/// to a valid index into `state.tgfs.blob_colls`.
pub fn blob_cluster_of(state: &AppState, index_doc: &Document) -> usize {
    let raw = get_i64(index_doc, "cluster").unwrap_or(0).max(0) as usize;
    if raw >= state.tgfs.blob_colls.len() {
        0
    } else {
        raw
    }
}

/// Batch-fetches blob docs for a set of `(blob_cluster, file_id)` pairs, grouped
/// per cluster into a single `$in` query each. Missing/orphaned file_ids are
/// simply absent from the returned map.
pub async fn fetch_blob_map(
    state: &AppState,
    wanted: &[(usize, i64)],
) -> HashMap<i64, Document> {
    let mut by_cluster: HashMap<usize, Vec<i64>> = HashMap::new();
    for (cluster, file_id) in wanted {
        by_cluster.entry(*cluster).or_default().push(*file_id);
    }

    let mut result = HashMap::new();
    for (cluster, file_ids) in by_cluster {
        let Some(coll) = state.tgfs.blob_colls.get(cluster) else {
            continue;
        };
        let cursor = match coll.find(doc! { "_id": { "$in": file_ids } }).await {
            Ok(c) => c,
            Err(err) => {
                tracing::error!("tgfs blob fetch error (cluster {cluster}): {err}");
                continue;
            }
        };
        let docs: Vec<Document> = cursor.try_collect().await.unwrap_or_default();
        for doc in docs {
            if let Some(fid) = get_i64(&doc, "_id") {
                result.insert(fid, doc);
            }
        }
    }
    result
}

/// True if a file hasn't been through the review workflow yet: no `reviewed_at`
/// on its blob doc (or the blob doc is missing entirely, e.g. an orphaned index
/// entry — treated as pending so it still surfaces for cleanup during review).
pub fn is_pending(blob_doc: Option<&Document>) -> bool {
    match blob_doc {
        Some(doc) => !doc.contains_key("reviewed_at"),
        None => true,
    }
}

/// Runs `$match(match_doc) + $sample(size)` against one index cluster.
async fn sample_index_cluster(
    coll: &Collection<Document>,
    match_doc: &Document,
    size: i64,
) -> Vec<Document> {
    let pipeline = vec![
        doc! { "$match": match_doc.clone() },
        doc! { "$sample": { "size": size } },
    ];
    match coll.aggregate(pipeline).await {
        Ok(cursor) => cursor.try_collect().await.unwrap_or_default(),
        Err(err) => {
            tracing::error!("tgfs index sample error: {err}");
            Vec::new()
        }
    }
}

/// Random sample of up to `page_size` *pending* (never-reviewed) index docs
/// across every index cluster, tagged with which cluster each came from.
/// Because "pending" depends on the joined blob doc (a different DB deployment
/// that Mongo can't `$lookup` across), this over-samples per cluster and
/// retries a bounded number of times with a bigger pool instead of filtering
/// server-side.
pub async fn sample_pending(
    state: &AppState,
    match_doc: &Document,
    page_size: i64,
) -> Vec<(usize, Document)> {
    let mut multiplier = 4i64;
    for _attempt in 0..3 {
        let per_cluster_size = (page_size * multiplier).min(2000);
        let mut pool: Vec<(usize, Document)> = Vec::new();
        for (i, coll) in state.tgfs.index_colls.iter().enumerate() {
            let docs = sample_index_cluster(coll, match_doc, per_cluster_size).await;
            pool.extend(docs.into_iter().map(|d| (i, d)));
        }

        let wanted: Vec<(usize, i64)> = pool
            .iter()
            .filter_map(|(_, d)| get_i64(d, "file_id").map(|fid| (blob_cluster_of(state, d), fid)))
            .collect();
        let blob_map = fetch_blob_map(state, &wanted).await;

        let mut pending: Vec<(usize, Document)> = pool
            .into_iter()
            .filter(|(_, d)| {
                let fid = get_i64(d, "file_id").unwrap_or(0);
                is_pending(blob_map.get(&fid))
            })
            .collect();

        if pending.len() as i64 >= page_size || multiplier >= 32 {
            pending.truncate(page_size as usize);
            return pending;
        }
        multiplier *= 4;
    }
    Vec::new()
}

/// Materializes up to [`TGFS_CANDIDATE_LIMIT`] index docs per cluster matching
/// `match_doc`, plus their joined blob docs (keyed by file_id). Used for the
/// Done page and for review/done stats, where every matching entry (not just a
/// random sample) needs to be classified by review status.
pub async fn candidates_with_blob(
    state: &AppState,
    match_doc: &Document,
) -> (Vec<(usize, Document)>, HashMap<i64, Document>) {
    let mut pool: Vec<(usize, Document)> = Vec::new();
    for (i, coll) in state.tgfs.index_colls.iter().enumerate() {
        let cursor = match coll.find(match_doc.clone()).limit(TGFS_CANDIDATE_LIMIT).await {
            Ok(c) => c,
            Err(err) => {
                tracing::error!("tgfs candidates find error: {err}");
                continue;
            }
        };
        let docs: Vec<Document> = cursor.try_collect().await.unwrap_or_default();
        pool.extend(docs.into_iter().map(|d| (i, d)));
    }

    let wanted: Vec<(usize, i64)> = pool
        .iter()
        .filter_map(|(_, d)| get_i64(d, "file_id").map(|fid| (blob_cluster_of(state, d), fid)))
        .collect();
    let blob_map = fetch_blob_map(state, &wanted).await;
    (pool, blob_map)
}

