use std::io;
use std::path::{PathBuf, StripPrefixError};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum LauncherError {
    #[error("invalid arguments: {message}; provided={provided_arguments:?}")]
    InvalidArguments {
        message: String,
        provided_arguments: Vec<String>,
    },
    #[error("invalid configuration at {path} line {line}: {message}")]
    InvalidConfig {
        path: PathBuf,
        line: usize,
        message: String,
    },
    #[error("could not read {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write {path}: {source}")]
    WriteConfig {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not create Wine prefix {path}: {source}")]
    CreateWinePrefix {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Studio executable {path} is outside Wine drive C: {wine_drive}: {source}")]
    StudioExecutableOutsideWineDrive {
        path: PathBuf,
        wine_drive: PathBuf,
        #[source]
        source: StripPrefixError,
    },
    #[error("Studio launch path {path} is not a valid UTF-8 path")]
    InvalidStudioLaunchPath { path: PathBuf },
    #[error("Studio launch value contains unsupported cmd.exe characters: {value:?}")]
    InvalidStudioLaunchValue { value: String },
    #[error("could not inspect {path}: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not inspect file metadata for {path}: {source}")]
    ReadFileMetadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not start {program}: {source}")]
    RunWine {
        program: String,
        #[source]
        source: io::Error,
    },
    #[error("Wine process {program} exited without a status code")]
    WineProcessExitedWithoutCode { program: String },
    #[error("WebView2 installer {path} is unavailable")]
    MissingWebView2Installer { path: PathBuf },
    #[error("WebView2 runtime was not found under {path} after installation")]
    MissingWebView2Runtime { path: PathBuf },
    #[error("could not resolve the current launcher executable: {source}")]
    ResolveCurrentExecutable {
        #[source]
        source: io::Error,
    },
    #[error("could not create desktop application directory {path}: {source}")]
    CreateDesktopDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write desktop entry {path}: {source}")]
    WriteDesktopEntry {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not run {program} to register the browser login handler: {source}")]
    RunDesktopRegistration {
        program: String,
        #[source]
        source: io::Error,
    },
    #[error("desktop registration command {program} exited with status {exit_code}")]
    DesktopRegistrationFailed { program: String, exit_code: i32 },
    #[error("desktop registration selected {actual}, expected {expected}")]
    DesktopRegistrationMismatch { expected: String, actual: String },
    #[error("could not read desktop MIME cache {path}: {source}")]
    ReadDesktopMimeCache {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write desktop MIME cache {path}: {source}")]
    WriteDesktopMimeCache {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not fetch deployment metadata from {url}: {source}")]
    FetchDeploymentMetadata {
        url: Url,
        #[source]
        source: Box<ureq::Error>,
    },
    #[error("could not read deployment metadata from {url}: {source}")]
    ReadDeploymentMetadata {
        url: Url,
        #[source]
        source: io::Error,
    },
    #[error("could not parse deployment metadata from {url}: {source}")]
    ParseDeploymentMetadata {
        url: Url,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not fetch deployment directory map from {url}: {source}")]
    FetchDeploymentDirectories {
        url: Url,
        #[source]
        source: Box<ureq::Error>,
    },
    #[error("could not read deployment directory map from {url}: {source}")]
    ReadDeploymentDirectories {
        url: Url,
        #[source]
        source: io::Error,
    },
    #[error("could not parse deployment directory map from {url}: {message}")]
    ParseDeploymentDirectories { url: Url, message: String },
    #[error("deployment installer mapped package {package} to unsafe directory {directory:?}")]
    InvalidDeploymentPackageDirectory { package: String, directory: String },
    #[error("deployment installer did not map package {package}")]
    MissingDeploymentPackageDirectory { package: String },
    #[error("invalid deployment URL {endpoint:?}: {source}")]
    InvalidDeploymentUrl {
        endpoint: String,
        #[source]
        source: url::ParseError,
    },
    #[error("invalid deployment identifier {identifier:?}: {reason}")]
    InvalidDeploymentIdentifier { identifier: String, reason: String },
    #[error("could not read deployment manifest from {url}: {source}")]
    ReadDeploymentManifest {
        url: Url,
        #[source]
        source: io::Error,
    },
    #[error("invalid deployment manifest at {url} line {line}: {message}")]
    InvalidDeploymentManifest {
        url: Url,
        line: usize,
        message: String,
    },
    #[error("could not create deployment directory {path}: {source}")]
    CreateDeploymentDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not remove deployment directory {path}: {source}")]
    RemoveDeploymentDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "could not promote deployment directory {source_path} to {destination_path}: {source}"
    )]
    PromoteDeploymentDirectory {
        source_path: PathBuf,
        destination_path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not fetch deployment archive from {url}: {source}")]
    FetchDeploymentArchive {
        url: Url,
        #[source]
        source: Box<ureq::Error>,
    },
    #[error("could not write deployment archive {path}: {source}")]
    WriteDeploymentArchive {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not inspect deployment archive {path}: {source}")]
    ReadDeploymentArchiveMetadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not read deployment archive data from {path}: {source}")]
    ReadDeploymentArchiveData {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("deployment archive {path} has size {actual_size}, expected {expected_size}")]
    DeploymentArchiveSizeMismatch {
        path: PathBuf,
        actual_size: u64,
        expected_size: u64,
    },
    #[error(
        "deployment archive {path} extracted to {actual_size} bytes, expected {expected_size}"
    )]
    DeploymentArchiveUncompressedSizeMismatch {
        path: PathBuf,
        actual_size: u64,
        expected_size: u64,
    },
    #[error("extracted size overflow in deployment archive {path}: current={current_size}, entry={entry_size}")]
    DeploymentArchiveSizeOverflow {
        path: PathBuf,
        current_size: u64,
        entry_size: u64,
    },
    #[error("deployment archive {path} has MD5 {actual_hash}, expected {expected_hash}")]
    DeploymentArchiveHashMismatch {
        path: PathBuf,
        actual_hash: String,
        expected_hash: String,
    },
    #[error("could not open deployment archive {path}: {source}")]
    OpenDeploymentArchive {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not read deployment archive {path}: {source}")]
    ReadDeploymentArchive {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },
    #[error("deployment archive entry {entry} escapes {destination}")]
    UnsafeDeploymentArchiveEntry {
        entry: PathBuf,
        destination: PathBuf,
    },
    #[error(
        "deployment archive entry {entry} has depth {depth}, exceeding {max_depth} in {destination}"
    )]
    DeploymentArchiveEntryDepthExceeded {
        entry: PathBuf,
        destination: PathBuf,
        depth: usize,
        max_depth: usize,
    },
    #[error("could not create extracted deployment directory {path}: {source}")]
    CreateExtractedDeploymentDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not create extracted deployment file {path}: {source}")]
    CreateExtractedDeploymentFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not extract deployment archive {path}: {source}")]
    ExtractDeploymentArchive {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("latest deployment did not contain {path}")]
    MissingStudioExecutable { path: PathBuf },
    #[error("could not write deployment completion marker {path}: {source}")]
    WriteDeploymentMarker {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write deployment settings {path}: {source}")]
    WriteDeploymentSettings {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}
