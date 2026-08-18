use crate::error::LauncherError;
use md5::Context;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use url::Url;
use zip::ZipArchive;

const DEPLOYMENT_ENDPOINT: &str =
    "https://clientsettingscdn.roblox.com/v2/client-version/WindowsStudio64";
const CDN_CHANNEL_PATH: &str = "https://setup.rbxcdn.com/channel/common";
const MANIFEST_SUFFIX: &str = "rbxPkgManifest.txt";
const INSTALLER_SUFFIX: &str = "RobloxStudioInstaller.exe";
const STUDIO_EXECUTABLE: &str = "RobloxStudioBeta.exe";
const INSTALLATION_MARKER: &str = ".roblox-studio-deployment-complete-v4";
const APP_SETTINGS_FILE: &str = "AppSettings.xml";
const APP_SETTINGS_CONTENT: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n",
    "<Settings>\r\n",
    "        <ContentFolder>content</ContentFolder>\r\n",
    "        <BaseUrl>http://www.roblox.com</BaseUrl>\r\n",
    "</Settings>\r\n",
);
const STAGING_SUFFIX: &str = ".staging";
const MANIFEST_HEADER: &str = "v0";
const MD5_HEX_LENGTH: usize = 32;
const MAX_ARCHIVE_ENTRY_DEPTH: usize = 64;
const DIGEST_BUFFER_SIZE: usize = 1024 * 1024;
const EMPTY_BYTE: u8 = 0;
const EMPTY_SIZE: u64 = 0;

#[derive(Debug, Deserialize)]
struct StudioDeploymentResponse {
    #[serde(rename = "clientVersionUpload")]
    client_version_upload: String,
}

#[derive(Debug)]
struct DeploymentPackage {
    archive_name: String,
    expected_md5: String,
    compressed_size: u64,
    uncompressed_size: u64,
}

pub(crate) fn install_latest_studio(wine_prefix: &Path) -> Result<PathBuf, LauncherError> {
    let deployment = fetch_current_deployment()?;
    validate_deployment_identifier(&deployment.client_version_upload)?;

    let version_directory = wine_prefix
        .join("drive_c")
        .join("Roblox")
        .join("Versions")
        .join(&deployment.client_version_upload);
    let studio_executable = version_directory.join(STUDIO_EXECUTABLE);
    let installation_marker = version_directory.join(INSTALLATION_MARKER);
    let app_settings = version_directory.join(APP_SETTINGS_FILE);
    if studio_executable.is_file() && installation_marker.is_file() && app_settings.is_file() {
        tracing::info!(
            path = %studio_executable.display(),
            "Current Studio deployment is already installed"
        );
        return Ok(studio_executable);
    }

    let manifest_endpoint = format!(
        "{CDN_CHANNEL_PATH}/{}-{MANIFEST_SUFFIX}",
        deployment.client_version_upload
    );
    let manifest_url = parse_deployment_url(&manifest_endpoint)?;
    let manifest = fetch_manifest(&manifest_url)?;
    let packages = parse_manifest(&manifest, &manifest_url)?;
    let package_directories =
        fetch_package_directories(&deployment.client_version_upload, &packages)?;

    let cache_directory = wine_prefix.join("deployment-cache");
    fs::create_dir_all(&cache_directory).map_err(|source| {
        LauncherError::CreateDeploymentDirectory {
            path: cache_directory.clone(),
            source,
        }
    })?;

    let staging_directory = version_directory.with_file_name(format!(
        "{}{}",
        deployment.client_version_upload, STAGING_SUFFIX
    ));
    if staging_directory.exists() {
        fs::remove_dir_all(&staging_directory).map_err(|source| {
            LauncherError::RemoveDeploymentDirectory {
                path: staging_directory.clone(),
                source,
            }
        })?;
    }
    fs::create_dir_all(&staging_directory).map_err(|source| {
        LauncherError::CreateDeploymentDirectory {
            path: staging_directory.clone(),
            source,
        }
    })?;

    for package in packages {
        let archive_endpoint = format!(
            "{CDN_CHANNEL_PATH}/{}-{}",
            deployment.client_version_upload, package.archive_name
        );
        let archive_url = parse_deployment_url(&archive_endpoint)?;
        let archive_path = cache_directory.join(format!(
            "{}-{}",
            deployment.client_version_upload, package.archive_name
        ));

        let cached_archive_is_valid = match archive_path.is_file() {
            true => match verify_archive(&archive_path, &package) {
                Ok(()) => true,
                Err(error) => {
                    tracing::debug!(
                        path = %archive_path.display(),
                        error = %error,
                        "Cached deployment archive failed verification"
                    );
                    false
                }
            },
            false => false,
        };

        match cached_archive_is_valid {
            true => tracing::debug!(
                path = %archive_path.display(),
                "Using verified cached deployment archive"
            ),
            false => download_archive(&archive_url, &archive_path, &package)?,
        }
        let destination =
            staging_directory.join(package_directories.get(&package.archive_name).ok_or_else(
                || LauncherError::MissingDeploymentPackageDirectory {
                    package: package.archive_name.clone(),
                },
            )?);
        let extracted_size = extract_archive(&archive_path, &destination)?;
        if extracted_size != package.uncompressed_size {
            return Err(LauncherError::DeploymentArchiveUncompressedSizeMismatch {
                path: archive_path,
                actual_size: extracted_size,
                expected_size: package.uncompressed_size,
            });
        }
    }

    let staged_studio_executable = staging_directory.join(STUDIO_EXECUTABLE);
    if !staged_studio_executable.is_file() {
        return Err(LauncherError::MissingStudioExecutable {
            path: staged_studio_executable,
        });
    }
    let staged_app_settings = staging_directory.join(APP_SETTINGS_FILE);
    fs::write(&staged_app_settings, APP_SETTINGS_CONTENT).map_err(|source| {
        LauncherError::WriteDeploymentSettings {
            path: staged_app_settings,
            source,
        }
    })?;
    let staged_installation_marker = staging_directory.join(INSTALLATION_MARKER);
    fs::write(
        &staged_installation_marker,
        format!("{}\n", deployment.client_version_upload),
    )
    .map_err(|source| LauncherError::WriteDeploymentMarker {
        path: staged_installation_marker,
        source,
    })?;

    if version_directory.exists() {
        fs::remove_dir_all(&version_directory).map_err(|source| {
            LauncherError::RemoveDeploymentDirectory {
                path: version_directory.clone(),
                source,
            }
        })?;
    }
    fs::rename(&staging_directory, &version_directory).map_err(|source| {
        LauncherError::PromoteDeploymentDirectory {
            source_path: staging_directory,
            destination_path: version_directory.clone(),
            source,
        }
    })?;
    Ok(studio_executable)
}

