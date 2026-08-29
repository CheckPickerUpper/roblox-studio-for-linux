use atomic_write_file::AtomicWriteFile;
use std::io::{self, Write};
use std::path::Path;

pub(crate) fn replace_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut replacement = AtomicWriteFile::open(path)?;
    replacement.write_all(contents)?;
    replacement.commit()
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
