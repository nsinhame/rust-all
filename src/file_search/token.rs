use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use mongodb::bson::oid::ObjectId;
use sha2::Sha256;

const SOURCE_PLGB: u8 = 0;
const SOURCE_TGFS: u8 = 1;

/// Which database a `/file-search/file/{token}` link points at, plus whatever else is
/// needed to look the file back up (the TGFS index cluster).
pub enum Source {
    Plgb,
    Tgfs { cluster_idx: u8 },
}

/// Builds an opaque, HMAC-signed token embedding `(source, cluster_idx, oid)`. Used
/// instead of a bare `/file/{source}/{objectid}` URL so an end user can neither tell
/// which project/database a search result came from, nor edit the id to probe for a
/// different file — any modification changes the signature, which `decode_token`
/// checks and rejects.
pub fn encode_token(secret: &[u8], source: Source, oid: ObjectId) -> String {
    let (source_byte, cluster_idx) = match source {
        Source::Plgb => (SOURCE_PLGB, 0),
        Source::Tgfs { cluster_idx } => (SOURCE_TGFS, cluster_idx),
    };

    let mut payload = Vec::with_capacity(14);
    payload.push(source_byte);
    payload.push(cluster_idx);
    payload.extend_from_slice(&oid.bytes());

    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&payload);
    payload.extend_from_slice(&mac.finalize().into_bytes());

    URL_SAFE_NO_PAD.encode(payload)
}

/// Verifies and decodes a token built by [`encode_token`]. Returns `None` if it's
/// malformed or fails signature verification (e.g. tampered with).
pub fn decode_token(secret: &[u8], token: &str) -> Option<(Source, ObjectId)> {
    let combined = URL_SAFE_NO_PAD.decode(token).ok()?;
    if combined.len() != 14 + 32 {
        return None;
    }
    let (payload, sig) = combined.split_at(14);

    let mut mac = Hmac::<Sha256>::new_from_slice(secret).ok()?;
    mac.update(payload);
    mac.verify_slice(sig).ok()?;

    let mut oid_bytes = [0u8; 12];
    oid_bytes.copy_from_slice(&payload[2..14]);
    let oid = ObjectId::from_bytes(oid_bytes);

    let source = match payload[0] {
        SOURCE_PLGB => Source::Plgb,
        SOURCE_TGFS => Source::Tgfs { cluster_idx: payload[1] },
        _ => return None,
    };
    Some((source, oid))
}
