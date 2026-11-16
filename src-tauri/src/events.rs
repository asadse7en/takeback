use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, Result};

pub const PROCESSING_PROGRESS_EVENT: &str = "processing:progress";

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingProgressEvent {
    pub current_file: Option<String>,
    pub completed_files: u64,
    pub total_files: u64,
    pub processed_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_second: f64,
    pub eta_seconds: Option<u64>,
    pub warnings_count: u64,
    pub warnings: Vec<String>,
}

pub fn emit_processing_progress(app: &AppHandle, progress: &ProcessingProgressEvent) -> Result<()> {
    app.emit(PROCESSING_PROGRESS_EVENT, progress)
        .map_err(|error| AppError::Other(format!("Could not send processing progress: {error}")))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgressEvent {
    pub processed: u32,
    pub total: u32,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunProgressEvent {
    pub file: String,
    pub completed: u32,
    pub total: u32,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunFinishedEvent {
    pub ok: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processing_progress_uses_camel_case_payload_fields() {
        let value = serde_json::to_value(ProcessingProgressEvent::default())
            .expect("serialize progress event");

        assert!(value.get("currentFile").is_some());
        assert!(value.get("completedFiles").is_some());
        assert!(value.get("processedBytes").is_some());
        assert!(value.get("bytesPerSecond").is_some());
        assert!(value.get("etaSeconds").is_some());
        assert!(value.get("warningsCount").is_some());
    }
}