fn fetch_current_deployment() -> Result<StudioDeploymentResponse, LauncherError> {
    let endpoint = parse_deployment_url(DEPLOYMENT_ENDPOINT)?;
    let response = ureq::get(endpoint.as_str()).call().map_err(|source| {
        LauncherError::FetchDeploymentMetadata {
            url: endpoint.clone(),
            source: Box::new(source),
        }
    })?;
    let mut body = String::new();
    response
        .into_reader()
        .read_to_string(&mut body)
        .map_err(|source| LauncherError::ReadDeploymentMetadata {
            url: endpoint.clone(),
            source,
        })?;
    serde_json::from_str(&body).map_err(|source| LauncherError::ParseDeploymentMetadata {
        url: endpoint,
        source,
    })
}

fn fetch_package_directories(
    deployment_identifier: &str,
    packages: &[DeploymentPackage],
) -> Result<BTreeMap<String, PathBuf>, LauncherError> {
    let installer_endpoint =
        format!("{CDN_CHANNEL_PATH}/{deployment_identifier}-{INSTALLER_SUFFIX}");
    let installer_url = parse_deployment_url(&installer_endpoint)?;
    let response = ureq::get(installer_url.as_str()).call().map_err(|source| {
        LauncherError::FetchDeploymentDirectories {
            url: installer_url.clone(),
            source: Box::new(source),
        }
    })?;
    let mut installer = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut installer)
        .map_err(|source| LauncherError::ReadDeploymentDirectories {
            url: installer_url.clone(),
            source,
        })?;

    let raw_directories = scan_package_directories(&installer).ok_or_else(|| {
        LauncherError::ParseDeploymentDirectories {
            url: installer_url.clone(),
            message: "the official installer did not contain a package directory map".to_owned(),
        }
    })?;
    let mut directories = BTreeMap::new();
    for (package, directory) in raw_directories {
        let normalized_directory = normalize_deployment_directory(&directory).ok_or_else(|| {
            LauncherError::InvalidDeploymentPackageDirectory {
                package: package.clone(),
                directory,
            }
        })?;
        directories.insert(package, normalized_directory);
    }

    for package in packages {
        if !directories.contains_key(&package.archive_name) {
            return Err(LauncherError::MissingDeploymentPackageDirectory {
                package: package.archive_name.clone(),
            });
        }
    }
    Ok(directories)
}

