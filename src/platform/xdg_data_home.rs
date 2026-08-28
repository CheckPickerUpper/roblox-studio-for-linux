use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub(crate) fn xdg_data_home() -> PathBuf {
    resolve_xdg_data_home(
        env::var_os("XDG_DATA_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    )
}

fn resolve_xdg_data_home(xdg_data_home: Option<&OsStr>, home: Option<&OsStr>) -> PathBuf {
    xdg_data_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|value| Path::new(value).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::resolve_xdg_data_home;
    use behave::prelude::*;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    behave! {
        "Choosing the user's data directory" {
            "XDG_DATA_HOME is set" {
                "uses it" {
                    expect!(resolve_xdg_data_home(
                        Some(OsStr::new("/data")),
                        Some(OsStr::new("/home/person")),
                    ))
                    .to_equal(PathBuf::from("/data"))?;
                }
            }

            "XDG_DATA_HOME is empty" {
                "uses the home data directory" {
                    expect!(resolve_xdg_data_home(
                        Some(OsStr::new("")),
                        Some(OsStr::new("/home/person")),
                    ))
                    .to_equal(PathBuf::from("/home/person/.local/share"))?;
                }
            }

            "both environment values are absent" {
                "uses the current directory" {
                    expect!(resolve_xdg_data_home(None, None)).to_equal(PathBuf::from("."))?;
                }
            }
        }
    }
}
