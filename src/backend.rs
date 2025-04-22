use futures::future::join_all;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;
use tokio::{join, spawn};
use twox_hash::XxHash3_64;

pub type SizeResult = io::Result<u64>;

fn collect_results<T, E>(vec: Vec<Result<T, E>>) -> Result<Vec<T>, E> {
    vec.into_iter().collect()
}

pub fn flatten_dir_files_recur(base_dir: &Path, dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(flatten_dir_files_recur(base_dir, &path)?);
        } else {
            let relative_path = path.strip_prefix(base_dir).unwrap().to_path_buf();
            files.push(relative_path);
        }
    }
    Ok(files)
}

pub fn flatten_dir_files(base_dir: &Path) -> io::Result<Vec<PathBuf>> {
    flatten_dir_files_recur(base_dir, base_dir)
}

#[derive(Clone, Copy, Debug)]
pub struct Progress {
    pub total: usize,
    pub completed: usize,
}

impl Progress {
    pub fn mut_increment(&mut self) {
        self.completed += 1;
    }
}

impl Default for Progress {
    fn default() -> Self {
        Progress {
            total: 0,
            completed: 0,
        }
    }
}

pub async fn read_file_copy_batch<P: AsRef<Path>>(
    source_path: P,
    dest_paths: Vec<PathBuf>,
) -> SizeResult {
    // Open the source file
    let mut source_file = File::open(&source_path).await?;

    // Open all destination files
    let mut dest_files = Vec::with_capacity(dest_paths.len());
    for path in dest_paths {
        dest_files.push(File::create(&path).await?);
    }

    let mut total_bytes = 0;

    // Rotated buffers for concurrent read/write
    const BUFFER_SIZE: usize = 1024 * 1024; // 1MB
    let mut buffer1 = vec![0u8; BUFFER_SIZE];
    let mut buffer2 = vec![0u8; BUFFER_SIZE];
    let mut read_buffer = &mut buffer1;
    let mut write_buffer = &mut buffer2;

    // Read first chunk into write_buffer
    let mut bytes_read = source_file.read(read_buffer).await?;
    if bytes_read == 0 {
        return Ok(0); // Edge case: empty file
    }

    loop {
        // Data from read_buffer from the last loop goes to write_buffer, and write_buffer from the last loop
        // is overwritten
        std::mem::swap(&mut read_buffer, &mut write_buffer);

        let mut write_futures = Vec::with_capacity(dest_files.len());
        for file in &mut dest_files {
            write_futures.push(file.write_all(&write_buffer[..bytes_read]));
        }
        let write_futures = join_all(write_futures);

        let read_future = source_file.read(read_buffer);

        // Execute read and write futures concurrently
        let (read_result, write_results) = join!(read_future, write_futures);

        // Check for write errors
        for result in write_results {
            result?;
        }

        bytes_read = read_result?; // Might not be BUFFER_SIZE if the upcoming read will hit EOF
        if bytes_read == 0 {
            break; // EOF
        }
        total_bytes += bytes_read as u64;
    }

    // Flush all destination files
    for file in &mut dest_files {
        file.flush().await?;
    }

    Ok(total_bytes)
}

pub async fn copy_dirs(
    source: &PathBuf,
    dest: &Vec<PathBuf>,
    tx: watch::Sender<Progress>,
) -> SizeResult {
    let files = flatten_dir_files(source)?;
    let total_files = files.len();
    let mut progress = Progress {
        total: total_files,
        completed: 0,
    };
    let mut total_bytes = 0;

    for file in files {
        let source_path = source.join(&file);
        let dest_paths: Vec<_> = dest.iter().map(|d| d.join(&file)).collect();

        // Create destination directories if they don't exist
        for dest_path in &dest_paths {
            if let Some(parent) = dest_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        total_bytes += read_file_copy_batch(&source_path, dest_paths).await?;

        progress.mut_increment();
        tx.send(progress).unwrap();
    }
    Ok(total_bytes)
}

pub struct ChecksumReport(pub Vec<ChecksumReportSingleFile>);

pub struct ChecksumReportSingleFile {
    pub source: (PathBuf, u64),
    pub destinations: Vec<(PathBuf, u64)>,
}

impl ChecksumReportSingleFile {
    pub fn consistent(&self) -> bool {
        let source_hash = self.source.1;
        self.destinations.iter().all(|(_, d)| *d == source_hash)
    }
}

impl ChecksumReport {
    pub fn total_files(&self) -> usize {
        self.0.len()
    }

    pub fn count_errors(&self) -> usize {
        self.0.iter().filter(|file| !file.consistent()).count()
    }
}

pub async fn hash_dirs(
    source: &PathBuf,
    dest: &Vec<PathBuf>,
    files: &Vec<PathBuf>,
    tx: watch::Sender<Progress>,
) -> io::Result<ChecksumReport> {
    let mut report = Vec::new();
    let mut progress = Progress {
        total: files.len(),
        completed: 0,
    };
    tx.send(progress).unwrap();

    for file in files {
        let source_path = source.join(file);
        let dest_paths: Vec<_> = dest.iter().map(|d| d.join(file)).collect();
        let source_path_clone = source_path.clone();
        let dest_paths_clone = dest_paths.clone();

        // Take advantage of multiple cores, just in case.
        let source_hash_future = spawn(async move { compute_file_hash(&source_path_clone).await });
        let dest_hash_futures: Vec<_> = dest_paths_clone
            .into_iter()
            .map(|dest_path| spawn(async move { compute_file_hash(dest_path).await }))
            .collect();
        let dest_hash_futures = join_all(dest_hash_futures);

        // Execute the futures concurrently
        let (source_hash_result, dest_hash_results) = join!(source_hash_future, dest_hash_futures);
        // Remove JoinError
        let source_hash_result = source_hash_result?;
        let dest_hash_results = collect_results(dest_hash_results)?;

        let mut destination_hashes = Vec::new();
        for (dest_path, dest_hash_result) in dest_paths.iter().zip(dest_hash_results) {
            destination_hashes.push((dest_path.clone(), dest_hash_result?));
        }

        report.push(ChecksumReportSingleFile {
            source: (source_path, source_hash_result?),
            destinations: destination_hashes,
        });

        progress.mut_increment();
        tx.send(progress).unwrap();
    }
    Ok(ChecksumReport(report))
}

pub async fn compute_file_hash<P: AsRef<Path>>(path: P) -> io::Result<u64> {
    let file = File::open(path).await?;
    let mut reader = BufReader::new(file);

    // Create the hasher
    let mut hasher = XxHash3_64::default();

    const CHUNK_SIZE: usize = 1024 * 1024; // 1MB
    let mut buffer = vec![0; CHUNK_SIZE];

    loop {
        let bytes_read = reader.read(&mut buffer).await?;
        if bytes_read == 0 {
            // EOF reached
            break;
        }

        // Since memory read is far faster than disk IO, and xxHash3 has roughly the same throughput as memory read,
        // we can assume it is not a long enough task to spawn_blocking
        hasher.write(&buffer[..bytes_read]);
    }

    // Return the final hash
    Ok(hasher.finish())
}
