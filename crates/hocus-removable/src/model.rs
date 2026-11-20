//! What the bar shows of UDisks2's objects: the drives a person plugs in
//! ([`Drive`]), and which of UDisks2's block devices are such drives
//! ([`counts`]).

use std::{collections::HashMap, path::PathBuf};

use zbus::{OwnedObjectPath, OwnedVariant, Variant};

/// An interface's properties, as UDisks2 says them.
pub type Props = HashMap<String, OwnedVariant>;
/// An object's interfaces and their properties.
pub type Interfaces = HashMap<String, Props>;

pub const BLOCK: &str = "org.freedesktop.UDisks2.Block";
pub const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
pub const ENCRYPTED: &str = "org.freedesktop.UDisks2.Encrypted";
pub const LOOP: &str = "org.freedesktop.UDisks2.Loop";
pub const DRIVE: &str = "org.freedesktop.UDisks2.Drive";

/// What a drive is, for its words ("Clé USB", "Disque", "Carte SD").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A USB stick: its medium said removable.
    Stick,
    /// Any other external disk (USB, Thunderbolt, FireWire).
    Disk,
    /// An SD or MMC card, in a reader of the computer's or a USB one.
    Card,
}

/// A filesystem a person plugged in, or an encrypted one locked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    /// Its number for this program's life, in the order drives came: the
    /// bar's rows and the bubbles' actions name it by that.
    pub serial: u32,
    /// Its label, else its drive's model or vendor; empty when none says.
    pub name: String,
    pub kind: Kind,
    /// Bytes.
    pub size: u64,
    /// Bytes free, known once mounted (statvfs).
    pub free: Option<u64>,
    /// Where it is mounted, if it is.
    pub mount: Option<PathBuf>,
    /// An encrypted one (LUKS): its passphrase opens it.
    pub encrypted: bool,
}

impl Drive {
    pub fn mounted(&self) -> bool {
        self.mount.is_some()
    }
}

/// What the worker tried, for a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    Open,
    Eject,
}

/// What the worker says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum News {
    /// The drives as they are now, in the order they came.
    Drives(Vec<Drive>),
    /// A drive plugged in while followed (not one there as it started).
    Plugged(Drive),
    /// Asked to open: mounted at `at`.
    Opened { drive: Drive, at: PathBuf },
    /// Asked to open, but encrypted and locked: its passphrase is asked
    /// elsewhere.
    Locked(Drive),
    /// Unmounted and powered off: it can be pulled out.
    Ejected(Drive),
    /// Asked to eject, but something still uses it (UDisks2's
    /// `DeviceBusy`): nothing was undone.
    Busy(Drive),
    /// Refused or failed (not allowed, an error).
    Failed { drive: Drive, doing: Doing },
}

/// A property of `props`.
pub fn get<T>(props: &Props, name: &str) -> Option<T>
where
    T: for<'a> TryFrom<&'a Variant<'a>>,
    for<'a> <T as TryFrom<&'a Variant<'a>>>::Error: Into<zbus::Error>,
{
    props.get(name)?.downcast_ref::<T>().ok()
}

fn text(props: &Props, name: &str) -> String {
    get::<String>(props, name).unwrap_or_default()
}

fn flag(props: &Props, name: &str) -> bool {
    get::<bool>(props, name).unwrap_or(false)
}

/// A property of `props` that is not a basic type (an array, a path).
pub fn owned<T: TryFrom<OwnedVariant>>(props: &Props, name: &str) -> Option<T> {
    T::try_from(props.get(name)?.try_clone().ok()?).ok()
}

/// An object path property: `None` for UDisks2's `/` (none).
pub fn path(props: &Props, name: &str) -> Option<String> {
    let path = owned::<OwnedObjectPath>(props, name)?;
    (path.as_str() != "/").then(|| path.as_str().to_owned())
}

/// Whether the block device `block` (its interfaces), on the drive whose
/// `Drive` properties are `drive`, is one a person plugged in: a
/// filesystem or an encrypted container (not a partition table, not
/// swap), on a drive UDisks2 says removable (USB, FireWire, SD), not a
/// loop device, not one UDisks2 hints is the system's or to be ignored.
/// An unlocked container's cleartext device is reached through its
/// container, not counted itself.
pub fn counts(block: &Interfaces, drive: Option<&Props>) -> bool {
    let Some(props) = block.get(BLOCK) else {
        return false;
    };
    if block.contains_key(LOOP) || flag(props, "HintIgnore") || flag(props, "HintSystem") {
        return false;
    }
    if !matches!(text(props, "IdUsage").as_str(), "filesystem" | "crypto") {
        return false;
    }
    if path(props, "CryptoBackingDevice").is_some() {
        return false;
    }
    let Some(drive) = drive else {
        return false;
    };
    flag(drive, "Removable")
        || matches!(
            text(drive, "ConnectionBus").as_str(),
            "usb" | "sdio" | "ieee1394"
        )
}

/// What the drive whose properties are `drive` is.
pub fn kind(drive: &Props) -> Kind {
    let card = |media: &str| media.starts_with("flash_sd") || media.starts_with("flash_mmc");
    let compatible = owned::<Vec<String>>(drive, "MediaCompatibility").unwrap_or_default();
    if card(&text(drive, "Media"))
        || compatible.iter().any(|m| card(m))
        || text(drive, "ConnectionBus") == "sdio"
    {
        Kind::Card
    } else if flag(drive, "MediaRemovable") {
        Kind::Stick
    } else {
        Kind::Disk
    }
}

