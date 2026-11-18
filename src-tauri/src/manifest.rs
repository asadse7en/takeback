use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use rusqlite::{params, Connection};

#[derive(Debug, Clone)]
pub struct ManifestRecord {
    pub source_key: String,
    pub placement_key: String,
    pub destination: String,
    pub status: String,
    pub destination_size: Option<u64>,
}

pub struct RunManifest {
    connection: Connection,
    output_root_key: String,
    records: HashMap<(String, String), ManifestRecord>,
}

impl RunManifest {
    pub fn open(path: &Path, output_root: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create manifest folder {}: {error}", parent.display()))?;
        }
        let connection = Connection::open(path)
            .map_err(|error| format!("open run manifest {}: {error}", path.display()))?;
        connection
            .busy_timeout(std::time::Duration::from_secs(2))
            .map_err(|error| format!("configure run manifest: {error}"))?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS placements (
                    output_root TEXT NOT NULL,
                    source_key TEXT NOT NULL,
                    placement_key TEXT NOT NULL,
                    destination TEXT NOT NULL,
                    status TEXT NOT NULL CHECK (status IN ('pending', 'done')),
                    destination_size INTEGER,
                    PRIMARY KEY (output_root, source_key, placement_key)
                );
                CREATE INDEX IF NOT EXISTS placements_by_root_source
                    ON placements (output_root, source_key);",
            )
            .map_err(|error| format!("create run manifest tables: {error}"))?;

        let output_root_key = normalize_output_root(output_root);
        let records = {
            let mut statement = connection
                .prepare(
                    "SELECT source_key, placement_key, destination, status, destination_size
                     FROM placements WHERE output_root = ?1",
                )
                .map_err(|error| format!("read run manifest: {error}"))?;
            let rows = statement
                .query_map([&output_root_key], |row| {
                    let size = row.get::<_, Option<i64>>(4)?;
                    Ok(ManifestRecord {
                        source_key: row.get(0)?,
                        placement_key: row.get(1)?,
                        destination: row.get(2)?,
                        status: row.get(3)?,
                        destination_size: size
                            .map(|size| {
                                u64::try_from(size)
                                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(4, size))
                            })
                            .transpose()?,
                    })
                })
                .map_err(|error| format!("query run manifest: {error}"))?;
            let mut records = HashMap::new();
            for record in rows {
                let record = record.map_err(|error| format!("read run manifest row: {error}"))?;
                records.insert(
                    (record.source_key.clone(), record.placement_key.clone()),
                    record,
                );
            }
            records
        };

        Ok(Self {
            connection,
            output_root_key,
            records,
        })
    }

    pub fn planner_destinations(
        &self,
        source_keys: &HashSet<String>,
    ) -> Result<HashMap<String, String>, String> {
        let mut destinations = HashMap::new();
        for record in self.records.values() {
            if !source_keys.contains(&record.source_key) {
                continue;
            }
            let path = Path::new(&record.destination);
            let can_reuse = match record.status.as_str() {
                "done" => match fs::symlink_metadata(path) {
                    Ok(metadata) => Some(
                        metadata.file_type().is_file()
                            && record.destination_size == Some(metadata.len()),
                    ),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(true),
                    Err(error) => {
                        return Err(format!(
                            "inspect manifest destination {}: {error}",
                            path.display()
                        ));
                    }
                },
                "pending" => match fs::symlink_metadata(path) {
                    Ok(_) => Some(false),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(true),
                    Err(error) => {
                        return Err(format!(
                            "inspect manifest destination {}: {error}",
                            path.display()
                        ));
                    }
                },
                _ => None,
            };
            if can_reuse == Some(true) {
                destinations.insert(
                    lookup_key(&record.source_key, &record.placement_key),
                    record.destination.clone(),
                );
            }
        }
        Ok(destinations)
    }

    pub fn is_done_at_destination(
        &self,
        source_key: &str,
        placement_key: &str,
        destination: &Path,
    ) -> Result<bool, String> {
        let Some(record) = self
            .records
            .get(&(source_key.to_string(), placement_key.to_string()))
        else {
            return Ok(false);
        };
        if record.status != "done"
            || normalize_destination(&record.destination)
                != normalize_destination(&destination.to_string_lossy())
        {
            return Ok(false);
        }
        match fs::symlink_metadata(destination) {
            Ok(metadata) => {
                Ok(metadata.file_type().is_file()
                    && record.destination_size == Some(metadata.len()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!(
                "inspect manifest destination {}: {error}",
                destination.display()
            )),
        }
    }

    pub fn mark_pending(
        &mut self,
        source_key: &str,
        placement_key: &str,
        destination: &str,
    ) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO placements
                    (output_root, source_key, placement_key, destination, status, destination_size)
                 VALUES (?1, ?2, ?3, ?4, 'pending', NULL)
                 ON CONFLICT(output_root, source_key, placement_key) DO UPDATE SET
                    destination = excluded.destination,
                    status = 'pending',
                    destination_size = NULL",
                params![self.output_root_key, source_key, placement_key, destination],
            )
            .map_err(|error| format!("record pending output in manifest: {error}"))?;
        self.records.insert(
            (source_key.to_string(), placement_key.to_string()),
            ManifestRecord {
                source_key: source_key.to_string(),
                placement_key: placement_key.to_string(),
                destination: destination.to_string(),
                status: "pending".to_string(),
                destination_size: None,
            },
        );
        Ok(())
    }

    pub fn mark_done(
        &mut self,
        source_key: &str,
        placement_key: &str,
        destination: &Path,
    ) -> Result<(), String> {
        let size = fs::metadata(destination)
            .map_err(|error| {
                format!(
                    "inspect completed output {}: {error}",
                    destination.display()
                )
            })?
            .len();
        let size_i64 = i64::try_from(size)
            .map_err(|_| format!("output is too large to record: {}", destination.display()))?;
        let destination_text = destination.to_string_lossy().into_owned();
        self.connection
            .execute(
                "INSERT INTO placements
                    (output_root, source_key, placement_key, destination, status, destination_size)
                 VALUES (?1, ?2, ?3, ?4, 'done', ?5)
                 ON CONFLICT(output_root, source_key, placement_key) DO UPDATE SET
                    destination = excluded.destination,
                    status = 'done',
                    destination_size = excluded.destination_size",
                params![
                    self.output_root_key,
                    source_key,
                    placement_key,
                    destination_text,
                    size_i64
                ],
            )
            .map_err(|error| format!("record completed output in manifest: {error}"))?;
        self.records.insert(
            (source_key.to_string(), placement_key.to_string()),
            ManifestRecord {
                source_key: source_key.to_string(),
                placement_key: placement_key.to_string(),
                destination: destination_text,
                status: "done".to_string(),
                destination_size: Some(size),
            },
        );
        Ok(())
    }
}

