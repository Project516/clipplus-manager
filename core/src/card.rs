//! The Sansa card over UDisks2. A card is a vfat filesystem labeled SANSA.
//! Changes arrive as D-Bus signals; nothing polls.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type as MessageType;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const DEST: &str = "org.freedesktop.UDisks2";
const ROOT: &str = "/org/freedesktop/UDisks2";
const BLOCK: &str = "org.freedesktop.UDisks2.Block";
const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
const DRIVE: &str = "org.freedesktop.UDisks2.Drive";
const LABEL: &str = "SANSA";
const STOCK_LABEL: &str = "SANSA CLIPP";
const SYSFS: &str = "/sys";

pub type Properties = HashMap<String, OwnedValue>;
pub type Objects = HashMap<OwnedObjectPath, HashMap<String, Properties>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardInfo {
    pub block: String,
    pub device: String,
    pub drive: Option<String>,
    pub mount: Option<PathBuf>,
}

fn string(props: &Properties, key: &str) -> Option<String> {
    String::try_from(props.get(key)?.try_clone().ok()?).ok()
}

fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    PathBuf::from(std::ffi::OsStr::from_bytes(&bytes[..end]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Internal,
    Card,
}

struct DriveInfo {
    vendor: String,
    model: String,
}

fn drive_info(objects: &Objects, block: &Properties) -> Option<DriveInfo> {
    let path = OwnedObjectPath::try_from(block.get("Drive")?.try_clone().ok()?).ok()?;
    let drive = objects.get(&path)?.get(DRIVE)?;
    Some(DriveInfo {
        vendor: string(drive, "Vendor").unwrap_or_default().trim().into(),
        model: string(drive, "Model").unwrap_or_default().trim().into(),
    })
}

/// The USB vendor and product id above a block device, from sysfs.
fn usb_id(sysfs: &Path, block: &Properties) -> Option<(String, String)> {
    let device = block
        .get("Device")
        .and_then(|v| Vec::<u8>::try_from(v.try_clone().ok()?).ok())?;
    let name = bytes_to_path(&device).file_name()?.to_owned();
    let top = std::fs::canonicalize(sysfs).ok()?;
    let node = std::fs::canonicalize(sysfs.join("class/block").join(name)).ok()?;
    node.ancestors()
        .take_while(|dir| *dir != top)
        .find_map(|dir| {
            let read = |file: &str| std::fs::read_to_string(dir.join(file)).ok();
            Some((
                read("idVendor")?.trim().into(),
                read("idProduct")?.trim().into(),
            ))
        })
}

fn is_clip_plus(sysfs: &Path, block: &Properties) -> bool {
    usb_id(sysfs, block).is_some_and(|(vendor, product)| vendor == "0781" && product == "74d1")
}

fn classify(
    objects: &Objects,
    block: &Properties,
    mount: Option<&Path>,
    sysfs: &Path,
    has_rockbox: &dyn Fn(&Path) -> bool,
) -> Option<Kind> {
    if string(block, "IdType")? != "vfat" {
        return None;
    }
    let label = string(block, "IdLabel").unwrap_or_default();
    let drive = drive_info(objects, block);
    let clip = is_clip_plus(sysfs, block);
    if clip && let Some(drive) = &drive {
        let rockbox = drive.vendor.eq_ignore_ascii_case("Rockbox");
        if rockbox && drive.model.eq_ignore_ascii_case("Internal Storage") {
            return Some(Kind::Internal);
        }
        if drive.model.eq_ignore_ascii_case("SD Card Slot") {
            return Some(Kind::Card);
        }
    }
    if label.eq_ignore_ascii_case(LABEL) {
        return Some(Kind::Card);
    }
    if clip && (label.eq_ignore_ascii_case(STOCK_LABEL) || mount.is_some_and(has_rockbox)) {
        return Some(Kind::Internal);
    }
    None
}

fn volume(path: &OwnedObjectPath, interfaces: &HashMap<String, Properties>) -> Option<CardInfo> {
    let block = interfaces.get(BLOCK)?;
    let filesystem = interfaces.get(FILESYSTEM)?;
    let device = block
        .get("PreferredDevice")
        .or_else(|| block.get("Device"))
        .and_then(|v| Vec::<u8>::try_from(v.try_clone().ok()?).ok())
        .map(|b| bytes_to_path(&b).to_string_lossy().into_owned())
        .unwrap_or_default();
    let drive = block
        .get("Drive")
        .and_then(|v| OwnedObjectPath::try_from(v.try_clone().ok()?).ok())
        .map(|p| p.as_str().to_string())
        .filter(|p| p != "/");
    let mount = filesystem
        .get("MountPoints")
        .and_then(|v| Vec::<Vec<u8>>::try_from(v.try_clone().ok()?).ok())
        .and_then(|points| points.first().map(|p| bytes_to_path(p)));
    Some(CardInfo {
        block: path.as_str().to_string(),
        device,
        drive,
        mount,
    })
}

/// The microSD card and the player's internal drive, when present.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Volumes {
    pub card: Option<CardInfo>,
    pub internal: Option<CardInfo>,
}

