use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::engine::RunSummary;
use crate::settings::Settings;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReportFormat {
    Csv,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReportExport {
    pub path: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReportItem {
    pub id: String,
    pub file: String,
    pub destination: Option<String>,
    pub reason: String,
    pub group: String,
    pub is_video: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportDocument {
    pub created_at: String,
    pub elapsed_seconds: f64,
    pub output_folder: String,
    pub settings: Settings,
    pub summary: RunSummary,
    pub items: Vec<ReportItem>,
}

impl ReportDocument {
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        let contents = match fs::read(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!("read report {}: {error}", path.display()));
            }
        };
        serde_json::from_slice(&contents)
            .map(Some)
            .map_err(|error| format!("parse report {}: {error}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| format!("report path has no parent: {}", path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("create report folder {}: {error}", parent.display()))?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".takeback-report-")
            .tempfile_in(parent)
            .map_err(|error| format!("create temporary report: {error}"))?;
        serde_json::to_writer(temporary.as_file_mut(), self)
            .map_err(|error| format!("serialize report: {error}"))?;
        temporary
            .persist(path)
            .map_err(|error| format!("save report {}: {}", path.display(), error.error))?;
        Ok(())
    }

    pub fn readable_summary(&self) -> String {
        format!(
            "Takeback run completed in {:.0} seconds.\n{} processed, {} skipped, {} failed.\n{} files without JSON, {} collisions, {} warnings.\nOutput: {}",
            self.elapsed_seconds,
            self.summary.processed,
            self.summary.skipped,
            self.summary.failed,
            self.summary.files_without_json,
            self.summary.collisions,
            self.summary.warnings.len(),
            self.output_folder,
        )
    }

    pub fn export(&self, destination: &Path, format: ReportFormat) -> Result<ReportExport, String> {
        let parent = destination
            .parent()
            .ok_or_else(|| format!("report path has no parent: {}", destination.display()))?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".takeback-report-")
            .tempfile_in(parent)
            .map_err(|error| format!("create temporary report: {error}"))?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            match format {
                ReportFormat::Json => serde_json::to_writer_pretty(&mut writer, self)
                    .map_err(|error| format!("write JSON report: {error}"))?,
                ReportFormat::Csv => self.write_csv(&mut writer)?,
            }
            writer
                .flush()
                .map_err(|error| format!("flush report: {error}"))?;
        }
        temporary.persist(destination).map_err(|error| {
            format!("save report to {}: {}", destination.display(), error.error)
        })?;
        Ok(ReportExport {
            path: destination.to_string_lossy().into_owned(),
            summary: self.readable_summary(),
        })
    }

    fn write_csv(&self, writer: &mut impl Write) -> Result<(), String> {
        writeln!(writer, "Takeback report")
            .and_then(|_| writeln!(writer, "Created,{}", csv_field(&self.created_at)))
            .and_then(|_| writeln!(writer, "Output folder,{}", csv_field(&self.output_folder)))
            .and_then(|_| writeln!(writer, "Processed,{}", self.summary.processed))
            .and_then(|_| writeln!(writer, "Skipped,{}", self.summary.skipped))
            .and_then(|_| writeln!(writer, "Failed,{}", self.summary.failed))
            .and_then(|_| {
                writeln!(
                    writer,
                    "Files without JSON,{}",
                    self.summary.files_without_json
                )
            })
            .and_then(|_| writeln!(writer, "Collisions,{}", self.summary.collisions))
            .and_then(|_| writeln!(writer, "Warnings,{}", self.summary.warnings.len()))
            .and_then(|_| writeln!(writer, ""))
            .and_then(|_| writeln!(writer, "File,Issue,Group,Destination"))
            .map_err(|error| format!("write CSV report: {error}"))?;
        for item in &self.items {
            writeln!(
                writer,
                "{},{},{},{}",
                csv_field(&item.file),
                csv_field(&item.reason),
                csv_field(&item.group),
                csv_field(item.destination.as_deref().unwrap_or(""))
            )
            .map_err(|error| format!("write CSV report item: {error}"))?;
        }
        for warning in &self.summary.warnings {
            writeln!(
                writer,
                "{},{},{},{}",
                csv_field(""),
                csv_field(warning),
                csv_field("warning"),
                csv_field("")
            )
            .map_err(|error| format!("write CSV warning: {error}"))?;
        }
        for problem in &self.summary.problems {
            writeln!(
                writer,
                "{},{},{},{}",
                csv_field(""),
                csv_field(problem),
                csv_field("failed"),
                csv_field("")
            )
            .map_err(|error| format!("write CSV failure: {error}"))?;
        }
        Ok(())
    }
}

fn csv_field(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
