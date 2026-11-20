//! UDisks2, faked on the fakes' private bus (`fakes.rs`), never the
//! machine's: the computer's own disk (the system's, never shown), a loop
//! device (a snap's), and a USB stick plugged in when the test says, its
//! one filesystem labelled "KINGSTON", 32 GB.
//!
//! It behaves as UDisks2 does where the shell looks: its object manager
//! at `/org/freedesktop/UDisks2` says what comes and goes
//! (`InterfacesAdded` and `…Removed`); `Filesystem.Mount` mounts the stick
//! in a folder of the test's and says `MountPoints` changed;
//! `Filesystem.Unmount` unmounts it, or fails with UDisks2's `DeviceBusy`
//! while the test holds it busy; `Drive.PowerOff` takes the stick and its
//! filesystem away. What the shell asked is recorded, for the tests to
//! compare.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

use zbus::{Connection, OwnedObjectPath, OwnedVariant, Variant, connection::Builder};

pub const UDISKS: &str = "org.freedesktop.UDisks2";
const ROOT: &str = "/org/freedesktop/UDisks2";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";

const STICK_DRIVE: &str = "/org/freedesktop/UDisks2/drives/Kingston_DataTraveler_3_0_408D5C1A";
const STICK_WHOLE: &str = "/org/freedesktop/UDisks2/block_devices/sdb";
const STICK: &str = "/org/freedesktop/UDisks2/block_devices/sdb1";
const NVME_DRIVE: &str = "/org/freedesktop/UDisks2/drives/Samsung_SSD_980_S64";
const NVME: &str = "/org/freedesktop/UDisks2/block_devices/nvme0n1p2";
const LOOP: &str = "/org/freedesktop/UDisks2/block_devices/loop0";

/// The stick's size, as its box says.
pub const STICK_SIZE: u64 = 32_000_000_000;

/// What the shell asked, by the block's or the drive's last part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Mount(String),
    Unmount(String),
    PowerOff(String),
}

#[derive(Debug, Default)]
struct State {
    calls: Vec<Call>,
    /// Where the stick is mounted, if it is.
    mounted: Option<PathBuf>,
    /// Its unmount fails as in use.
    busy: bool,
    /// Where it is mounted when asked.
    media: PathBuf,
}

struct Inner {
    state: Mutex<State>,
    connection: OnceLock<Connection>,
}

type Shared = Arc<Inner>;

fn state<T>(shared: &Shared, with: impl FnOnce(&mut State) -> T) -> T {
    with(&mut shared.state.lock().unwrap_or_else(PoisonError::into_inner))
}

