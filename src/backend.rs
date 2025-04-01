use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::{io, thread};
use xxhash_rust::xxh3::Xxh3;

type SharedCopyProgress = Arc<Mutex<CopyProgress>>;

pub fn flatten_filetree(base_dir: &PathBuf, dir: &PathBuf) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in dir.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(flatten_filetree(base_dir, &path)?);
        } else {
            let relative_path = path.strip_prefix(base_dir).unwrap().to_path_buf();
            files.push(relative_path);
        }
    }
    Ok(files)
}

/// Copy a directory and its contents to another location.
///
/// The thread is guaranteed to finish when `CopyProgress` is set to `Error` or `Finished`.
pub fn copy_dir_threaded(
    from: &PathBuf,
    to: &PathBuf,
    reporter: SharedCopyProgress,
) -> io::Result<()> {
    let files = flatten_filetree(from, from)?;
    let from = from.clone();
    let to = to.clone();
    let todo: Vec<(PathBuf, PathBuf)> = files
        .iter()
        .map(|file| {
            let from = from.join(file);
            let to = to.join(file);
            (from, to)
        })
        .collect();
    let reporter = reporter.clone();
    thread::spawn(move || {
        if copy(&todo, &reporter) {
            return;
        }
        if checksum(&todo, &reporter) {
            return;
        }
    });
    Ok(())
}

type PathPairVec = Vec<(PathBuf, PathBuf)>;
fn copy(todo: &PathPairVec, reporter: &SharedCopyProgress) -> bool {
    {
        *(reporter.lock().unwrap()) = CopyProgress::Copy {
            total: todo.len(),
            copied: 0,
        };
    }
    for (full_from, full_to) in todo {
        if let Some(parent) = full_to.parent() {
            match std::fs::create_dir_all(parent) {
                Err(e) => {
                    *(reporter.lock().unwrap()) = CopyProgress::Error { error: e };
                    return true;
                }
                Ok(_) => {}
            }
        }
        match std::fs::copy(&full_from, &full_to) {
            Ok(_size) => {
                let mut progress = reporter.lock().unwrap();
                if let CopyProgress::Copy { total, copied } = *progress {
                    *progress = CopyProgress::Copy {
                        total,
                        copied: copied + 1,
                    };
                }
            }
            Err(e) => {
                *(reporter.lock().unwrap()) = CopyProgress::Error { error: e };
                return true;
            }
        }
    }
    false
}

fn checksum(todo: &PathPairVec, reporter: &SharedCopyProgress) -> bool {
    {
        *(reporter.lock().unwrap()) = CopyProgress::Checksum {
            total: todo.len(),
            completed: 0,
        };
    }
    let mut report = ChecksumReport(Vec::new());
    for (full_from, full_to) in todo {
        let source_hash = match xxh3_hash_file(full_from) {
            Ok(hash) => hash,
            Err(e) => {
                *(reporter.lock().unwrap()) = CopyProgress::Error { error: e };
                return true;
            }
        };
        let destination_hash = match xxh3_hash_file(full_to) {
            Ok(hash) => hash,
            Err(e) => {
                *(reporter.lock().unwrap()) = CopyProgress::Error { error: e };
                return true;
            }
        };
        report.0.push(ChecksumReportSingleFile {
            source: full_from.clone(),
            source_hash,
            destination: full_to.clone(),
            destination_hash,
        });
        let mut progress = reporter.lock().unwrap();
        if let CopyProgress::Checksum { total, completed } = *progress {
            *progress = CopyProgress::Checksum {
                total,
                completed: completed + 1,
            };
        }
    }
    {
        *(reporter.lock().unwrap()) = CopyProgress::Finished { report };
    }
    false
}

pub enum CopyProgress {
    Copy { total: usize, copied: usize },
    Checksum { total: usize, completed: usize },
    Error { error: io::Error },
    Finished { report: ChecksumReport },
}

pub struct ChecksumReport(Vec<ChecksumReportSingleFile>);

pub struct ChecksumReportSingleFile {
    pub source: PathBuf,
    pub source_hash: u64,
    pub destination: PathBuf,
    pub destination_hash: u64,
}

impl ChecksumReport {
    pub fn total_files(&self) -> usize {
        self.0.len()
    }

    pub fn count_errors(&self) -> usize {
        self.0
            .iter()
            .filter(|file| file.source_hash != file.destination_hash)
            .count()
    }
}

pub fn xxh3_hash_file<P: AsRef<Path>>(path: P) -> io::Result<u64> {
    // Open the file
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(1024 * 1024 * 8, file); // 8MB buffer

    // Create XXH3 hasher
    let mut hasher = Xxh3::new();

    // Process the file in chunks
    let mut buffer = [0u8; 1024 * 1024]; // 1MB buffer
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    // Get the final hash
    Ok(hasher.digest())
}
