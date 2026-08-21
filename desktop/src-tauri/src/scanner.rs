use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const HASH_BUFFER_SIZE: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ScanOptions {
    extensions: HashSet<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self::with_extensions(["mp4", "mov", "mkv", "avi", "mxf", "webm"])
    }
}

impl ScanOptions {
    pub fn with_extensions<I, S>(extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let extensions = extensions
            .into_iter()
            .map(|extension| {
                extension
                    .as_ref()
                    .trim_start_matches('.')
                    .to_ascii_lowercase()
            })
            .filter(|extension| !extension.is_empty())
            .collect();

        Self { extensions }
    }

    fn accepts(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| self.extensions.contains(&extension.to_ascii_lowercase()))
            .unwrap_or(false)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DiscoveredFile {
    pub path: String,
    pub size_bytes: u64,
    pub modified_unix_ms: Option<u64>,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScanWarning {
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScanReport {
    pub files: Vec<DiscoveredFile>,
    pub warnings: Vec<ScanWarning>,
}

#[derive(Debug)]
pub enum ScanError {
    RootUnavailable { path: PathBuf, message: String },
    RootNotDirectory { path: PathBuf },
}

impl Display for ScanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootUnavailable { path, message } => {
                write!(
                    formatter,
                    "cannot access scan root {}: {message}",
                    path.display()
                )
            }
            Self::RootNotDirectory { path } => {
                write!(
                    formatter,
                    "scan root is not a directory: {}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ScanError {}

#[derive(Clone, Debug, Deserialize)]
pub struct LocalFileRecord {
    pub path: String,
    pub size_bytes: u64,
    pub modified_unix_ms: Option<u64>,
    pub content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChangeKind {
    New,
    Unchanged,
    Modified,
    Moved,
    Deleted,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileChange {
    pub kind: ChangeKind,
    pub path: String,
    pub previous_path: Option<String>,
    pub content_hash: Option<String>,
}

pub fn scan_folder(root: &Path, options: &ScanOptions) -> Result<ScanReport, ScanError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| ScanError::RootUnavailable {
        path: root.to_path_buf(),
        message: error.to_string(),
    })?;

    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ScanError::RootNotDirectory {
            path: root.to_path_buf(),
        });
    }

    let mut files = Vec::new();
    let mut warnings = Vec::new();
    visit_directory(root, options, &mut files, &mut warnings);
    files.sort_by(|left, right| path_key(&left.path).cmp(&path_key(&right.path)));

    Ok(ScanReport { files, warnings })
}

pub fn detect_changes(previous: &[LocalFileRecord], current: &[DiscoveredFile]) -> Vec<FileChange> {
    let mut previous_by_path = HashMap::new();
    let mut previous_by_hash: HashMap<&str, Vec<usize>> = HashMap::new();

    for (index, file) in previous.iter().enumerate() {
        previous_by_path.insert(path_key(&file.path), index);
        previous_by_hash
            .entry(file.content_hash.as_str())
            .or_default()
            .push(index);
    }

    let mut matched_previous = HashSet::new();
    let mut changes = Vec::with_capacity(previous.len().max(current.len()));

    for file in current {
        let current_key = path_key(&file.path);

        if let Some(previous_index) = previous_by_path.get(&current_key).copied() {
            let previous_file = &previous[previous_index];
            matched_previous.insert(previous_index);
            let kind = if previous_file.content_hash == file.content_hash {
                ChangeKind::Unchanged
            } else {
                ChangeKind::Modified
            };

            changes.push(FileChange {
                kind,
                path: file.path.clone(),
                previous_path: None,
                content_hash: Some(file.content_hash.clone()),
            });
            continue;
        }

        let moved_from = previous_by_hash
            .get(file.content_hash.as_str())
            .and_then(|candidates| {
                candidates
                    .iter()
                    .copied()
                    .find(|index| !matched_previous.contains(index))
            });

        if let Some(previous_index) = moved_from {
            matched_previous.insert(previous_index);
            changes.push(FileChange {
                kind: ChangeKind::Moved,
                path: file.path.clone(),
                previous_path: Some(previous[previous_index].path.clone()),
                content_hash: Some(file.content_hash.clone()),
            });
        } else {
            changes.push(FileChange {
                kind: ChangeKind::New,
                path: file.path.clone(),
                previous_path: None,
                content_hash: Some(file.content_hash.clone()),
            });
        }
    }

    let mut deleted: Vec<_> = previous
        .iter()
        .enumerate()
        .filter(|(index, _)| !matched_previous.contains(index))
        .collect();
    deleted.sort_by(|(_, left), (_, right)| path_key(&left.path).cmp(&path_key(&right.path)));

    for (_, file) in deleted {
        changes.push(FileChange {
            kind: ChangeKind::Deleted,
            path: file.path.clone(),
            previous_path: None,
            content_hash: Some(file.content_hash.clone()),
        });
    }

    changes
}

fn visit_directory(
    directory: &Path,
    options: &ScanOptions,
    files: &mut Vec<DiscoveredFile>,
    warnings: &mut Vec<ScanWarning>,
) {
    let mut entries: Vec<_> = match fs::read_dir(directory) {
        Ok(entries) => entries.filter_map(Result::ok).collect(),
        Err(error) => {
            warnings.push(ScanWarning {
                path: display_path(directory),
                message: format!("cannot read directory: {error}"),
            });
            return;
        }
    };

    entries.sort_by(|left, right| {
        path_key(&display_path(&left.path())).cmp(&path_key(&display_path(&right.path())))
    });

    for entry in entries {
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                warnings.push(ScanWarning {
                    path: display_path(&path),
                    message: format!("cannot inspect entry: {error}"),
                });
                continue;
            }
        };

        if metadata.file_type().is_symlink() {
            continue;
        }

        if metadata.is_dir() {
            visit_directory(&path, options, files, warnings);
            continue;
        }

        if !metadata.is_file() || !options.accepts(&path) {
            continue;
        }

        let content_hash = match hash_file(&path) {
            Ok(hash) => hash,
            Err(error) => {
                warnings.push(ScanWarning {
                    path: display_path(&path),
                    message: format!("cannot hash file: {error}"),
                });
                continue;
            }
        };

        files.push(DiscoveredFile {
            path: display_path(&path),
            size_bytes: metadata.len(),
            modified_unix_ms: modified_unix_ms(&metadata),
            content_hash,
        });
    }
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; HASH_BUFFER_SIZE];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn modified_unix_ms(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn path_key(path: &str) -> String {
    let normalized = path.replace('\\', "/");

    #[cfg(windows)]
    {
        normalized.to_ascii_lowercase()
    }

    #[cfg(not(windows))]
    {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, remove_dir_all, write};
    use std::time::SystemTime;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "mediaindex-{name}-{}-{timestamp}",
                std::process::id()
            ));
            create_dir_all(&path).expect("test directory should be created");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = remove_dir_all(&self.0);
        }
    }

    fn record(path: &str, hash: &str) -> LocalFileRecord {
        LocalFileRecord {
            path: path.to_owned(),
            size_bytes: hash.len() as u64,
            modified_unix_ms: None,
            content_hash: hash.to_owned(),
        }
    }

    fn discovered(path: &str, hash: &str) -> DiscoveredFile {
        DiscoveredFile {
            path: path.to_owned(),
            size_bytes: hash.len() as u64,
            modified_unix_ms: None,
            content_hash: hash.to_owned(),
        }
    }

    #[test]
    fn scans_supported_files_deterministically_and_hashes_content() {
        let directory = TestDirectory::new("scan");
        write(directory.0.join("b.MP4"), b"second").expect("fixture should be written");
        write(directory.0.join("a.mp4"), b"abc").expect("fixture should be written");
        write(directory.0.join("notes.txt"), b"not media").expect("fixture should be written");
        create_dir_all(directory.0.join("nested")).expect("nested directory should be created");
        write(directory.0.join("nested/c.mov"), b"third").expect("fixture should be written");

        let report =
            scan_folder(&directory.0, &ScanOptions::default()).expect("scan should succeed");
        let names: Vec<_> = report
            .files
            .iter()
            .map(|file| {
                Path::new(&file.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        assert_eq!(names, vec!["a.mp4", "b.MP4", "c.mov"]);
        assert_eq!(
            report.files[0].content_hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn rejects_a_missing_scan_root() {
        let directory = std::env::temp_dir().join("mediaindex-definitely-missing");
        let error =
            scan_folder(&directory, &ScanOptions::default()).expect_err("missing root should fail");
        assert!(matches!(error, ScanError::RootUnavailable { .. }));
    }

    #[test]
    fn detects_unchanged_moved_and_modified_files() {
        let previous = vec![
            record("/footage/a.mp4", "hash-a"),
            record("/footage/b.mp4", "hash-b"),
        ];
        let current = vec![
            discovered("/footage/a.mp4", "hash-a"),
            discovered("/archive/b.mp4", "hash-b"),
        ];

        let changes = detect_changes(&previous, &current);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].kind, ChangeKind::Unchanged);
        assert_eq!(changes[1].kind, ChangeKind::Moved);
        assert_eq!(changes[1].previous_path.as_deref(), Some("/footage/b.mp4"));

        let modified = detect_changes(&previous, &[discovered("/footage/a.mp4", "hash-new")]);
        assert_eq!(modified[0].kind, ChangeKind::Modified);
        assert_eq!(modified[1].kind, ChangeKind::Deleted);
    }

    #[test]
    fn reports_new_files_and_normalizes_extensions() {
        let previous = vec![record("/footage/a.mp4", "hash-a")];
        let current = vec![
            discovered("/footage/a.mp4", "hash-a"),
            discovered("/footage/new.MOV", "hash-new"),
        ];
        let changes = detect_changes(&previous, &current);
        assert_eq!(changes[1].kind, ChangeKind::New);

        let options = ScanOptions::with_extensions([".MP4", "mov"]);
        assert!(options.accepts(Path::new("clip.Mp4")));
        assert!(!options.accepts(Path::new("clip.mkv")));
    }
}
