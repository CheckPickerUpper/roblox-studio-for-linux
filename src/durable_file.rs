use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn replace_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let temporary_path = temporary_path(path, parent);
    let existing_permissions = match fs::metadata(path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut temporary_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary_path)?;

    let replacement = (|| {
        temporary_file.write_all(contents)?;
        if let Some(permissions) = existing_permissions {
            temporary_file.set_permissions(permissions)?;
        }
        temporary_file.sync_all()?;
        drop(temporary_file);
        fs::rename(&temporary_path, path)?;
        File::open(parent)?.sync_all()
    })();

    if replacement.is_err() {
        remove_temporary_file(&temporary_path);
    }
    replacement
}

fn temporary_path(path: &Path, parent: &Path) -> PathBuf {
    let stamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos(),
        Err(error) => {
            tracing::debug!(error = %error, "The system clock is before the Unix epoch");
            0
        }
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("replacement");
    parent.join(format!(".{file_name}.tmp-{}-{stamp}", std::process::id()))
}

fn remove_temporary_file(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::debug!(path = %path.display(), error = %error, "Could not remove a temporary replacement file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::replace_file;
    use behave::prelude::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    behave! {
        "Replacing a saved file" {
            setup {
                let stamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
                    Ok(duration) => duration.as_nanos(),
                    Err(error) => {
                        expect!(format!("the test clock failed: {error}")).to_be_empty()?;
                        return Ok(());
                    }
                };
                let test_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target")
                    .join(format!("durable-file-test-{}-{stamp}", std::process::id()));
                expect!(fs::create_dir_all(&test_directory)).to_be_ok()?;
                let saved_file = test_directory.join("settings.json");
                expect!(fs::write(&saved_file, b"old contents")).to_be_ok()?;
                #[cfg(unix)]
                expect!(fs::set_permissions(
                    &saved_file,
                    fs::Permissions::from_mode(0o640),
                ))
                .to_be_ok()?;
            }

            "new contents are saved" {
                "replaces the whole file and keeps its permissions" {
                    expect!(replace_file(&saved_file, b"new contents")).to_be_ok()?;
                    expect!(fs::read(&saved_file)?).to_equal(b"new contents".to_vec())?;
                    #[cfg(unix)]
                    expect!(fs::metadata(&saved_file)?.mode() & 0o777).to_equal(0o640)?;
                    expect!(fs::remove_dir_all(&test_directory)).to_be_ok()?;
                }
            }
        }
    }
}
