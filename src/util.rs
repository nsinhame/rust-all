/// Convert a byte count to a human readable string, mirroring the Python
/// `format_file_size` helper (binary/1024-based units, 2 decimal places).
pub fn format_file_size(bytes: i64) -> String {
    let mut size = bytes as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if size.abs() < 1024.0 {
            return format!("{:.2} {}", size, unit);
        }
        size /= 1024.0;
    }
    format!("{:.2} TB", size)
}

/// Escapes regex metacharacters so a raw user-supplied search term is matched as a
/// literal substring in a MongoDB `$regex` filter instead of being parsed as a
/// (possibly invalid, e.g. a lone `+`/`*`/`?`) regex pattern.
pub fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Format an integer with thousands separators, e.g. 1234567 -> "1,234,567".
pub fn commas(n: i64) -> String {
    let negative = n < 0;
    let digits = n.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let mut result: String = grouped.chars().rev().collect();
    if negative {
        result.insert(0, '-');
    }
    result
}

/// Percentage of `n` over `d`, or 0.0 if `d` is not positive.
pub fn pct(n: i64, d: i64) -> f64 {
    if d > 0 {
        (n as f64 / d as f64) * 100.0
    } else {
        0.0
    }
}

/// Percentage formatted to one decimal place, e.g. "42.3".
pub fn fmt_pct1(n: i64, d: i64) -> String {
    format!("{:.1}", pct(n, d))
}

/// Percent-encodes a string for safe use as a URL query parameter value.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Renders an Askama template to an HTTP response, turning render errors into a 500.
pub fn render<T: askama::Template>(tmpl: T) -> axum::response::Response {
    use axum::response::{Html, IntoResponse};
    match tmpl.render() {
        Ok(body) => Html(body).into_response(),
        Err(err) => {
            tracing::error!("template render error: {err}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "template rendering failed",
            )
                .into_response()
        }
    }
}

/// Renders a unix-epoch-seconds timestamp as an IST (UTC+5:30) date/time string.
pub fn format_ist(ts: f64) -> String {
    use chrono::{DateTime, FixedOffset, Utc};

    let secs = ts as i64;
    let dt_utc = match DateTime::<Utc>::from_timestamp(secs, 0) {
        Some(dt) => dt,
        None => return "N/A".to_string(),
    };
    let ist_offset = FixedOffset::east_opt(5 * 3600 + 1800).expect("valid fixed offset");
    let dt_ist = dt_utc.with_timezone(&ist_offset);
    dt_ist.format("%Y-%m-%d %H:%M IST").to_string()
}

const IST_OFFSET_SECS: i32 = 5 * 3600 + 1800;

/// Parses a `YYYY-MM-DD` (HTML `<input type=date>`) string as an IST calendar day and
/// returns the unix-epoch-seconds instant of its start (`00:00:00`) in that timezone.
/// Returns `None` if `date_str` is empty or not a valid date.
pub fn ist_date_start_epoch(date_str: &str) -> Option<f64> {
    use chrono::{FixedOffset, NaiveDate, TimeZone};

    let date_str = date_str.trim();
    if date_str.is_empty() {
        return None;
    }
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    let naive_dt = date.and_hms_opt(0, 0, 0)?;
    let ist_offset = FixedOffset::east_opt(IST_OFFSET_SECS).expect("valid fixed offset");
    Some(ist_offset.from_local_datetime(&naive_dt).single()?.timestamp() as f64)
}

/// Same as [`ist_date_start_epoch`] but returns the instant of the day's last second
/// (`23:59:59`) in IST, for use as the inclusive upper bound of a date-range filter.
pub fn ist_date_end_epoch(date_str: &str) -> Option<f64> {
    use chrono::{FixedOffset, NaiveDate, TimeZone};

    let date_str = date_str.trim();
    if date_str.is_empty() {
        return None;
    }
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    let naive_dt = date.and_hms_opt(23, 59, 59)?;
    let ist_offset = FixedOffset::east_opt(IST_OFFSET_SECS).expect("valid fixed offset");
    Some(ist_offset.from_local_datetime(&naive_dt).single()?.timestamp() as f64)
}
