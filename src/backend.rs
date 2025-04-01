use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::{io, thread};

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
    reporter: Arc<Mutex<CopyProgress>>,
) -> io::Result<()> {
    let todo = flatten_filetree(from, from)?;
    let from = from.clone();
    let to = to.clone();
    let reporter = reporter.clone();
    thread::spawn(move || {
        {
            *(reporter.lock().unwrap()) = CopyProgress::Normal {
                total: todo.len(),
                copied: 0,
            };
        }
        for file in &todo {
            let full_from = from.join(file);
            let full_to = to.join(file);
            if let Some(parent) = full_to.parent() {
                match std::fs::create_dir_all(parent) {
                    Err(e) => {
                        *(reporter.lock().unwrap()) = CopyProgress::Error { error: e };
                        return;
                    }
                    Ok(_) => {}
                }
            }
            match std::fs::copy(&full_from, &full_to) {
                Ok(_size) => {
                    let mut progress = reporter.lock().unwrap();
                    if let CopyProgress::Normal { total, copied } = *progress {
                        *progress = CopyProgress::Normal {
                            total,
                            copied: copied + 1,
                        };
                    }
                }
                Err(e) => {
                    *(reporter.lock().unwrap()) = CopyProgress::Error { error: e };
                    return;
                }
            }
        }
        *(reporter.lock().unwrap()) = CopyProgress::Finished { total: todo.len() };
    });
    Ok(())
}

pub enum CopyProgress {
    Normal { total: usize, copied: usize },
    Error { error: io::Error },
    Finished { total: usize },
}