fn scan_package_directories(installer: &[u8]) -> Option<BTreeMap<String, String>> {
    let mut candidate_start = None;
    for index in 0..installer.len() {
        let starts_json = index + 1 < installer.len()
            && installer[index] == b"{"[0]
            && installer[index + 1] == b"\""[0]
            && (index == 0 || installer[index - 1] == b"\0"[0]);
        if starts_json {
            candidate_start = Some(index);
        }

        let ends_json = index >= 2
            && installer[index] == b"\0"[0]
            && installer[index - 1] == b"}"[0]
            && installer[index - 2] == b"\""[0];
        if ends_json {
            if let Some(start) = candidate_start.take() {
                if let Ok(directories) = serde_json::from_slice(&installer[start..index]) {
                    return Some(directories);
                }
            }
        }
    }
    None
}

fn normalize_deployment_directory(directory: &str) -> Option<PathBuf> {
    let normalized_name = directory.replace(char::from(92), "/");
    let relative_name = normalized_name.trim_end_matches(char::from(47));
    if relative_name.is_empty() {
        return Some(PathBuf::new());
    }

    let mut relative_path = PathBuf::new();
    for component in Path::new(relative_name).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => relative_path.push(segment),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(relative_path)
}

fn parse_deployment_url(endpoint: &str) -> Result<Url, LauncherError> {
    Url::parse(endpoint).map_err(|source| LauncherError::InvalidDeploymentUrl {
        endpoint: endpoint.to_owned(),
        source,
    })
}

fn validate_deployment_identifier(identifier: &str) -> Result<(), LauncherError> {
    let valid = identifier.starts_with("version-")
        && identifier.len() > "version-".len()
        && identifier
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-');
    match valid {
        true => Ok(()),
        false => Err(LauncherError::InvalidDeploymentIdentifier {
            identifier: identifier.to_owned(),
            reason: "expected version- followed by ASCII letters, numbers, or hyphens".to_owned(),
        }),
    }
}

fn fetch_manifest(url: &Url) -> Result<String, LauncherError> {
    let response = ureq::get(url.as_str()).call().map_err(|source| {
        LauncherError::FetchDeploymentMetadata {
            url: url.clone(),
            source: Box::new(source),
        }
    })?;
    let mut manifest = String::new();
    response
        .into_reader()
        .read_to_string(&mut manifest)
        .map_err(|source| LauncherError::ReadDeploymentManifest {
            url: url.clone(),
            source,
        })?;
    Ok(manifest)
}

fn parse_manifest(manifest: &str, url: &Url) -> Result<Vec<DeploymentPackage>, LauncherError> {
    let mut lines = manifest.lines().enumerate();
    let (header_line, header) = match lines.next() {
        Some(value) => value,
        None => {
            return Err(invalid_manifest(url, 1, "manifest is empty".to_owned()));
        }
    };
    match header {
        MANIFEST_HEADER => {}
        _ => {
            return Err(invalid_manifest(
                url,
                header_line + 1,
                format!("expected {MANIFEST_HEADER:?}, found {header:?}"),
            ));
        }
    }

    let mut packages = Vec::new();
    while let Some(package_line) = lines.next() {
        let hash_line = match lines.next() {
            Some(value) => value,
            None => {
                return Err(invalid_manifest(
                    url,
                    package_line.0 + 1,
                    "package entry is missing its MD5 and size fields".to_owned(),
                ));
            }
        };
        let compressed_size_line = match lines.next() {
            Some(value) => value,
            None => {
                return Err(invalid_manifest(
                    url,
                    hash_line.0 + 1,
                    "package entry is missing its compressed size".to_owned(),
                ));
            }
        };
        let uncompressed_size_line = match lines.next() {
            Some(value) => value,
            None => {
                return Err(invalid_manifest(
                    url,
                    compressed_size_line.0 + 1,
                    "package entry is missing its uncompressed size".to_owned(),
                ));
            }
        };

        validate_archive_name(package_line.1, url, package_line.0 + 1)?;
        validate_md5(hash_line.1, url, hash_line.0 + 1)?;
        let compressed_size =
            parse_manifest_size(compressed_size_line.1, url, compressed_size_line.0 + 1)?;
        let uncompressed_size =
            parse_manifest_size(uncompressed_size_line.1, url, uncompressed_size_line.0 + 1)?;

        tracing::debug!(
            package = package_line.1,
            expected_md5 = hash_line.1,
            compressed_size,
            uncompressed_size,
            "Parsed deployment package"
        );
        packages.push(DeploymentPackage {
            archive_name: package_line.1.to_owned(),
            expected_md5: hash_line.1.to_ascii_lowercase(),
            compressed_size,
            uncompressed_size,
        });
    }

    if packages.is_empty() {
        return Err(invalid_manifest(
            url,
            header_line + 1,
            "manifest contains no packages".to_owned(),
        ));
    }
    Ok(packages)
}

