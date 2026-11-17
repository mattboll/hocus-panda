//! Where whisper.cpp hears: a GPU through Vulkan, whatever its vendor
//! (Intel, AMD, NVIDIA, a hybrid laptop's two), else the CPU.
//!
//! - **Which GPU**: a discrete one first (a hybrid laptop's NVIDIA or AMD)
//!   with memory enough for the model, the largest; else an integrated one
//!   with enough; else the CPU, with the smaller model. A discrete GPU
//!   asleep (runtime power management) takes a second to wake: fine for a
//!   job that runs after the meeting.
//! - **Chosen by hand**: `[meetings] device` (`--device`): `auto`, `cpu`,
//!   or a part of a device's name (`RTX`, `Radeon`, `llvmpipe`), which is
//!   taken whatever its kind.
//!
//! The devices are read from the system's Vulkan loader here ([`list`]);
//! the one chosen is the only one whisper.cpp's Vulkan backend is shown
//! (`GGML_VK_VISIBLE_DEVICES`, by its index in the loader's list).

/// What a device is, as Vulkan says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Discrete,
    Integrated,
    Virtual,
    /// A CPU's Vulkan (Mesa's lavapipe): slower than whisper.cpp's own.
    Cpu,
    Other,
}

/// A Vulkan device: its index in the loader's list, its name, its kind,
/// and its largest memory of its own (bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub index: usize,
    pub name: String,
    pub kind: Kind,
    pub memory: u64,
}

/// What `[meetings] device` asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wanted {
    Auto,
    Cpu,
    /// The first device whose name holds this, without regard to case.
    Named(String),
}

impl Wanted {
    pub fn of(said: &str) -> Self {
        match said.trim() {
            "" | "auto" => Self::Auto,
            "cpu" => Self::Cpu,
            name => Self::Named(name.to_owned()),
        }
    }
}

/// Where it hears, and why: the one line the log says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Gpu { device: Device, why: String },
    Cpu { why: String },
}

/// The memory the large model takes on a GPU, its buffers included.
pub const LARGE_NEEDS: u64 = 1_600_000_000;

/// The device to hear on among `devices`, as `wanted` says, for a model
/// needing `needs` bytes.
pub fn choose(devices: &[Device], wanted: &Wanted, needs: u64) -> Choice {
    let gb = |b: u64| format!("{:.1} GB", b as f64 / 1e9);
    match wanted {
        Wanted::Cpu => {
            return Choice::Cpu {
                why: "asked for ([meetings] device = \"cpu\")".to_owned(),
            };
        }
        Wanted::Named(part) => {
            let lower = part.to_lowercase();
            if let Some(device) = devices
                .iter()
                .find(|d| d.name.to_lowercase().contains(&lower))
            {
                return Choice::Gpu {
                    why: format!("asked for by name ({part:?})"),
                    device: device.clone(),
                };
            }
        }
        Wanted::Auto => {}
    }
    let named_missing = match wanted {
        Wanted::Named(part) => format!("no device named {part:?}; "),
        _ => String::new(),
    };
    for (kind, said) in [
        (Kind::Discrete, "a discrete GPU"),
        (Kind::Integrated, "an integrated GPU"),
    ] {
        let best = devices
            .iter()
            .filter(|d| d.kind == kind && d.memory >= needs)
            .max_by_key(|d| d.memory);
        if let Some(device) = best {
            return Choice::Gpu {
                why: format!("{named_missing}{said} with {}", gb(device.memory)),
                device: device.clone(),
            };
        }
    }
    let why = if devices.is_empty() {
        "no Vulkan device".to_owned()
    } else {
        format!("no GPU with {} of memory", gb(needs))
    };
    Choice::Cpu {
        why: format!("{named_missing}{why}"),
    }
}

