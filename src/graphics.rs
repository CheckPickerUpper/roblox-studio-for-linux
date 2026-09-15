use crate::platform::STATUS_PATH_ENVIRONMENT;
use ash::vk;
use std::env;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{DeserializeDict, Type, Value};

const SWITCHEROO_SERVICE: &str = "net.hadess.SwitcherooControl";
const SWITCHEROO_OBJECT_PATH: &str = "/net/hadess/SwitcherooControl";
const SWITCHEROO_INTERFACE: &str = "net.hadess.SwitcherooControl";
const SWITCHEROO_GPUS_PROPERTY: &str = "GPUs";
/// Hidden launcher command that reports whether Vulkan can see any GPU in its environment.
pub(crate) const VULKAN_PROBE_COMMAND: &str = "probe-vulkan";
const VULKAN_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const VULKAN_PROBE_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Chooses which GPU renders Studio on a computer with more than one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum GpuPreference {
    /// Render on the discrete GPU through PRIME render offload when one is present.
    #[default]
    Discrete,
    /// Render on the GPU that drives the boot display.
    SystemDefault,
}

impl GpuPreference {
    pub(crate) const fn config_value(self) -> &'static str {
        match self {
            Self::Discrete => "discrete",
            Self::SystemDefault => "default",
        }
    }
}

/// The GPU Studio will render on, as resolved for one launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StudioGpu {
    /// switcheroo-control named a discrete GPU and the environment that selects it.
    Discrete {
        name: String,
        environment: Vec<(String, String)>,
    },
    /// switcheroo-control named a discrete GPU, but Vulkan cannot use it with that environment.
    DiscreteUnusable {
        name: String,
        failure: VulkanProbeFailure,
    },
    /// The user chose the GPU that drives the boot display.
    SystemDefault,
    /// switcheroo-control reports no discrete GPU on this computer.
    NoDiscreteGpu,
    /// switcheroo-control could not be reached, so the choice falls back to the system default.
    SwitcherooUnavailable { message: String },
    /// switcheroo-control described a GPU whose environment is not name/value pairs.
    MalformedSwitcherooGpu { name: String },
}

impl StudioGpu {
    pub(crate) fn apply(&self, command: &mut Command) {
        if let Self::Discrete { environment, .. } = self {
            command.envs(environment.iter().map(|(name, value)| (name, value)));
        }
    }
}

/// Why Vulkan could not be shown to work on the discrete GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VulkanProbeFailure {
    /// The Vulkan loader found no physical device, as happens with a crashed or mismatched driver.
    NoPhysicalDevice,
    /// The driver did not answer before the probe deadline.
    TimedOut,
    /// The probe process could not be started or observed.
    CouldNotRun { message: String },
}

impl std::fmt::Display for VulkanProbeFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoPhysicalDevice => formatter.write_str("Vulkan found no usable device"),
            Self::TimedOut => formatter.write_str("the Vulkan driver did not respond"),
            Self::CouldNotRun { message } => {
                write!(formatter, "the Vulkan check could not run: {message}")
            }
        }
    }
}

/// One entry of switcheroo-control's `GPUs` property.
#[derive(Debug, DeserializeDict, Type, Value)]
#[zvariant(signature = "a{sv}", rename_all = "PascalCase")]
struct SwitcherooGpu {
    name: String,
    environment: Vec<String>,
    discrete: bool,
}

/// Asks switcheroo-control, the desktop's GPU service, which GPU Studio should use.
pub(crate) fn studio_gpu(preference: GpuPreference) -> StudioGpu {
    match preference {
        GpuPreference::SystemDefault => StudioGpu::SystemDefault,
        GpuPreference::Discrete => match switcheroo_gpus() {
            Ok(gpus) => match discrete_gpu(gpus) {
                StudioGpu::Discrete { name, environment } => match probe_vulkan(&environment) {
                    Ok(()) => StudioGpu::Discrete { name, environment },
                    Err(failure) => StudioGpu::DiscreteUnusable { name, failure },
                },
                other => other,
            },
            Err(error) => StudioGpu::SwitcherooUnavailable {
                message: error.to_string(),
            },
        },
    }
}

fn switcheroo_gpus() -> zbus::Result<Vec<SwitcherooGpu>> {
    let connection = Connection::system()?;
    let proxy = Proxy::new(
        &connection,
        SWITCHEROO_SERVICE,
        SWITCHEROO_OBJECT_PATH,
        SWITCHEROO_INTERFACE,
    )?;
    proxy.get_property(SWITCHEROO_GPUS_PROPERTY)
}

fn discrete_gpu(gpus: Vec<SwitcherooGpu>) -> StudioGpu {
    let Some(gpu) = gpus.into_iter().find(|gpu| gpu.discrete) else {
        return StudioGpu::NoDiscreteGpu;
    };
    let (pairs, remainder) = gpu.environment.as_chunks::<2>();
    if !remainder.is_empty() {
        return StudioGpu::MalformedSwitcherooGpu { name: gpu.name };
    }
    StudioGpu::Discrete {
        environment: pairs
            .iter()
            .map(|[name, value]| (name.clone(), value.clone()))
            .collect(),
        name: gpu.name,
    }
}

// The Vulkan loader reads driver selection from the environment when an instance is created, so the
// check runs in a child process that carries exactly the environment Studio will receive.
fn probe_vulkan(environment: &[(String, String)]) -> Result<(), VulkanProbeFailure> {
    let could_not_run = |message: String| VulkanProbeFailure::CouldNotRun { message };
    let executable = env::current_exe().map_err(|error| could_not_run(error.to_string()))?;
    let mut child = Command::new(executable)
        .arg(VULKAN_PROBE_COMMAND)
        .envs(environment.iter().map(|(name, value)| (name, value)))
        .env_remove(STATUS_PATH_ENVIRONMENT)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| could_not_run(error.to_string()))?;
    let deadline = Instant::now() + VULKAN_PROBE_TIMEOUT;
    loop {
        match child
            .try_wait()
            .map_err(|error| could_not_run(error.to_string()))?
        {
            Some(status) if status.success() => return Ok(()),
            Some(_) => return Err(VulkanProbeFailure::NoPhysicalDevice),
            None if Instant::now() < deadline => thread::sleep(VULKAN_PROBE_POLL_INTERVAL),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(VulkanProbeFailure::TimedOut);
            }
        }
    }
}

/// Counts the GPUs the Vulkan loader exposes to this process; any loader failure counts as none.
pub(crate) fn vulkan_physical_device_count() -> usize {
    // SAFETY: loading the system Vulkan loader runs its initialisation code, which is the
    // intended use of libvulkan. The instance is destroyed before it goes out of scope.
    unsafe {
        let Ok(entry) = ash::Entry::load() else {
            return 0;
        };
        let application = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let create_info = vk::InstanceCreateInfo::default().application_info(&application);
        let Ok(instance) = entry.create_instance(&create_info, None) else {
            return 0;
        };
        let count = instance
            .enumerate_physical_devices()
            .map_or(0, |devices| devices.len());
        instance.destroy_instance(None);
        count
    }
}
