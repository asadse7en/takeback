use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use sysinfo::Disks;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkMethod {
    HardLink,
    Copy,
}

pub fn resolve_output_root(
    selected_folder: Option<&Path>,
    pictures_folder: &Path,
) -> Result<PathBuf, String> {
    let Some(selected_folder) = selected_folder.filter(|path| !path.as_os_str().is_empty()) else {
        return Ok(pictures_folder.join("Photo Library"));
    };

    if !selected_folder.exists() {
        return Ok(selected_folder.to_path_buf());
    }
    if !selected_folder.is_dir() {
        return Err(format!(
            "The selected output path is not a folder: {}",
            selected_folder.display()
        ));
    }

    let mut entries = fs::read_dir(selected_folder).map_err(|error| {
        format!(
            "Could not check the selected output folder {}: {error}",
            selected_folder.display()
        )
    })?;
    if let Some(entry) = entries.next() {
        entry.map_err(|error| {
            format!(
                "Could not read the selected output folder {}: {error}",
                selected_folder.display()
            )
        })?;
        Ok(selected_folder.join("Photo Library"))
    } else {
        Ok(selected_folder.to_path_buf())
    }
}

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
    let mut to_check = path;
    while !to_check.exists() {
        to_check = to_check
            .parent()
            .ok_or_else(|| format!("unable to resolve output drive for {}", path.display()))?;
    }

    let canonical = std::fs::canonicalize(to_check).unwrap_or_else(|_| to_check.to_path_buf());
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

pub fn existing_paths_in_directories(
    directories: &HashSet<PathBuf>,
) -> Result<HashSet<String>, String> {
    let mut paths = HashSet::new();
    for directory in directories {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(directory).map_err(|error| {
            format!(
                "Could not inspect existing output folder {}: {error}",
                directory.display()
            )
        })? {
            let entry = entry.map_err(|error| {
                format!(
                    "Could not read existing output folder {}: {error}",
                    directory.display()
                )
            })?;
            paths.insert(
                entry
                    .path()
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_lowercase(),
            );
        }
    }
    Ok(paths)
}

pub fn hard_link_or_copy(src: &Path, dest: &Path) -> Result<LinkMethod, String> {
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("create directory {}: {e}", parent.display()))?;
        }
    }

    match fs::hard_link(src, dest) {
        Ok(()) => Ok(LinkMethod::HardLink),
        Err(err) => {
            fs::copy(src, dest).map_err(|copy_err| {
                format!(
                    "hard link {} -> {} failed: {err}; fallback copy failed: {copy_err}",
                    src.display(),
                    dest.display()
                )
            })?;
            Ok(LinkMethod::Copy)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn output_root_uses_new_or_empty_folder_directly() {
        let temp = tempdir().expect("create test directory");
        let pictures = temp.path().join("Pictures");
        let new_folder = temp.path().join("new-library");
        let empty_folder = temp.path().join("empty-library");
        fs::create_dir(&empty_folder).expect("create empty folder");

        assert_eq!(
            resolve_output_root(None, &pictures).expect("default output root"),
            pictures.join("Photo Library")
        );
        assert_eq!(
            resolve_output_root(Some(&new_folder), &pictures).expect("new output root"),
            new_folder
        );
        assert_eq!(
            resolve_output_root(Some(&empty_folder), &pictures).expect("empty output root"),
            empty_folder
        );
    }

    #[test]
    fn nonempty_selected_folder_gets_library_child() {
        let temp = tempdir().expect("create test directory");
        let selected = temp.path().join("selected");
        fs::create_dir(&selected).expect("create selected folder");
        fs::write(selected.join("existing.txt"), "existing").expect("write existing file");

        assert_eq!(
            resolve_output_root(Some(&selected), temp.path()).expect("resolve output root"),
            selected.join("Photo Library")
        );
    }
}
