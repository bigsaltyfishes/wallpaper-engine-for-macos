use std::{io, path::Path};

use tempfile::NamedTempFile;

/// # Errors
///
/// Returns an error when the parent directory cannot be created, the temporary
/// file cannot be written and synced, or the final persist operation fails.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = match path.parent() {
        Some(parent) if parent.as_os_str().is_empty() => Path::new("."),
        Some(parent) => parent,
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path has no parent",
            ));
        }
    };

    std::fs::create_dir_all(parent)?;

    let mut temp = NamedTempFile::new_in(parent)?;
    io::Write::write_all(&mut temp, bytes)?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|err| err.error)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn atomic_write_creates_parent_directories() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(Path::new("nested/app.toml"));

        atomic_write(&path, b"ok").unwrap();

        assert_eq!(fs::read(path).unwrap(), b"ok");
    }
}
