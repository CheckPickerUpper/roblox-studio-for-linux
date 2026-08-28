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
    #[error("could not start the graphical launcher: {message}")]
    GuiStartup { message: String },
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
    #[error("could not serialize the MCP doctor result: {source}")]
    SerializeMcpDoctorFinding {
        #[source]
        source: serde_json::Error,
    },
    #[error("could not create Wine prefix {path}: {source}")]
    CreateWinePrefix {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not read Wine registry {path}: {source}")]
    ReadWineRegistry {
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
    #[error("managed Wine window-driver setup exited with status {exit_code}")]
    WineGraphicsConfigurationFailed { exit_code: i32 },
    #[error(
        "Wine is already running in {prefix}; refusing to change its window driver from {saved_driver} to {desired_driver} because that would close Studio"
    )]
    WineGraphicsChangeWhileRunning {
        prefix: PathBuf,
        saved_driver: String,
        desired_driver: String,
    },
    #[error("Wine server check {program} exited with status {exit_code}")]
    WineServerCheckFailed { program: String, exit_code: i32 },
    #[error("WebView2 runtime was not found under {path} after installation")]
    MissingWebView2Runtime { path: PathBuf },
    #[error("could not prepare the managed WebView2 runtime: {message}")]
    PrepareWebView2Runtime { message: String },
    #[error("could not run the managed WebView2 download: {source}")]
    RunWebView2Download {
        #[source]
        source: io::Error,
    },
    #[error("managed WebView2 download exited with status {exit_code}")]
    WebView2DownloadFailed { exit_code: String },
    #[error("could not parse managed WebView2 download information: {source}")]
    ParseWebView2Download {
        #[source]
        source: serde_json::Error,
    },
    #[error("could not read managed WebView2 file {path}: {source}")]
    ReadWebView2File {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write managed WebView2 file {path}: {source}")]
    WriteWebView2File {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("managed DXVK files are missing from {path}")]
    MissingManagedDxvk { path: PathBuf },
    #[error("could not prepare managed DXVK file {path}: {source}")]
    PrepareManagedDxvkFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not read Studio client settings {path}: {source}")]
    ReadStudioClientSettings {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not parse Studio client settings {path}: {source}")]
    ParseStudioClientSettings {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("Studio client settings {path} are invalid: {message}")]
    InvalidStudioClientSettings { path: PathBuf, message: String },
    #[error("could not create Studio client settings directory {path}: {source}")]
    CreateStudioClientSettingsDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write Studio client settings {path}: {source}")]
    WriteStudioClientSettings {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
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
    #[error("could not write launcher icon {path}: {source}")]
    WriteDesktopIcon {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not open the Roblox sign-in page in the browser: {source}")]
    OpenBrowser {
        #[source]
        source: io::Error,
    },
    #[error("browser sign-in opener exited with status {exit_code}")]
    BrowserOpenFailed { exit_code: String },
    #[error("could not inspect Studio browser history {path}: {message}")]
    ReadBrowserHistory { path: PathBuf, message: String },
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
    #[error("latest deployment did not contain the matching Studio MCP executable {path}")]
    MissingStudioMcpExecutable { path: PathBuf },
    #[error("the selected Studio installation has no matching StudioMCP.exe at {path}")]
    MissingMcpExecutable { path: PathBuf },
    #[error("MCP runtime is unavailable: {message}")]
    McpRuntimeUnavailable { message: String },
    #[error("Flatpak Studio instance is unavailable: {message}")]
    FlatpakInstanceUnavailable { message: String },
    #[error("MCP protocol request {method} failed: {message}")]
    McpProtocolFailure { method: String, message: String },
    #[error("MCP protocol request {method} timed out after {timeout_seconds} seconds")]
    McpProtocolTimeout {
        method: String,
        timeout_seconds: u64,
    },
    #[error("MCP client configuration at {path} is invalid: {message}")]
    InvalidMcpClientConfiguration { path: PathBuf, message: String },
    #[error("could not read MCP client configuration {path}: {source}")]
    ReadMcpClientConfiguration {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not parse MCP client configuration {path}: {source}")]
    ParseMcpClientConfiguration {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not serialize MCP client configuration: {source}")]
    SerializeMcpClientConfiguration {
        #[source]
        source: serde_json::Error,
    },
    #[error("could not back up MCP client configuration {path}: {source}")]
    BackupMcpClientConfiguration {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write MCP client configuration {path}: {source}")]
    WriteMcpClientConfiguration {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
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