fn validate_archive_name(name: &str, url: &Url, line: usize) -> Result<(), LauncherError> {
    let valid = name.ends_with(".zip")
        && !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        });
    match valid {
        true => Ok(()),
        false => Err(invalid_manifest(
            url,
            line,
            format!("unsafe archive name {name:?}"),
        )),
    }
}

fn validate_md5(value: &str, url: &Url, line: usize) -> Result<(), LauncherError> {
    let valid = value.len() == MD5_HEX_LENGTH
        && value.chars().all(|character| character.is_ascii_hexdigit());
    match valid {
        true => Ok(()),
        false => Err(invalid_manifest(
            url,
            line,
            format!("invalid MD5 digest {value:?}"),
        )),
    }
}

fn parse_manifest_size(value: &str, url: &Url, line: usize) -> Result<u64, LauncherError> {
    value.parse::<u64>().map_err(|source| {
        invalid_manifest(
            url,
            line,
            format!("invalid archive size {value:?}: {source}"),
        )
    })
}

fn invalid_manifest(url: &Url, line: usize, message: String) -> LauncherError {
    LauncherError::InvalidDeploymentManifest {
        url: url.clone(),
        line,
        message,
    }
}

fn download_archive(
    url: &Url,
    archive_path: &Path,
    package: &DeploymentPackage,
) -> Result<(), LauncherError> {
    tracing::info!(
        package = %package.archive_name,
        url = %url,
        "Downloading Studio deployment package"
    );
    let response =
        ureq::get(url.as_str())
            .call()
            .map_err(|source| LauncherError::FetchDeploymentArchive {
                url: url.clone(),
                source: Box::new(source),
            })?;
    let mut input = response.into_reader();
    let mut output =
        File::create(archive_path).map_err(|source| LauncherError::WriteDeploymentArchive {
            path: archive_path.to_path_buf(),
            source,
        })?;
    io::copy(&mut input, &mut output).map_err(|source| LauncherError::WriteDeploymentArchive {
        path: archive_path.to_path_buf(),
        source,
    })?;
    output
        .flush()
        .map_err(|source| LauncherError::WriteDeploymentArchive {
            path: archive_path.to_path_buf(),
            source,
        })?;
    verify_archive(archive_path, package)
}

fn verify_archive(archive_path: &Path, package: &DeploymentPackage) -> Result<(), LauncherError> {
    let metadata = fs::metadata(archive_path).map_err(|source| {
        LauncherError::ReadDeploymentArchiveMetadata {
            path: archive_path.to_path_buf(),
            source,
        }
    })?;
    match metadata.len() {
        actual_size if actual_size == package.compressed_size => {}
        actual_size => {
            return Err(LauncherError::DeploymentArchiveSizeMismatch {
                path: archive_path.to_path_buf(),
                actual_size,
                expected_size: package.compressed_size,
            });
        }
    }

    let mut input =
        File::open(archive_path).map_err(|source| LauncherError::OpenDeploymentArchive {
            path: archive_path.to_path_buf(),
            source,
        })?;
    let mut digest = Context::new();
    let mut buffer = [EMPTY_BYTE; DIGEST_BUFFER_SIZE];
    loop {
        let bytes_read =
            input
                .read(&mut buffer)
                .map_err(|source| LauncherError::ReadDeploymentArchiveData {
                    path: archive_path.to_path_buf(),
                    source,
                })?;
        match bytes_read {
            0 => break,
            count => digest.consume(&buffer[..count]),
        }
    }
    let actual_hash = format!("{:x}", digest.compute());
    if actual_hash == package.expected_md5 {
        return Ok(());
    }
    Err(LauncherError::DeploymentArchiveHashMismatch {
        path: archive_path.to_path_buf(),
        actual_hash,
        expected_hash: package.expected_md5.clone(),
    })
}

