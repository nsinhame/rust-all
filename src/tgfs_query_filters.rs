use mongodb::bson::{doc, Document};

use crate::util::{ist_date_end_epoch, ist_date_start_epoch};

/// Converts an IST-calendar-day epoch (seconds, as returned by
/// `ist_date_start_epoch`/`ist_date_end_epoch`) into a BSON `DateTime`, since
/// telethon-plgb stores `user_files.added_at` as a native Mongo date, not the
/// unix-seconds float the plgb bot uses for `time`.
fn epoch_to_bson_datetime(epoch_secs: f64) -> mongodb::bson::DateTime {
    mongodb::bson::DateTime::from_millis((epoch_secs * 1000.0) as i64)
}

/// Result of parsing the TGFS review/done search form (see `query_filters::ParsedFilters`
/// for the PLGB equivalent this mirrors). Conditions apply to `user_files` doc fields only;
/// review/reviewed-status filtering happens separately against the joined blob doc.
pub struct TgfsParsedFilters {
    pub conditions: Vec<Document>,
    pub search_user_id: String,
    /// Absolute values of every comma-separated user id supplied (OR'd together via `$in`,
    /// or `$nin` when all are negative). Empty if not supplied/invalid.
    pub actual_user_ids: Vec<i64>,
    pub is_exclusion: bool,
    pub search_bot_id: String,
    pub actual_bot_id: Option<i64>,
    pub bot_is_exclusion: bool,
    pub search_file_name: String,
    pub search_forward_from: String,
    pub search_size_min: i64,
    pub search_size_max: i64,
    pub size_filter_active: bool,
    pub search_file_id: String,
    pub search_date_from: String,
    pub search_date_to: String,
    pub date_filter_active: bool,
    pub invalid_user_id: bool,
    pub invalid_bot_id: bool,
    pub invalid_file_id: bool,
    pub invalid_date_range: bool,
}

pub struct TgfsRawFilterInput<'a> {
    pub user_id: &'a str,
    pub bot_id: &'a str,
    pub file_name: &'a str,
    pub forward_from: &'a str,
    pub size_min: &'a str,
    pub size_max: &'a str,
    pub file_id: &'a str,
    pub date_from: &'a str,
    pub date_to: &'a str,
}