fn volumes_of(
    objects: &Objects,
    sysfs: &Path,
    has_rockbox: &dyn Fn(&Path) -> bool,
) -> Vec<(Kind, CardInfo)> {
    let mut found: Vec<(Kind, CardInfo)> = objects
        .iter()
        .filter_map(|(path, interfaces)| {
            let info = volume(path, interfaces)?;
            let kind = classify(
                objects,
                interfaces.get(BLOCK)?,
                info.mount.as_deref(),
                sysfs,
                has_rockbox,
            )?;
            Some((kind, info))
        })
        .collect();
    found.sort_by(|a, b| a.1.block.cmp(&b.1.block));
    found
}

fn first(found: &[(Kind, CardInfo)], want: Kind) -> Option<CardInfo> {
    found
        .iter()
        .find(|(kind, _)| *kind == want)
        .map(|(_, info)| info.clone())
}

/// The first vfat filesystem labeled SANSA, or on the "SD Card Slot" drive.
pub fn find_card(objects: &Objects, sysfs: &Path) -> Option<CardInfo> {
    first(&volumes_of(objects, sysfs, &|_| false), Kind::Card)
}

/// The player's internal drive. It must sit on USB device 0781:74d1, and
/// then the drive model, the stock label or the Rockbox files pick it.
pub fn find_internal(
    objects: &Objects,
    sysfs: &Path,
    has_rockbox: &dyn Fn(&Path) -> bool,
) -> Option<CardInfo> {
    first(&volumes_of(objects, sysfs, has_rockbox), Kind::Internal)
}

pub fn find_volumes(
    objects: &Objects,
    sysfs: &Path,
    has_rockbox: &dyn Fn(&Path) -> bool,
) -> Volumes {
    let found = volumes_of(objects, sysfs, has_rockbox);
    Volumes {
        card: first(&found, Kind::Card),
        internal: first(&found, Kind::Internal),
    }
}

/// Mounted filesystems on `drive` or on a drive with the same UDisks2
/// `SiblingId`, which marks drives that share one physical device.
fn sibling_filesystems(objects: &Objects, drive: &str) -> Vec<String> {
    let sibling = |path: &str| {
        objects
            .iter()
            .find(|(p, _)| p.as_str() == path)
            .and_then(|(_, ifaces)| string(ifaces.get(DRIVE)?, "SiblingId"))
            .filter(|id| !id.is_empty())
    };
    let wanted = sibling(drive);
    let mut found: Vec<String> = objects
        .iter()
        .filter_map(|(path, ifaces)| {
            let block = ifaces.get(BLOCK)?;
            let filesystem = ifaces.get(FILESYSTEM)?;
            let mounted = filesystem
                .get("MountPoints")
                .and_then(|v| Vec::<Vec<u8>>::try_from(v.try_clone().ok()?).ok())
                .is_some_and(|points| !points.is_empty());
            let owner = block
                .get("Drive")
                .and_then(|v| OwnedObjectPath::try_from(v.try_clone().ok()?).ok())?;
            let same =
                owner.as_str() == drive || (wanted.is_some() && sibling(owner.as_str()) == wanted);
            (mounted && same).then(|| path.as_str().to_string())
        })
        .collect();
    found.sort();
    found
}

/// The text of a D-Bus error, without the wrapping.
fn describe(error: zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(_, Some(text), _) if !text.is_empty() => text,
        zbus::Error::MethodError(name, _, _) => name.to_string(),
        other => other.to_string(),
    }
}

