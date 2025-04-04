use futures::future::join_all;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::join;
use tokio::sync::watch;
use twox_hash::XxHash3_64;

pub type SizeResult = io::Result<u64>;

pub fn flatten_filetree_recur(base_dir: &PathBuf, dir: &PathBuf) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(flatten_filetree_recur(base_dir, &path)?);
        } else {
            let relative_path = path.strip_prefix(base_dir).unwrap().to_path_buf();
            files.push(relative_path);
        }
    }
    Ok(files)
}

pub fn flatten_filetree(base_dir: &PathBuf) -> io::Result<Vec<PathBuf>> {
    flatten_filetree_recur(base_dir, base_dir)
}

pub struct Progress {
    pub total: usize,
    pub completed: usize,
}
impl Progress {
    pub fn mut_increment(&mut self) {
        self.completed += 1;
    }

    pub fn increment(&self) -> Self {
        Progress {
            total: self.total,
            completed: self.completed + 1,
        }
    }
}
pub async fn read_file_copy_batch<P: AsRef<Path>>(
    source_path: P,
    dest_paths: Vec<PathBuf>,
) -> SizeResult {
    // Open the source file
    let mut source_file = match File::open(&source_path).await {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to open source file: {}", e);
            return Err(e);
        }
    };

    // Open all destination files
    let mut dest_files = Vec::with_capacity(dest_paths.len());
    for path in dest_paths {
        match File::create(&path).await {
            Ok(file) => dest_files.push(file),
            Err(e) => {
                eprintln!("Failed to create destination file {:?}: {}", path, e);
                return Err(e);
            }
        }
    }

    let mut total_bytes = 0;

    const BUFFER_SIZE: usize = 1024 * 1024; // 1MB
    let mut buffer1 = vec![0u8; BUFFER_SIZE];
    let mut buffer2 = vec![0u8; BUFFER_SIZE];

    // Rotated buffers for concurrent read/write
    let mut read_buffer = &mut buffer2;
    let mut write_buffer = &mut buffer1;

    // Read first chunk into write_buffer
    let mut bytes_read = source_file.read(write_buffer).await?;
    if bytes_read == 0 {
        return Ok(0); // Source file is empty
    }

    // Main copy loop
    loop {
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
            if let Err(e) = result {
                eprintln!("Write error: {}", e);
                return Err(e);
            }
        }

        // Get the result of the read operation
        match read_result {
            Ok(n) => {
                bytes_read = n; // Might not be BUFFER_SIZE if the next read will hit EOF
                total_bytes += bytes_read as u64;
                if bytes_read == 0 {
                    break; // EOF
                }
            }
            Err(e) => {
                eprintln!("Read error: {}", e);
                return Err(e);
            }
        }
    }

    // Flush all destination files
    for file in &mut dest_files {
        if let Err(e) = file.flush().await {
            eprintln!("Error flushing file: {}", e);
            return Err(e);
        }
    }

    Ok(total_bytes)
}

pub async fn copy_dirs(
    source: &PathBuf,
    dest: &Vec<PathBuf>,
    rx: watch::Sender<Progress>,
) -> SizeResult {
    let files = flatten_filetree(source)?;
    let total_files = files.len();
    let progress = Progress {
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

        match read_file_copy_batch(&source_path, dest_paths).await {
            Ok(bytes) => total_bytes += bytes,
            Err(e) => {
                eprintln!("Error copying file {:?}: {}", source_path, e);
                return Err(e);
            }
        }

        let progress = progress.increment();
        rx.send(progress).unwrap();
    }
    Ok(total_bytes)
}

pub struct ChecksumReport(pub Vec<ChecksumReportSingleFile>);

pub struct ChecksumReportSingleFile {
    pub source_hash: (PathBuf, u64),
    pub destination_hash: Vec<(PathBuf, u64)>,
}

impl ChecksumReport {
    pub fn total_files(&self) -> usize {
        self.0.len()
    }

    pub fn count_errors(&self) -> usize {
        self.0
            .iter()
            .filter(|report| {
                let source_hash = report.source_hash.1;
                report
                    .destination_hash
                    .iter()
                    .any(|(_, dest_hash)| source_hash != *dest_hash)
            })
            .count()
    }
}

pub async fn hash_dirs(source: &PathBuf, dest: &Vec<PathBuf>) -> io::Result<ChecksumReport> {
    let files = flatten_filetree(source)?;
    let mut report = Vec::new();

    for file in files {
        let source_path = source.join(&file);
        let dest_paths: Vec<_> = dest.iter().map(|d| d.join(&file)).collect();

        let source_hash_future = compute_file_hash(&source_path);
        let dest_hash_futures: Vec<_> = dest_paths
            .iter()
            .map(|dest_path| compute_file_hash(dest_path))
            .collect();
        let dest_hash_futures = join_all(dest_hash_futures);

        // Execute the futures concurrently
        let (source_hash_result, dest_hash_results) = join!(source_hash_future, dest_hash_futures);

        let mut destination_hashes = Vec::new();
        for (dest_path, dest_hash_result) in dest_paths.iter().zip(dest_hash_results) {
            match dest_hash_result {
                Ok(hash) => destination_hashes.push((dest_path.clone(), hash)),
                Err(e) => {
                    eprintln!("Error computing hash for {:?}: {}", dest_path, e);
                    return Err(e);
                }
            }
        }

        report.push(ChecksumReportSingleFile {
            source_hash: (source_path, source_hash_result?),
            destination_hash: destination_hashes,
        });
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
            // EoF reached
            break;
        }

        // Since memory read is far faster than disk IO, and xxHash3 has roughly the same throughput as memory read, 
        // we can assume it is not a long enough task to spawn_blocking
        hasher.write(&buffer[..bytes_read]);
    }

    // Return the final hash
    Ok(hasher.finish())
}