pub fn parse_tgfs_filters(input: TgfsRawFilterInput) -> TgfsParsedFilters {
    let mut conditions = Vec::new();

    // --- size range (MB, default 0-4200) ---
    let mut size_min: i64 = input.size_min.trim().parse().unwrap_or(0);
    let mut size_max: i64 = input.size_max.trim().parse().unwrap_or(4200);
    size_min = size_min.clamp(0, 4200);
    size_max = size_max.clamp(0, 4200);
    if size_min > size_max {
        std::mem::swap(&mut size_min, &mut size_max);
    }
    let size_filter_active = size_min > 0 || size_max < 4200;

    // --- user id (comma-separated list OR'd together; positive = include, negative = exclude) ---
    let mut search_user_id = input.user_id.trim().to_string();
    let mut actual_user_ids: Vec<i64> = Vec::new();
    let mut is_exclusion = false;
    let mut invalid_user_id = false;
    if !search_user_id.is_empty() {
        let parts: Vec<&str> = search_user_id
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let parsed_ids: Result<Vec<i64>, _> = parts.iter().map(|p| p.parse::<i64>()).collect();
        match parsed_ids {
            Ok(ids) if !ids.is_empty() => {
                is_exclusion = ids.iter().all(|v| *v < 0);
                let abs_ids: Vec<i64> = ids.iter().map(|v| v.abs()).collect();
                actual_user_ids = abs_ids.clone();
                if is_exclusion {
                    if abs_ids.len() == 1 {
                        conditions.push(doc! { "user_id": { "$ne": abs_ids[0] } });
                    } else {
                        conditions.push(doc! { "user_id": { "$nin": abs_ids } });
                    }
                } else if abs_ids.len() == 1 {
                    conditions.push(doc! { "user_id": abs_ids[0] });
                } else {
                    conditions.push(doc! { "user_id": { "$in": abs_ids } });
                }
            }
            _ => {
                invalid_user_id = true;
                search_user_id.clear();
            }
        }
    }

    // --- bot id (positive = include, negative = exclude) ---
    let mut search_bot_id = input.bot_id.trim().to_string();
    let mut actual_bot_id: Option<i64> = None;
    let mut bot_is_exclusion = false;
    let mut invalid_bot_id = false;
    if !search_bot_id.is_empty() {
        match search_bot_id.parse::<i64>() {
            Ok(parsed) => {
                if parsed < 0 {
                    bot_is_exclusion = true;
                    let abs_id = parsed.abs();
                    actual_bot_id = Some(abs_id);
                    conditions.push(doc! { "bot_id": { "$ne": abs_id } });
                } else {
                    actual_bot_id = Some(parsed);
                    conditions.push(doc! { "bot_id": parsed });
                }
            }
            Err(_) => {
                invalid_bot_id = true;
                search_bot_id.clear();
            }
        }
    }

    // --- file name (all words must match, AND) ---
    let search_file_name = input.file_name.trim().to_string();
    if !search_file_name.is_empty() {
        for word in search_file_name.split_whitespace() {
            conditions.push(doc! { "file_name": { "$regex": word, "$options": "i" } });
        }
    }

    // --- size filter ---
    if size_filter_active {
        let min_bytes = size_min * 1024 * 1024;
        let max_bytes = size_max * 1024 * 1024;
        conditions.push(doc! { "file_size": { "$gte": min_bytes, "$lte": max_bytes } });
    }

    // --- forward-from (OR across the 2 fields telethon-plgb records) ---
    let search_forward_from = input.forward_from.trim().to_string();
    if !search_forward_from.is_empty() {
        let regex = doc! { "$regex": &search_forward_from, "$options": "i" };
        conditions.push(doc! { "$or": [
            { "forward_name": regex.clone() },
            { "forward_username": regex },
        ] });
    }

    // --- Telegram file id (exact numeric match on user_files.file_id) ---
    let mut search_file_id = input.file_id.trim().to_string();
    let mut invalid_file_id = false;
    if !search_file_id.is_empty() {
        match search_file_id.parse::<i64>() {
            Ok(fid) => conditions.push(doc! { "file_id": fid }),
            Err(_) => {
                invalid_file_id = true;
                search_file_id.clear();
            }
        }
    }

    // --- added_at date range (IST calendar days) ---
    let mut search_date_from = input.date_from.trim().to_string();
    let mut search_date_to = input.date_to.trim().to_string();
    let mut invalid_date_range = false;
    let mut from_epoch = if search_date_from.is_empty() {
        None
    } else {
        match ist_date_start_epoch(&search_date_from) {
            Some(epoch) => Some(epoch),
            None => {
                invalid_date_range = true;
                search_date_from.clear();
                None
            }
        }
    };
    let mut to_epoch = if search_date_to.is_empty() {
        None
    } else {
        match ist_date_end_epoch(&search_date_to) {
            Some(epoch) => Some(epoch),
            None => {
                invalid_date_range = true;
                search_date_to.clear();
                None
            }
        }
    };
    if let (Some(from), Some(to)) = (from_epoch, to_epoch) {
        if from > to {
            std::mem::swap(&mut from_epoch, &mut to_epoch);
            std::mem::swap(&mut search_date_from, &mut search_date_to);
        }
    }
    let date_filter_active = from_epoch.is_some() || to_epoch.is_some();
    if date_filter_active {
        let mut range = Document::new();
        if let Some(from) = from_epoch {
            range.insert("$gte", epoch_to_bson_datetime(from));
        }
        if let Some(to) = to_epoch {
            range.insert("$lte", epoch_to_bson_datetime(to));
        }
        conditions.push(doc! { "added_at": range });
    }

    TgfsParsedFilters {
        conditions,
        search_user_id,
        actual_user_ids,
        is_exclusion,
        search_bot_id,
        actual_bot_id,
        bot_is_exclusion,
        search_file_name,
        search_forward_from,
        search_size_min: size_min,
        search_size_max: size_max,
        size_filter_active,
        search_file_id,
        search_date_from,
        search_date_to,
        date_filter_active,
        invalid_user_id,
        invalid_bot_id,
        invalid_file_id,
        invalid_date_range,
    }
}
