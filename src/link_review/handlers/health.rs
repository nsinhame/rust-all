//! Liveness probe for uptime pingers (e.g. Koyeb free-tier keepalive) — no access key required.

pub async fn health() -> &'static str {
    "ok"
}
