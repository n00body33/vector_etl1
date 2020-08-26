use super::InternalEvent;
use metrics::counter;
use std::path::PathBuf;

#[derive(Debug)]
pub struct VectorStarted;

impl InternalEvent for VectorStarted {
    fn emit_logs(&self) {
        info!(
            target: "vector",
            message = "Vector has started.",
            version = built_info::PKG_VERSION,
            git_version = built_info::GIT_VERSION.unwrap_or(""),
            released = built_info::BUILT_TIME_UTC,
            arch = built_info::CFG_TARGET_ARCH
        );
    }

    fn emit_metrics(&self) {
        counter!("vector_started_total", 1);
    }
}

#[derive(Debug)]
pub struct VectorReloaded<'a> {
    pub config_paths: &'a [PathBuf],
}

impl InternalEvent for VectorReloaded<'_> {
    fn emit_logs(&self) {
        info!(
            target: "vector",
            message = "Vector has reloaded.",
            path = ?self.config_paths
        );
    }

    fn emit_metrics(&self) {
        counter!("vector_reloaded_total", 1);
    }
}

#[derive(Debug)]
pub struct VectorStopped;

impl InternalEvent for VectorStopped {
    fn emit_logs(&self) {
        info!(
            target: "vector",
            message = "Vector has stopped."
        );
    }

    fn emit_metrics(&self) {
        counter!("vector_stopped_total", 1);
    }
}

#[derive(Debug)]
pub struct VectorQuited;

impl InternalEvent for VectorQuited {
    fn emit_logs(&self) {
        info!(
            target: "vector",
            message = "Vector has quited."
        );
    }

    fn emit_metrics(&self) {
        counter!("vector_quited_total", 1);
    }
}

#[derive(Debug)]
pub struct VectorReloadFailed;

impl InternalEvent for VectorReloadFailed {
    fn emit_logs(&self) {
        error!(
            target: "vector",
            message = "Reload was not successful."
        );
    }

    fn emit_metrics(&self) {
        counter!("vector_reload_errors", 1);
    }
}

#[derive(Debug)]
pub struct VectorConfigLoadFailed;

impl InternalEvent for VectorConfigLoadFailed {
    fn emit_logs(&self) {
        error!(
            target: "vector",
            message = "Failed to load config files, reload aborted."
        );
    }

    fn emit_metrics(&self) {
        counter!("config_load_errors", 1);
    }
}

#[derive(Debug)]
pub struct VectorRecoveryFailed;

impl InternalEvent for VectorRecoveryFailed {
    fn emit_logs(&self) {
        error!(
            target: "vector",
            message = "Vector has failed to recover from a failed reload."
        );
    }

    fn emit_metrics(&self) {
        counter!("vector_recover_errors", 1);
    }
}

#[allow(unused)]
mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}
