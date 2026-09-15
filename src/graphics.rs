use std::process::Command;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{DeserializeDict, Type, Value};

const SWITCHEROO_SERVICE: &str = "net.hadess.SwitcherooControl";
const SWITCHEROO_OBJECT_PATH: &str = "/net/hadess/SwitcherooControl";
const SWITCHEROO_INTERFACE: &str = "net.hadess.SwitcherooControl";
const SWITCHEROO_GPUS_PROPERTY: &str = "GPUs";

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
            Ok(gpus) => discrete_gpu(gpus),
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