/// The Vulkan devices the system's loader lists, in its order; none
/// without a loader or a driver.
#[allow(unsafe_code)]
pub fn list() -> Vec<Device> {
    use ash::vk;
    // SAFETY: the system's Vulkan loader, a C library, loaded as ash
    // loads it; nothing of it is used once `entry` goes.
    let Ok(entry) = (unsafe { ash::Entry::load() }) else {
        return Vec::new();
    };
    let app = vk::ApplicationInfo::default()
        .application_name(c"hocus-transcribe")
        .api_version(vk::make_api_version(0, 1, 1, 0));
    let info = vk::InstanceCreateInfo::default().application_info(&app);
    // SAFETY: a valid create info, which outlives the call; the instance is
    // destroyed below, after its last use.
    let Ok(instance) = (unsafe { entry.create_instance(&info, None) }) else {
        return Vec::new();
    };
    // SAFETY: `instance` is alive; the physical devices are its own and
    // are read only while it is.
    let physical = unsafe { instance.enumerate_physical_devices() }.unwrap_or_default();
    let devices = physical
        .iter()
        .enumerate()
        .map(|(index, device)| {
            // SAFETY: `device` is one of `instance`'s, alive.
            let props = unsafe { instance.get_physical_device_properties(*device) };
            // SAFETY: as above.
            let memory = unsafe { instance.get_physical_device_memory_properties(*device) };
            let own = memory
                .memory_heaps_as_slice()
                .iter()
                .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
                .map(|h| h.size)
                .max()
                .unwrap_or(0);
            let kind = match props.device_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => Kind::Discrete,
                vk::PhysicalDeviceType::INTEGRATED_GPU => Kind::Integrated,
                vk::PhysicalDeviceType::VIRTUAL_GPU => Kind::Virtual,
                vk::PhysicalDeviceType::CPU => Kind::Cpu,
                _ => Kind::Other,
            };
            Device {
                index,
                name: props
                    .device_name_as_c_str()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                kind,
                memory: own,
            }
        })
        .collect();
    // SAFETY: nothing of the instance is used after this.
    unsafe { instance.destroy_instance(None) };
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(index: usize, name: &str, kind: Kind, gb: u64) -> Device {
        Device {
            index,
            name: name.to_owned(),
            kind,
            memory: gb * 1_000_000_000,
        }
    }

    /// A hybrid laptop: Intel's integrated GPU first, NVIDIA's discrete
    /// one, and Mesa's CPU Vulkan.
    fn hybrid() -> Vec<Device> {
        vec![
            gpu(0, "Intel(R) Graphics (RPL-P)", Kind::Integrated, 16),
            gpu(1, "NVIDIA GeForce RTX 4060 Laptop GPU", Kind::Discrete, 8),
            gpu(2, "llvmpipe (LLVM 19.1.7, 256 bits)", Kind::Cpu, 32),
        ]
    }

    #[test]
    fn a_discrete_gpu_first_then_an_integrated_one_then_the_cpu() {
        let Choice::Gpu { device, why } = choose(&hybrid(), &Wanted::Auto, LARGE_NEEDS) else {
            panic!("not a GPU");
        };
        assert_eq!(device.index, 1);
        assert_eq!(why, "a discrete GPU with 8.0 GB");
        // A discrete GPU too small: the integrated one.
        let mut small = hybrid();
        small[1].memory = 1_000_000_000;
        let Choice::Gpu { device, .. } = choose(&small, &Wanted::Auto, LARGE_NEEDS) else {
            panic!("not a GPU");
        };
        assert_eq!(device.kind, Kind::Integrated);
        // Only a CPU's Vulkan, or nothing: whisper.cpp's own CPU path.
        let lavapipe = vec![gpu(0, "llvmpipe", Kind::Cpu, 32)];
        assert!(matches!(
            choose(&lavapipe, &Wanted::Auto, LARGE_NEEDS),
            Choice::Cpu { .. }
        ));
        assert_eq!(
            choose(&[], &Wanted::Auto, LARGE_NEEDS),
            Choice::Cpu {
                why: "no Vulkan device".to_owned()
            }
        );
    }

    #[test]
    fn the_configuration_chooses_the_cpu_or_a_device_by_name() {
        assert_eq!(Wanted::of("auto"), Wanted::Auto);
        assert_eq!(Wanted::of(""), Wanted::Auto);
        assert_eq!(Wanted::of("cpu"), Wanted::Cpu);
        assert!(matches!(
            choose(&hybrid(), &Wanted::Cpu, LARGE_NEEDS),
            Choice::Cpu { .. }
        ));
        // By name, whatever its kind: lavapipe, to try the Vulkan path.
        let Choice::Gpu { device, .. } = choose(&hybrid(), &Wanted::of("LLVMPIPE"), LARGE_NEEDS)
        else {
            panic!("not chosen");
        };
        assert_eq!(device.index, 2);
        // A name no device has: as `auto`, saying so.
        let Choice::Gpu { device, why } = choose(&hybrid(), &Wanted::of("Radeon"), LARGE_NEEDS)
        else {
            panic!("not a GPU");
        };
        assert_eq!(device.index, 1);
        assert!(why.starts_with("no device named \"Radeon\""), "{why}");
    }
}