#[derive(Clone)]
pub struct Udisks {
    conn: Connection,
}

impl Udisks {
    pub fn connect() -> Result<Self, String> {
        Connection::system()
            .map(|conn| Self { conn })
            .map_err(|e| format!("No connection to UDisks2: {}", describe(e)))
    }

    fn proxy<'a>(&'a self, path: &'a str, interface: &'a str) -> Result<Proxy<'a>, String> {
        Proxy::new(&self.conn, DEST, path, interface).map_err(describe)
    }

    pub fn scan(&self) -> Result<Volumes, String> {
        let manager = self.proxy(ROOT, "org.freedesktop.DBus.ObjectManager")?;
        let objects: Objects = manager.call("GetManagedObjects", &()).map_err(describe)?;
        Ok(find_volumes(
            &objects,
            Path::new(SYSFS),
            &crate::player::is_rockbox_volume,
        ))
    }

    /// Mounts the card when needed and returns where it is mounted.
    pub fn mount(&self, card: &CardInfo) -> Result<PathBuf, String> {
        if let Some(mount) = &card.mount {
            return Ok(mount.clone());
        }
        let options: HashMap<&str, Value> = HashMap::new();
        let proxy = self.proxy(&card.block, FILESYSTEM)?;
        match proxy.call::<_, _, String>("Mount", &(options,)) {
            Ok(path) => Ok(PathBuf::from(path)),
            Err(e) => {
                let text = describe(e);
                // Another mounter won the race.
                let now = self.scan().ok().and_then(|v| {
                    [v.card, v.internal]
                        .into_iter()
                        .flatten()
                        .find(|c| c.block == card.block)
                });
                match now.and_then(|c| c.mount) {
                    Some(mount) => Ok(mount),
                    None => Err(text),
                }
            }
        }
    }

    /// Unmounts every volume of the player, then powers it off. UDisks2 refuses
    /// to power off a drive while a sibling drive on the same USB device (the
    /// internal storage and the microSD slot) still has a mounted filesystem.
    pub fn eject(&self, card: &CardInfo) -> Result<(), String> {
        let options = || HashMap::<&str, Value>::new();
        let manager = self.proxy(ROOT, "org.freedesktop.DBus.ObjectManager")?;
        let objects: Objects = manager.call("GetManagedObjects", &()).map_err(describe)?;
        let mut blocks = vec![card.block.clone()];
        if let Some(drive) = &card.drive {
            for block in sibling_filesystems(&objects, drive) {
                if !blocks.contains(&block) {
                    blocks.push(block);
                }
            }
        }
        for block in &blocks {
            let filesystem = self.proxy(block, FILESYSTEM)?;
            if let Err(e) = filesystem.call::<_, _, ()>("Unmount", &(options(),)) {
                let text = describe(e);
                if !text.contains("not mounted") && !text.contains("NotMounted") {
                    return Err(text);
                }
            }
        }
        if let Some(drive) = &card.drive {
            let proxy = self.proxy(drive, DRIVE)?;
            let can_power_off = proxy.get_property::<bool>("CanPowerOff").unwrap_or(false);
            if can_power_off {
                proxy
                    .call::<_, _, ()>("PowerOff", &(options(),))
                    .map_err(describe)?;
            }
        }
        Ok(())
    }

    /// Watches for the volumes on their own thread for the life of the app.
    /// `on_connect` gets every new UDisks2 connection; `on_change` gets the
    /// volumes now and whenever one appears, goes away, or is (un)mounted, and
    /// errors while UDisks2 is unreachable. Lost connections are retried.
    pub fn watch(
        on_connect: impl Fn(Udisks) + Send + 'static,
        mut on_change: impl FnMut(Result<Volumes, String>) + Send + 'static,
    ) {
        std::thread::spawn(move || {
            let mut last: Option<Volumes> = None;
            let mut delay = RETRY_MIN;
            loop {
                let outcome = Udisks::connect().and_then(|udisks| {
                    on_connect(udisks.clone());
                    let started = std::time::Instant::now();
                    let result = udisks.follow(&mut last, &mut on_change);
                    if started.elapsed() >= RETRY_MAX {
                        delay = RETRY_MIN;
                    }
                    result
                });
                if let Err(e) = outcome {
                    eprintln!("clipplus-manager: card watch: {e}");
                    last = None;
                    on_change(Err(e));
                }
                std::thread::sleep(delay);
                delay = (delay * 2).min(RETRY_MAX);
            }
        });
    }

    /// Reports card changes until the signal stream fails or ends.
    fn follow(
        &self,
        last: &mut Option<Volumes>,
        on_change: &mut impl FnMut(Result<Volumes, String>),
    ) -> Result<(), String> {
        let rule = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender(DEST)
            .map_err(describe)?
            .build();
        let signals =
            MessageIterator::for_match_rule(rule, &self.conn, Some(64)).map_err(describe)?;
        let mut report = |now| report_change(last, on_change, now);
        report(self.scan());
        for message in signals {
            let message = message.map_err(describe)?;
            if relevant(&message) {
                report(self.scan());
            }
        }
        Err("UDisks2 signal stream ended".into())
    }
}