/// A drive's name: the filesystem's label (`label`), else its drive's
/// model, else its vendor; empty when none says one.
pub fn name(label: &str, drive: &Props) -> String {
    [
        label.to_owned(),
        text(drive, "Model"),
        text(drive, "Vendor"),
    ]
    .into_iter()
    .map(|n| n.trim().to_owned())
    .find(|n| !n.is_empty())
    .unwrap_or_default()
}

/// The first of a filesystem's mount points (UDisks2 says them as bytes
/// ending with a NUL).
pub fn mount_point(filesystem: &Props) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let points = owned::<Vec<Vec<u8>>>(filesystem, "MountPoints")?;
    let first = points.into_iter().next()?;
    let bytes = first.strip_suffix(&[0]).unwrap_or(&first);
    (!bytes.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<'a>(value: impl Into<Variant<'a>>) -> OwnedVariant {
        OwnedVariant::try_from(value.into()).unwrap_or_else(|_| unreachable!())
    }

    fn props(values: Vec<(&str, OwnedVariant)>) -> Props {
        values.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    fn object_path(path: &str) -> OwnedVariant {
        value(zbus::ObjectPath::try_from(path).unwrap_or_else(|_| unreachable!()))
    }

    /// A block device: its usage, the device it is the cleartext of.
    fn block(usage: &str, backing: &str) -> Interfaces {
        let mut block = Interfaces::new();
        block.insert(
            BLOCK.to_owned(),
            props(vec![
                ("IdUsage", value(usage)),
                ("IdLabel", value("KINGSTON")),
                ("HintSystem", value(false)),
                ("HintIgnore", value(false)),
                ("CryptoBackingDevice", object_path(backing)),
            ]),
        );
        block
    }

    fn usb_stick() -> Props {
        props(vec![
            ("Removable", value(true)),
            ("MediaRemovable", value(true)),
            ("ConnectionBus", value("usb")),
            ("Vendor", value("Kingston")),
            ("Model", value("DataTraveler 3.0")),
        ])
    }

    #[test]
    fn a_stick_a_disk_and_a_card_count_and_internal_disks_loops_and_the_systems_do_not() {
        let stick = usb_stick();
        // A filesystem on a USB stick, an encrypted container on it.
        assert!(counts(&block("filesystem", "/"), Some(&stick)));
        assert!(counts(&block("crypto", "/"), Some(&stick)));
        // An external disk: on USB, its medium not removable.
        let disk = props(vec![
            ("Removable", value(false)),
            ("ConnectionBus", value("usb")),
        ]);
        assert!(counts(&block("filesystem", "/"), Some(&disk)));
        assert_eq!(kind(&disk), Kind::Disk);
        assert_eq!(kind(&stick), Kind::Stick);
        // An SD card in the computer's reader.
        let card = props(vec![
            ("Removable", value(true)),
            ("ConnectionBus", value("sdio")),
            ("Media", value("flash_sdhc")),
        ]);
        assert!(counts(&block("filesystem", "/"), Some(&card)));
        assert_eq!(kind(&card), Kind::Card);
        // The computer's own disk.
        let nvme = props(vec![
            ("Removable", value(false)),
            ("ConnectionBus", value("")),
        ]);
        assert!(!counts(&block("filesystem", "/"), Some(&nvme)));
        // The partition table, swap, a block without a drive.
        assert!(!counts(&block("", "/"), Some(&stick)));
        assert!(!counts(&block("other", "/"), Some(&stick)));
        assert!(!counts(&block("filesystem", "/"), None));
        // An unlocked container's cleartext: through its container only.
        assert!(!counts(
            &block("filesystem", "/org/freedesktop/UDisks2/block_devices/sdb1"),
            Some(&stick)
        ));
        // A loop device (a snap, an image mounted by hand).
        let mut looped = block("filesystem", "/");
        looped.insert(LOOP.to_owned(), Props::new());
        assert!(!counts(&looped, Some(&stick)));
        // What UDisks2 hints is the system's, or to be ignored.
        for hint in ["HintSystem", "HintIgnore"] {
            let mut hinted = block("filesystem", "/");
            if let Some(b) = hinted.get_mut(BLOCK) {
                b.insert(hint.to_owned(), value(true));
            }
            assert!(!counts(&hinted, Some(&stick)), "{hint}");
        }
    }

    #[test]
    fn its_name_is_its_label_else_its_model_and_its_mount_point_is_read_from_bytes() {
        let stick = usb_stick();
        assert_eq!(name("KINGSTON", &stick), "KINGSTON");
        assert_eq!(name("  ", &stick), "DataTraveler 3.0");
        assert_eq!(
            name("", &props(vec![("Vendor", value("SanDisk "))])),
            "SanDisk"
        );
        assert_eq!(name("", &Props::new()), "");
        let fs = props(vec![(
            "MountPoints",
            value(vec![b"/media/alex/KINGSTON\0".to_vec()]),
        )]);
        assert_eq!(
            mount_point(&fs),
            Some(PathBuf::from("/media/alex/KINGSTON"))
        );
        let none = props(vec![("MountPoints", value(Vec::<Vec<u8>>::new()))]);
        assert_eq!(mount_point(&none), None);
    }
}