pub fn source_key(source: &str, size: u64) -> String {
    format!("{}::size={size}", source.replace('\\', "/").to_lowercase())
}

pub fn placement_key(
    kind: &str,
    tags: &[String],
    destination_directory: &Path,
    output_root: &Path,
) -> String {
    let normalized_root = normalize_destination(&output_root.to_string_lossy());
    let normalized_directory = normalize_destination(&destination_directory.to_string_lossy());
    let relative_directory = if normalized_directory == normalized_root {
        ""
    } else {
        normalized_directory
            .strip_prefix(&format!("{normalized_root}/"))
            .unwrap_or(&normalized_directory)
    };
    let role_tag = tags
        .iter()
        .filter(|tag| tag.as_str() == "Link" || tag.as_str() == "Copy")
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{}|{}|{}",
        kind.to_ascii_lowercase(),
        role_tag.to_lowercase(),
        relative_directory
    )
}

pub fn lookup_key(source_key: &str, placement_key: &str) -> String {
    format!("{source_key}\u{1f}{placement_key}")
}

fn normalize_output_root(path: &Path) -> String {
    let absolute = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    normalize_destination(&absolute.to_string_lossy())
}

fn normalize_destination(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn manifest_reuses_done_destinations_only_when_sizes_match() {
        let directory = tempdir().expect("create manifest directory");
        let database = directory.path().join("manifest.sqlite");
        let output_root = directory.path().join("library");
        fs::create_dir(&output_root).expect("create output root");
        let destination = output_root.join("photo.jpg");
        fs::write(&destination, b"saved").expect("write destination");
        let mut manifest = RunManifest::open(&database, &output_root).expect("open manifest");
        manifest
            .mark_pending("source::size=4", "original", &destination.to_string_lossy())
            .expect("record pending output");
        manifest
            .mark_done("source::size=4", "original", &destination)
            .expect("mark output done");

        assert!(manifest
            .is_done_at_destination("source::size=4", "original", &destination,)
            .expect("verify done output"));
        assert!(!manifest
            .is_done_at_destination("source::size=5", "original", &destination,)
            .expect("reject changed source key"));

        fs::write(&destination, b"changed size").expect("change destination size");
        assert!(!manifest
            .is_done_at_destination("source::size=4", "original", &destination,)
            .expect("reject changed destination size"));
        assert!(manifest
            .planner_destinations(&HashSet::from(["source::size=4".to_string()]))
            .expect("read planner destinations")
            .is_empty());
    }
}