fn report_change<T: Clone + PartialEq>(
    last: &mut Option<T>,
    on_change: &mut impl FnMut(Result<T, String>),
    now: Result<T, String>,
) {
    match now {
        Ok(value) if last.as_ref() == Some(&value) => {}
        Ok(value) => {
            *last = Some(value.clone());
            on_change(Ok(value));
        }
        Err(e) => {
            *last = None;
            on_change(Err(e));
        }
    }
}

const RETRY_MIN: std::time::Duration = std::time::Duration::from_secs(2);
const RETRY_MAX: std::time::Duration = std::time::Duration::from_secs(60);

fn relevant(message: &zbus::Message) -> bool {
    let header = message.header();
    match header.member().map(|m| m.as_str()) {
        Some("InterfacesAdded" | "InterfacesRemoved") => true,
        Some("PropertiesChanged") => message
            .body()
            .deserialize::<(String, Properties, Vec<String>)>()
            .is_ok_and(|(interface, _, _)| interface == FILESYSTEM || interface == BLOCK),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<'a, T: Into<Value<'a>>>(v: T) -> OwnedValue {
        OwnedValue::try_from(v.into()).unwrap()
    }

    fn object(
        label: &str,
        fs_type: &str,
        mount: Option<&str>,
        drive: &str,
    ) -> HashMap<String, Properties> {
        let mut block = Properties::new();
        block.insert("IdLabel".into(), value(label));
        block.insert("IdType".into(), value(fs_type));
        block.insert("Device".into(), value(b"/dev/sdb1\0".to_vec()));
        block.insert(
            "Drive".into(),
            value(zbus::zvariant::ObjectPath::try_from(drive).unwrap()),
        );
        let mut filesystem = Properties::new();
        let points: Vec<Vec<u8>> = mount
            .map(|m| vec![[m.as_bytes(), b"\0"].concat()])
            .unwrap_or_default();
        filesystem.insert("MountPoints".into(), value(points));
        HashMap::from([
            (BLOCK.to_string(), block),
            (FILESYSTEM.to_string(), filesystem),
        ])
    }

    fn objects(items: Vec<(&str, HashMap<String, Properties>)>) -> Objects {
        items
            .into_iter()
            .map(|(path, ifaces)| (OwnedObjectPath::try_from(path).unwrap(), ifaces))
            .collect()
    }

    fn drive_with_sibling(id: &str) -> HashMap<String, Properties> {
        let mut drive = Properties::new();
        drive.insert("SiblingId".into(), value(id));
        HashMap::from([(DRIVE.to_string(), drive)])
    }

    #[test]
    fn ejects_every_mounted_volume_of_the_same_player() {
        let all = objects(vec![
            ("/d/internal", drive_with_sibling("/sys/usb1/1-2")),
            ("/d/slot", drive_with_sibling("/sys/usb1/1-2")),
            ("/d/other", drive_with_sibling("/sys/usb1/1-3")),
            ("/o/sda", object("", "vfat", Some("/m/a"), "/d/internal")),
            ("/o/sdb1", object("SANSA", "vfat", Some("/m/b"), "/d/slot")),
            ("/o/sdc1", object("STICK", "vfat", Some("/m/c"), "/d/other")),
            ("/o/sdb2", object("X", "vfat", None, "/d/slot")),
        ]);
        assert_eq!(
            sibling_filesystems(&all, "/d/internal"),
            ["/o/sda", "/o/sdb1"]
        );
        assert_eq!(sibling_filesystems(&all, "/d/other"), ["/o/sdc1"]);
    }

    #[test]
    fn reports_the_same_card_after_a_scan_error() {
        let card = find_card(
            &objects(vec![("/o/b1", object("SANSA", "vfat", None, "/"))]),
            Path::new(NO_SYSFS),
        );
        let mut last = None;
        let mut reports = Vec::new();
        let mut record = |result| reports.push(result);
        report_change(&mut last, &mut record, Ok(card.clone()));
        report_change(&mut last, &mut record, Ok(card.clone()));
        report_change(&mut last, &mut record, Err("scan failed".into()));
        report_change(&mut last, &mut record, Ok(card));
        assert_eq!(reports.len(), 3);
        assert!(reports[1].is_err());
        assert!(reports[2].as_ref().unwrap().is_some());
    }

    #[test]
    fn finds_a_mounted_card() {
        let all = objects(vec![
            (
                "/org/freedesktop/UDisks2/block_devices/sda1",
                object("OTHER", "vfat", None, "/"),
            ),
            (
                "/org/freedesktop/UDisks2/block_devices/sdb1",
                object(
                    "SANSA",
                    "vfat",
                    Some("/run/media/<user>/SANSA"),
                    "/org/freedesktop/UDisks2/drives/D",
                ),
            ),
        ]);
        let card = find_card(&all, Path::new(NO_SYSFS)).unwrap();
        assert_eq!(card.device, "/dev/sdb1");
        assert_eq!(card.mount, Some(PathBuf::from("/run/media/<user>/SANSA")));
        assert_eq!(
            card.drive.as_deref(),
            Some("/org/freedesktop/UDisks2/drives/D")
        );
        assert!(card.block.ends_with("sdb1"));
    }

    #[test]
    fn unmounted_card_has_no_mount_point() {
        let all = objects(vec![("/o/b1", object("sansa", "vfat", None, "/"))]);
        let card = find_card(&all, Path::new(NO_SYSFS)).unwrap();
        assert_eq!(card.mount, None);
        assert_eq!(card.drive, None);
    }

    const DRIVE_PATH: &str = "/org/freedesktop/UDisks2/drives/D";
    const NO_SYSFS: &str = "/nonexistent-sysfs";

    fn drive_object(vendor: &str, model: &str) -> HashMap<String, Properties> {
        let mut drive = Properties::new();
        drive.insert("Vendor".into(), value(vendor));
        drive.insert("Model".into(), value(model));
        HashMap::from([(DRIVE.to_string(), drive)])
    }

    fn player(
        vendor: &str,
        model: &str,
        label: &str,
        mount: Option<&str>,
    ) -> Vec<(&'static str, HashMap<String, Properties>)> {
        vec![
            (DRIVE_PATH, drive_object(vendor, model)),
            ("/o/b1", object(label, "vfat", mount, DRIVE_PATH)),
        ]
    }

    /// A sysfs tree where `sdb1` sits under a USB device with this id.
    fn sysfs_with(vendor: &str, product: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let usb = dir.path().join("devices/usb1/1-3");
        let node = usb.join("1-3:1.0/host6/target6:0:0/6:0:0:0/block/sdb/sdb1");
        std::fs::create_dir_all(&node).unwrap();
        std::fs::write(usb.join("idVendor"), format!("{vendor}\n")).unwrap();
        std::fs::write(usb.join("idProduct"), format!("{product}\n")).unwrap();
        std::fs::create_dir_all(dir.path().join("class/block")).unwrap();
        std::os::unix::fs::symlink(&node, dir.path().join("class/block/sdb1")).unwrap();
        dir
    }

    fn clip() -> tempfile::TempDir {
        sysfs_with("0781", "74d1")
    }

    fn never(_: &Path) -> bool {
        false
    }

    #[test]
    fn rockbox_internal_drive_is_found_by_model_whatever_its_label() {
        let sysfs = clip();
        for label in ["", "CLIPPLUS", "My Music"] {
            let all = objects(player("Rockbox", "Internal Storage", label, None));
            let found = find_internal(&all, sysfs.path(), &never).unwrap();
            assert_eq!(found.drive.as_deref(), Some(DRIVE_PATH));
            assert_eq!(find_card(&all, sysfs.path()), None);
        }
    }

    #[test]
    fn rockbox_microsd_is_found_by_model() {
        let sysfs = clip();
        let all = objects(player("Rockbox", "SD Card Slot", "Songs", None));
        assert!(find_card(&all, sysfs.path()).is_some());
        assert_eq!(find_internal(&all, sysfs.path(), &never), None);
    }

    #[test]
    fn the_stock_label_finds_the_internal_drive() {
        let sysfs = clip();
        let all = objects(player("SanDisk", "Sansa Clip+", "SANSA CLIPP", None));
        assert!(find_internal(&all, sysfs.path(), &never).is_some());
        assert_eq!(find_card(&all, sysfs.path()), None);
    }

    #[test]
    fn a_mounted_clip_counts_when_its_files_say_rockbox() {
        let sysfs = clip();
        let mount = "/run/media/<user>/Anything";
        let all = objects(player("Generic", "Flash", "Anything", Some(mount)));
        assert_eq!(find_internal(&all, sysfs.path(), &never), None);
        let seen = std::cell::RefCell::new(Vec::new());
        let by_files = |path: &Path| {
            seen.borrow_mut().push(path.to_path_buf());
            true
        };
        let found = find_internal(&all, sysfs.path(), &by_files).unwrap();
        assert_eq!(found.mount, Some(PathBuf::from(mount)));
        assert_eq!(seen.borrow().as_slice(), [PathBuf::from(mount)]);
        let unmounted = objects(player("Generic", "Flash", "Anything", None));
        assert_eq!(find_internal(&unmounted, sysfs.path(), &|_| true), None);
    }

    #[test]
    fn a_drive_that_is_not_a_clip_plus_is_never_the_internal_drive() {
        let mount = "/run/media/<user>/Anything";
        let rockbox_files = |_: &Path| true;
        for sysfs in [
            sysfs_with("0781", "74d0"),
            sysfs_with("1234", "74d1"),
            sysfs_with("0781", "5567"),
        ] {
            for (vendor, model, label, mounted) in [
                ("Rockbox", "Internal Storage", "X", None),
                ("SanDisk", "Disk", "SANSA CLIPP", None),
                ("Generic", "Flash", "Anything", Some(mount)),
            ] {
                let all = objects(player(vendor, model, label, mounted));
                assert_eq!(
                    find_internal(&all, sysfs.path(), &rockbox_files),
                    None,
                    "{vendor} {model} {label}"
                );
            }
        }
        // No sysfs entry at all, such as an internal bus.
        let all = objects(player("Rockbox", "Internal Storage", "X", Some(mount)));
        assert_eq!(
            find_internal(&all, Path::new(NO_SYSFS), &rockbox_files),
            None
        );
    }

    #[test]
    fn the_sd_card_slot_model_needs_the_clip_plus_but_the_label_does_not() {
        let other = sysfs_with("1234", "0001");
        let slot = objects(player("Rockbox", "SD Card Slot", "Songs", None));
        assert_eq!(find_card(&slot, other.path()), None);
        let labeled = objects(player("Generic", "SD Reader", "SANSA", None));
        assert!(find_card(&labeled, other.path()).is_some());
    }

    #[test]
    fn the_sansa_label_stays_a_card_and_both_volumes_are_reported() {
        let sysfs = clip();
        let mut items = player("Rockbox", "Internal Storage", "X", None);
        items.push((
            "/org/freedesktop/UDisks2/drives/E",
            drive_object("Generic", "SD Reader"),
        ));
        items.push((
            "/o/b2",
            object("SANSA", "vfat", None, "/org/freedesktop/UDisks2/drives/E"),
        ));
        let volumes = find_volumes(&objects(items), sysfs.path(), &never);
        assert!(volumes.internal.unwrap().block.ends_with("b1"));
        assert!(volumes.card.unwrap().block.ends_with("b2"));
    }

    #[test]
    fn other_filesystems_and_labels_are_ignored() {
        let all = objects(vec![
            ("/o/a", object("SANSA", "ext4", None, "/")),
            ("/o/b", object("MUSIC", "vfat", None, "/")),
        ]);
        assert_eq!(find_card(&all, Path::new(NO_SYSFS)), None);
        assert_eq!(find_card(&Objects::new(), Path::new(NO_SYSFS)), None);
    }
}
