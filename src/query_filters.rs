use mongodb::bson::{doc, oid::ObjectId, Document};

/// Result of parsing the shared set of search-form fields used by both the
/// `/review` and `/done` pages (user id, file name, forward-from, size range,
/// mongo `_id`). Callers add their own extra conditions (e.g. the `is_public`
/// existence check) on top of `conditions`.
pub struct ParsedFilters {
    pub conditions: Vec<Document>,
    /// Raw user id string as typed by the user (kept for re-populating the form).
    pub search_user_id: String,
    /// Absolute value of the parsed user id (None if not supplied/invalid).
    pub actual_user_id: Option<i64>,
    pub is_exclusion: bool,
    pub search_file_name: String,
    pub search_forward_from: String,
    pub search_size_min: i64,
    pub search_size_max: i64,
    pub size_filter_active: bool,
    pub search_id: String,
    pub invalid_user_id: bool,
    pub invalid_file_id: bool,
}

pub struct RawFilterInput<'a> {
    pub user_id: &'a str,
    pub file_name: &'a str,
    pub forward_from: &'a str,
    pub size_min: &'a str,
    pub size_max: &'a str,
    pub id: &'a str,
}

pub fn parse_filters(input: RawFilterInput) -> ParsedFilters {
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

    // --- user id (positive = include, negative = exclude) ---
    let mut search_user_id = input.user_id.trim().to_string();
    let mut actual_user_id: Option<i64> = None;
    let mut is_exclusion = false;
    let mut invalid_user_id = false;
    if !search_user_id.is_empty() {
        match search_user_id.parse::<i64>() {
            Ok(parsed) => {
                if parsed < 0 {
                    is_exclusion = true;
                    let abs_id = parsed.abs();
                    actual_user_id = Some(abs_id);
                    conditions.push(doc! { "user_id": { "$ne": abs_id } });
                } else {
                    actual_user_id = Some(parsed);
                    conditions.push(doc! { "user_id": parsed });
                }
            }
            Err(_) => {
                invalid_user_id = true;
                search_user_id.clear();
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

    // --- forward-from (OR across 4 fields) ---
    let search_forward_from = input.forward_from.trim().to_string();
    if !search_forward_from.is_empty() {
        let regex = doc! { "$regex": &search_forward_from, "$options": "i" };
        conditions.push(doc! { "$or": [
            { "forward_first_name": regex.clone() },
            { "forward_username": regex.clone() },
            { "forward_chat_title": regex.clone() },
            { "forward_chat_username": regex },
        ] });
    }

    // --- mongo _id ---
    let mut search_id = input.id.trim().to_string();
    let mut invalid_file_id = false;
    if !search_id.is_empty() {
        match ObjectId::parse_str(&search_id) {
            Ok(oid) => conditions.push(doc! { "_id": oid }),
            Err(_) => {
                invalid_file_id = true;
                search_id.clear();
            }
        }
    }

    ParsedFilters {
        conditions,
        search_user_id,
        actual_user_id,
        is_exclusion,
        search_file_name,
        search_forward_from,
        search_size_min: size_min,
        search_size_max: size_max,
        size_filter_active,
        search_id,
        invalid_user_id,
        invalid_file_id,
    }
}
