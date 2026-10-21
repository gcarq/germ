use std::path::Path;
use std::{fs, io};

/// Writes `contents` to `path`, creating parent directories if needed.
pub fn write_file(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}