fn last(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

fn path(path: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(path).unwrap_or_default()
}

pub struct Udisks {
    shared: Shared,
    runtime: tokio::runtime::Handle,
}

impl Udisks {
    /// Serves UDisks2 on the bus at `address`, the computer's disk and a
    /// loop device there; the stick is mounted under `media` when asked.
    pub fn start(
        runtime: &tokio::runtime::Runtime,
        address: &str,
        media: PathBuf,
    ) -> anyhow::Result<Self> {
        let shared: Shared = Arc::new(Inner {
            state: Mutex::new(State {
                media,
                ..State::default()
            }),
            connection: OnceLock::new(),
        });
        let address = address.to_owned();
        let served = shared.clone();
        runtime.block_on(async move {
            let connection = Builder::address(address.as_str())
                .name(UDISKS)
                .serve_at(ROOT, zbus::fdo::ObjectManager)
                .build()
                .await?;
            let _ = served.connection.set(connection.clone());
            let server = connection.object_server();
            server
                .at(
                    NVME_DRIVE,
                    Drive {
                        removable: false,
                        bus: String::new(),
                        model: "Samsung SSD 980".to_owned(),
                        shared: served.clone(),
                    },
                )
                .await?;
            server
                .at(
                    NVME,
                    Block {
                        drive: path(NVME_DRIVE),
                        usage: "filesystem".to_owned(),
                        label: String::new(),
                        size: 500_000_000_000,
                        system: true,
                    },
                )
                .await?;
            server.at(LOOP, Loop).await?;
            server
                .at(
                    LOOP,
                    Block {
                        drive: path("/"),
                        usage: "filesystem".to_owned(),
                        label: String::new(),
                        size: 60_000_000,
                        system: false,
                    },
                )
                .await?;
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(Self {
            shared,
            runtime: runtime.handle().clone(),
        })
    }

    /// The stick plugged in: its drive, its partition table, then its
    /// filesystem.
    pub fn plug(&self) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        self.runtime.block_on(async move {
            let Some(connection) = shared.connection.get() else {
                anyhow::bail!("UDisks2 is not served");
            };
            let server = connection.object_server();
            server
                .at(
                    STICK_DRIVE,
                    Drive {
                        removable: true,
                        bus: "usb".to_owned(),
                        model: "DataTraveler 3.0".to_owned(),
                        shared: shared.clone(),
                    },
                )
                .await?;
            server
                .at(
                    STICK_WHOLE,
                    Block {
                        drive: path(STICK_DRIVE),
                        usage: String::new(),
                        label: String::new(),
                        size: STICK_SIZE,
                        system: false,
                    },
                )
                .await?;
            server
                .at(
                    STICK,
                    Block {
                        drive: path(STICK_DRIVE),
                        usage: "filesystem".to_owned(),
                        label: "KINGSTON".to_owned(),
                        size: STICK_SIZE,
                        system: false,
                    },
                )
                .await?;
            server
                .at(
                    STICK,
                    Filesystem {
                        shared: shared.clone(),
                    },
                )
                .await?;
            Ok(())
        })
    }

    /// The stick's unmount fails as in use (`busy`), or works again.
    pub fn hold(&self, busy: bool) {
        state(&self.shared, |s| s.busy = busy);
    }

    /// What the shell asked, in order.
    pub fn calls(&self) -> Vec<Call> {
        state(&self.shared, |s| s.calls.clone())
    }

    /// Whether the stick is there (not powered off).
    pub fn plugged(&self) -> bool {
        let shared = self.shared.clone();
        self.runtime.block_on(async move {
            let Some(connection) = shared.connection.get() else {
                return false;
            };
            connection
                .object_server()
                .interface::<_, Drive>(STICK_DRIVE)
                .await
                .is_ok()
        })
    }
}

/// `PropertiesChanged` for the stick's filesystem's mount points.
async fn mount_points_changed(shared: &Shared) -> zbus::Result<()> {
    let Some(connection) = shared.connection.get() else {
        return Ok(());
    };
    let points = state(shared, |s| points(s.mounted.as_ref()));
    let map: HashMap<&str, Variant<'_>> = [("MountPoints", Variant::from(points))].into();
    connection
        .emit_signal(
            None::<&str>,
            STICK,
            PROPERTIES,
            "PropertiesChanged",
            &(FILESYSTEM, map, Vec::<&str>::new()),
        )
        .await
}

fn points(mounted: Option<&PathBuf>) -> Vec<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    mounted
        .map(|m| {
            let mut bytes = m.as_os_str().as_bytes().to_vec();
            bytes.push(0);
            vec![bytes]
        })
        .unwrap_or_default()
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.UDisks2.Error")]
enum Refusal {
    #[zbus(error)]
    ZBus(zbus::Error),
    DeviceBusy(String),
    Failed(String),
}

struct Drive {
    removable: bool,
    bus: String,
    model: String,
    shared: Shared,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Drive")]
impl Drive {
    #[zbus(property)]
    fn removable(&self) -> bool {
        self.removable
    }
    #[zbus(property)]
    fn media_removable(&self) -> bool {
        self.removable
    }
    #[zbus(property)]
    fn connection_bus(&self) -> String {
        self.bus.clone()
    }
    #[zbus(property)]
    fn vendor(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn model(&self) -> String {
        self.model.clone()
    }
    #[zbus(property)]
    fn media(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn media_compatibility(&self) -> Vec<String> {
        Vec::new()
    }
    #[zbus(property)]
    fn can_power_off(&self) -> bool {
        self.removable
    }

    /// The stick goes: its filesystem, its partition table, itself;
    /// aside, after the answer, as the kernel takes the device away.
    fn power_off(&self, _options: HashMap<String, OwnedVariant>) {
        state(&self.shared, |s| {
            s.calls.push(Call::PowerOff(last(STICK_DRIVE)))
        });
        let Some(connection) = self.shared.connection.get().cloned() else {
            return;
        };
        tokio::spawn(async move {
            let server = connection.object_server();
            let _ = server.remove::<Filesystem, _>(STICK).await;
            let _ = server.remove::<Block, _>(STICK).await;
            let _ = server.remove::<Block, _>(STICK_WHOLE).await;
            let _ = server.remove::<Drive, _>(STICK_DRIVE).await;
        });
    }
}

struct Block {
    drive: OwnedObjectPath,
    usage: String,
    label: String,
    size: u64,
    system: bool,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Block")]
impl Block {
    #[zbus(property)]
    fn drive(&self) -> OwnedObjectPath {
        self.drive.clone()
    }
    #[zbus(property)]
    fn id_usage(&self) -> String {
        self.usage.clone()
    }
    #[zbus(property)]
    fn id_label(&self) -> String {
        self.label.clone()
    }
    #[zbus(property)]
    fn size(&self) -> u64 {
        self.size
    }
    #[zbus(property)]
    fn hint_system(&self) -> bool {
        self.system
    }
    #[zbus(property)]
    fn hint_ignore(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn crypto_backing_device(&self) -> OwnedObjectPath {
        path("/")
    }
}

/// A loop device's mark.
struct Loop;

#[zbus::interface(name = "org.freedesktop.UDisks2.Loop")]
impl Loop {
    #[zbus(property)]
    fn backing_file(&self) -> Vec<u8> {
        b"/var/lib/snapd/snaps/core22.snap\0".to_vec()
    }
}

struct Filesystem {
    shared: Shared,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Filesystem")]
impl Filesystem {
    #[zbus(property)]
    fn mount_points(&self) -> Vec<Vec<u8>> {
        state(&self.shared, |s| points(s.mounted.as_ref()))
    }

    async fn mount(&self, _options: HashMap<String, OwnedVariant>) -> Result<String, Refusal> {
        let at = state(&self.shared, |s| {
            s.calls.push(Call::Mount(last(STICK)));
            let at = s.media.join("KINGSTON");
            s.mounted = Some(at.clone());
            at
        });
        std::fs::create_dir_all(&at).map_err(|e| Refusal::Failed(e.to_string()))?;
        mount_points_changed(&self.shared).await?;
        Ok(at.display().to_string())
    }

    async fn unmount(&self, _options: HashMap<String, OwnedVariant>) -> Result<(), Refusal> {
        let busy = state(&self.shared, |s| {
            s.calls.push(Call::Unmount(last(STICK)));
            if !s.busy {
                s.mounted = None;
            }
            s.busy
        });
        if busy {
            return Err(Refusal::DeviceBusy(
                "Error unmounting /dev/sdb1: target is busy".to_owned(),
            ));
        }
        mount_points_changed(&self.shared).await?;
        Ok(())
    }
}
