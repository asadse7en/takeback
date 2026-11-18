use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sysinfo::Disks;
use walkdir::WalkDir;

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
    disk_space_bytes(path).map(|(available, _)| available)
}

pub fn disk_space_bytes(path: &Path) -> Result<(u64, u64), String> {
    let mut to_check = path;
    while !to_check.exists() {
        to_check = to_check
            .parent()
            .ok_or_else(|| format!("unable to resolve output drive for {}", path.display()))?;
    }

    let canonical = std::fs::canonicalize(to_check).unwrap_or_else(|_| to_check.to_path_buf());
    let disk_path = path_for_disk_matching(&canonical);
    let disks = Disks::new_with_refreshed_list();
    let (available, total) = if let Some(disk) = disks
        .list()
        .iter()
        .filter(|disk| mount_matches_path(&disk_path, disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().components().count())
    {
        (disk.available_space(), disk.total_space())
    } else {
        return Err(format!(
            "unable to determine free space for {}",
            canonical.display()
        ));
    };

    Ok((available, total))
}

fn mount_matches_path(path: &Path, mount_point: &Path) -> bool {
    let path = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase();
    let mount = mount_point
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase();

    if mount.is_empty() {
        return path.starts_with('/');
    }
    path == mount
        || path
            .strip_prefix(&mount)
            .is_some_and(|remainder| remainder.starts_with('/'))
}

#[cfg(windows)]
fn path_for_disk_matching(path: &Path) -> PathBuf {
    let path_text = path.to_string_lossy();
    if let Some(unc_path) = path_text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc_path}"));
    }
    if let Some(path) = path_text.strip_prefix(r"\\?\") {
        return PathBuf::from(path);
    }
    path.to_path_buf()
}

#[cfg(not(windows))]
fn path_for_disk_matching(path: &Path) -> PathBuf {
    path.to_path_buf()
}

pub fn cleanup_takeback_temporary_files(output_root: &Path) -> Result<u64, String> {
    if !output_root.exists() {
        return Ok(0);
    }

    let mut removed = 0_u64;
    for entry in WalkDir::new(output_root).follow_links(false) {
        let entry = entry.map_err(|error| {
            format!(
                "scan output folder for unfinished Takeback files {}: {error}",
                output_root.display()
            )
        })?;
        if entry.file_type().is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with(".takeback-")
        {
            fs::remove_file(entry.path()).map_err(|error| {
                format!(
                    "remove unfinished Takeback file {}: {error}",
                    entry.path().display()
                )
            })?;
            removed = removed.saturating_add(1);
        }
    }
    Ok(removed)
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
            let mut source = fs::File::open(src).map_err(|copy_error| {
                format!(
                    "hard link {} -> {} failed: {err}; fallback copy could not open source: {copy_error}",
                    src.display(),
                    dest.display()
                )
            })?;
            let mut destination = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(dest)
                .map_err(|copy_error| {
                    format!(
                        "hard link {} -> {} failed: {err}; fallback copy could not create destination without replacing it: {copy_error}",
                        src.display(),
                        dest.display()
                    )
                })?;
            if let Err(copy_error) =
                io::copy(&mut source, &mut destination).and_then(|_| destination.flush())
            {
                drop(destination);
                let cleanup_error = fs::remove_file(dest).err();
                return Err(match cleanup_error {
                    Some(cleanup_error) => format!(
                        "hard link {} -> {} failed: {err}; fallback copy failed: {copy_error}; partial output cleanup failed: {cleanup_error}",
                        src.display(),
                        dest.display()
                    ),
                    None => format!(
                        "hard link {} -> {} failed: {err}; fallback copy failed: {copy_error}",
                        src.display(),
                        dest.display()
                    ),
                });
            }
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
    fn mount_matching_is_case_insensitive_and_respects_component_boundaries() {
        assert!(mount_matches_path(
            Path::new("C:/Photo Library/June"),
            Path::new("c:/photo library")
        ));
        assert!(!mount_matches_path(
            Path::new("C:/Photo Library2"),
            Path::new("c:/photo library")
        ));
    }

    #[cfg(windows)]
    #[test]
    fn disk_matching_removes_windows_extended_path_prefixes() {
        assert_eq!(
            path_for_disk_matching(Path::new(r"\\?\C:\Photos")),
            PathBuf::from(r"C:\Photos")
        );
        assert_eq!(
            path_for_disk_matching(Path::new(r"\\?\UNC\server\share\Photos")),
            PathBuf::from(r"\\server\share\Photos")
        );
    }

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

    #[test]
    fn cleanup_removes_only_takeback_temporary_files_recursively() {
        let temp = tempdir().expect("create output directory");
        let nested = temp.path().join("2025/06");
        fs::create_dir_all(&nested).expect("create nested output directory");
        let temporary = nested.join(".takeback-abc.jpg");
        let ordinary = nested.join("photo.jpg");
        fs::write(&temporary, b"partial").expect("write temporary file");
        fs::write(&ordinary, b"complete").expect("write ordinary file");

        assert_eq!(
            cleanup_takeback_temporary_files(temp.path()).expect("clean temporary outputs"),
            1
        );
        assert!(!temporary.exists());
        assert!(ordinary.exists());
    }

    #[test]
    fn hard_link_or_copy_never_replaces_an_existing_destination() {
        let temp = tempdir().expect("create output directory");
        let source = temp.path().join("source.jpg");
        let destination = temp.path().join("existing.jpg");
        fs::write(&source, b"source contents").expect("write source");
        fs::write(&destination, b"existing contents").expect("write destination");

        assert!(hard_link_or_copy(&source, &destination).is_err());
        assert_eq!(
            fs::read(&destination).expect("read existing destination"),
            b"existing contents"
        );
    }
}
