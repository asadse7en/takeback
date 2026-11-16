use std::fs;
use std::path::{Path, PathBuf};

use sysinfo::Disks;

pub fn sanitize_file_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        let forbidden: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
        if forbidden.contains(&ch) {
            continue;
        }
        out.push(ch);
    }
    out
}

pub fn disk_free_bytes(path: &Path) -> Result<u64, String> {
    let to_check = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };

    let canonical = std::fs::canonicalize(&to_check).unwrap_or(to_check);
    let disks = Disks::new_with_refreshed_list();
    let available = if let Some(disk) = disks
        .list()
        .iter()
        .filter(|disk| canonical.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().components().count())
    {
        disk.available_space()
    } else {
        return Err(format!(
            "unable to determine free space for {}",
            canonical.display()
        ));
    };

    if available == 0 {
        return Err(format!(
            "unable to determine free space for {}",
            canonical.display()
        ));
    }

    Ok(available)
}

pub fn hard_link_or_copy(src: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("create directory {}: {e}", parent.display()))?;
        }
    }

    match fs::hard_link(src, dest) {
        Ok(()) => Ok(()),
        Err(err) => {
            fs::copy(src, dest).map_err(|copy_err| {
                format!(
                    "hard link {} -> {} failed: {err}; fallback copy failed: {copy_err}",
                    src.display(),
                    dest.display()
                )
            })?;
            Ok(())
        }
    }
}

pub fn exiftool_path(resource_dir: &Path) -> Result<PathBuf, String> {
    let root = resource_dir.join("binaries").join("exiftool");
    let executable_name = if cfg!(windows) {
        "exiftool.exe"
    } else {
        "exiftool"
    };
    let mut pending = vec![root.clone()];
    let mut candidates = Vec::new();

    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!(
                "read bundled ExifTool directory {}: {error}",
                directory.display()
            )
        })?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("read bundled ExifTool resource: {error}"))?;
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if entry.file_name() == executable_name {
                candidates.push(path);
            }
        }
    }

    match candidates.len() {
        0 => Err(format!(
            "bundled ExifTool was not found under {}",
            root.display()
        )),
        1 => Ok(candidates.remove(0)),
        _ => {
            let arch = std::env::consts::ARCH;
            candidates
                .iter()
                .find(|path| path.to_string_lossy().contains(arch))
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "multiple ExifTool distributions were found under {}; none matched {}",
                        root.display(),
                        arch
                    )
                })
        }
    }
}