fn extract_archive(archive_path: &Path, destination_root: &Path) -> Result<u64, LauncherError> {
    tracing::info!(
        path = %archive_path.display(),
        destination = %destination_root.display(),
        "Extracting Studio deployment package"
    );
    let input =
        File::open(archive_path).map_err(|source| LauncherError::OpenDeploymentArchive {
            path: archive_path.to_path_buf(),
            source,
        })?;
    let mut archive =
        ZipArchive::new(input).map_err(|source| LauncherError::ReadDeploymentArchive {
            path: archive_path.to_path_buf(),
            source,
        })?;
    let mut extracted_size = EMPTY_SIZE;

    for index in 0..archive.len() {
        let mut entry =
            archive
                .by_index(index)
                .map_err(|source| LauncherError::ReadDeploymentArchive {
                    path: archive_path.to_path_buf(),
                    source,
                })?;
        let entry_name = entry.name().to_owned();
        match entry_name.as_str() {
            "" | "/" | "\\" => continue,
            _ => {}
        }
        let Some(relative_path) = normalize_archive_entry(&entry_name) else {
            return Err(LauncherError::UnsafeDeploymentArchiveEntry {
                entry: PathBuf::from(entry_name),
                destination: destination_root.to_path_buf(),
            });
        };
        let depth = relative_path.components().count();
        if depth > MAX_ARCHIVE_ENTRY_DEPTH {
            return Err(LauncherError::DeploymentArchiveEntryDepthExceeded {
                entry: relative_path,
                destination: destination_root.to_path_buf(),
                depth,
                max_depth: MAX_ARCHIVE_ENTRY_DEPTH,
            });
        }

        let destination = destination_root.join(&relative_path);
        match entry.is_dir() {
            true => fs::create_dir_all(&destination).map_err(|source| {
                LauncherError::CreateExtractedDeploymentDirectory {
                    path: destination,
                    source,
                }
            })?,
            false => {
                let Some(parent) = destination.parent() else {
                    return Err(LauncherError::UnsafeDeploymentArchiveEntry {
                        entry: relative_path,
                        destination: destination_root.to_path_buf(),
                    });
                };
                fs::create_dir_all(parent).map_err(|source| {
                    LauncherError::CreateExtractedDeploymentDirectory {
                        path: parent.to_path_buf(),
                        source,
                    }
                })?;
                let mut output = File::create(&destination).map_err(|source| {
                    LauncherError::CreateExtractedDeploymentFile {
                        path: destination.clone(),
                        source,
                    }
                })?;
                let bytes_written = io::copy(&mut entry, &mut output).map_err(|source| {
                    LauncherError::ExtractDeploymentArchive {
                        path: archive_path.to_path_buf(),
                        source,
                    }
                })?;
                output
                    .flush()
                    .map_err(|source| LauncherError::ExtractDeploymentArchive {
                        path: archive_path.to_path_buf(),
                        source,
                    })?;
                extracted_size = match extracted_size.checked_add(bytes_written) {
                    Some(value) => value,
                    None => {
                        return Err(LauncherError::DeploymentArchiveSizeOverflow {
                            path: archive_path.to_path_buf(),
                            current_size: extracted_size,
                            entry_size: bytes_written,
                        });
                    }
                };
            }
        }
    }

    Ok(extracted_size)
}

fn normalize_archive_entry(entry_name: &str) -> Option<PathBuf> {
    let normalized_name = entry_name.replace('\\', "/");
    let relative_name = normalized_name.trim_start_matches('/');
    let mut relative_path = PathBuf::new();
    for component in Path::new(relative_name).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => relative_path.push(segment),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if relative_path.as_os_str().is_empty() {
        None
    } else {
        Some(relative_path)
    }
}
