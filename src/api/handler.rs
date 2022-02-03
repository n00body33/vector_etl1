use std::sync::{atomic::AtomicBool, Arc};

use serde_json::json;
use warp::{reply::json, Rejection, Reply};

// Health handler, responds with { ok: true }
pub async fn health(running: Arc<AtomicBool>) -> Result<impl Reply, Rejection> {
    if running.load(std::sync::atomic::Ordering::Relaxed) {
        Ok(warp::reply::with_status(
            json(&json!({"ok": true})),
            warp::http::StatusCode::OK,
        ))
    } else {
        Ok(warp::reply::with_status(
            json(&json!({"ok": false})),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        ))
    }
}
