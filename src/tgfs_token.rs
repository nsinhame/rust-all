//! Reimplementation of telethon-plgb's `make_token` link signing scheme
//! (see `tgfs/utils/utils.py`), so this Rust app can build working `/dl/` and
//! `/wt/` links against the same running tgfilestream server without needing
//! Python or an HTTP round-trip.
//!
//! Token format: `base64url_nopad(payload) + "/" + base64url_nopad(sig)` where
//! `payload = big-endian(user_id: u64) ++ big-endian(file_id: u64) ++ (cluster: u8)`
//! and `sig = HMAC-SHA256(secret, payload)`. The cluster byte isn't actually
//! consulted server-side when a `user_id` is supplied (routes.py resolves the
//! cluster itself from the user's index entry), so it's always written as 0.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Builds the `{payload_b64}/{sig_b64}` token telethon-plgb expects after `/dl/` or `/wt/`.
pub fn make_token(secret: &[u8], user_id: i64, file_id: i64) -> String {
    let mut payload = Vec::with_capacity(17);
    payload.extend_from_slice(&(user_id as u64).to_be_bytes());
    payload.extend_from_slice(&(file_id as u64).to_be_bytes());
    payload.push(0u8);

    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&payload);
    let sig = mac.finalize().into_bytes();

    format!(
        "{}/{}",
        URL_SAFE_NO_PAD.encode(&payload),
        URL_SAFE_NO_PAD.encode(sig)
    )
}

/// Builds the download (`/dl/`) link for a file, owned by `user_id`, against `public_url`.
pub fn dl_url(public_url: &str, secret: &[u8], user_id: i64, file_id: i64) -> String {
    format!("{public_url}/dl/{}", make_token(secret, user_id, file_id))
}

/// Builds the inline-watch (`/wt/`) link for a file, owned by `user_id`, against `public_url`.
pub fn watch_url(public_url: &str, secret: &[u8], user_id: i64, file_id: i64) -> String {
    format!("{public_url}/wt/{}", make_token(secret, user_id, file_id))
}
