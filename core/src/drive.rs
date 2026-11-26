//! Safe writes to the player's internal drive. Paths are compared the way
//! vfat does, only a few folders under `.rockbox` may change, and every
//! change goes through the directory handle opened when the job started.

use crate::transfer;
use rustix::fd::OwnedFd;
use rustix::fs::{
    Advice, AtFlags, CWD, Dir, FileType, Mode, OFlags, fadvise, fstatvfs, fsync, mkdirat, openat,
    renameat, statat, unlinkat,
};
use rustix::io::Errno;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// A path without `.`, `..` or empty parts, lowercased because vfat ignores case.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(Vec<String>);

/// A relative path with its comparison key. `path` keeps the original case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelPath {
    pub key: Key,
    pub path: PathBuf,
}

impl RelPath {
    /// Takes `/` or `\` separated names, as in zips and install logs.
    pub fn parse(name: &str) -> Result<Self, String> {
        let bad = || format!("The download holds an unsafe path: {name}");
        let text = name.replace('\\', "/");
        let mut parts = Vec::new();
        for part in text.split('/') {
            let odd = part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with(['.', ' '])
                || part.contains(|c: char| c.is_control() || "<>:\"|?*".contains(c));
            if odd {
                return Err(bad());
            }
            parts.push(part);
        }
        Ok(Self {
            key: Key(parts.iter().map(|p| p.to_ascii_lowercase()).collect()),
            path: parts.iter().collect(),
        })
    }

    /// Slash separated, as written to the install log.
    pub fn text(&self) -> String {
        self.path
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }
}

impl Key {
    pub fn of(name: &str) -> Self {
        RelPath::parse(name)
            .map(|p| p.key)
            .unwrap_or(Self(Vec::new()))
    }
}

/// Folders a kind of download may write to, as lowercase paths.
#[derive(Debug, Clone, Copy)]
pub struct Scope(&'static [&'static str]);

pub const BUILD: Scope = Scope(&[".rockbox"]);
pub const FONTS: Scope = Scope(&[".rockbox/fonts"]);
pub const VOICE: Scope = Scope(&[".rockbox/langs"]);
// What the Clip+ themes on the theme server hold.
pub const THEME: Scope = Scope(&[
    ".rockbox/themes",
    ".rockbox/wps",
    ".rockbox/fonts",
    ".rockbox/icons",
    ".rockbox/backdrops",
    ".rockbox/fmpresets",
]);

/// The player's own settings and data. No download or log entry may touch them.
fn protected(parts: &[String]) -> bool {
    let Some(name) = parts.get(1) else {
        return false;
    };
    matches!(
        name.as_str(),
        "config.cfg"
            | "config.cfg.new"
            | "config.cfg.old"
            | "fixed.cfg"
            | ".resume.cfg"
            | ".playlist_control"
            | "rbutil.log"
            | ".rbutil.log.part"
            | "playlist_catalog"
    ) || name.starts_with("database_")
        || name.ends_with(".bmark")
}

impl Scope {
    pub fn allows(&self, key: &Key) -> bool {
        let parts = &key.0;
        !protected(parts)
            && self.0.iter().any(|root| {
                let root: Vec<&str> = root.split('/').collect();
                parts.len() > root.len() && parts.iter().zip(&root).all(|(a, b)| a == b)
            })
    }
}

/// The mounted internal drive. All work goes through the directory handle
/// opened when the job starts, so a volume mounted later on the same path is
/// never touched. The device id check only gives an early "player went away".
pub struct Drive {
    dir: OwnedFd,
    root: PathBuf,
    dev: u64,
    lost: AtomicBool,
    #[cfg(test)]
    relaxed: bool,
    #[cfg(test)]
    fail_after: std::cell::Cell<Option<usize>>,
    #[cfg(test)]
    fail_sync: std::cell::Cell<bool>,
}

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_suffix() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(TEMP_COUNTER.fetch_add(1, Ordering::Relaxed));
    format!("{:08x}", hasher.finish() & 0xffff_ffff)
}

fn parts(rel: &Path) -> Vec<&OsStr> {
    rel.components().map(|c| c.as_os_str()).collect()
}

fn text(error: Errno) -> String {
    io::Error::from(error).to_string()
}

impl Drive {
    /// Fails when nothing is mounted at `root`. Keeps the root open until dropped.
    pub fn open(root: &Path) -> Result<Self, String> {
        let dev = transfer::mount_device(root).map_err(|_| "The player is not mounted")?;
        let fd = openat(
            CWD,
            root,
            DIR_FLAGS.difference(OFlags::NOFOLLOW),
            Mode::empty(),
        )
        .map_err(|e| format!("The player is not mounted: {}", text(e)))?;
        let file = File::from(fd);
        if file.metadata().map_err(|e| e.to_string())?.dev() != dev {
            return Err("The player changed while it was being opened".into());
        }
        Ok(Self::new(root, file.into(), dev, false))
    }

    fn new(root: &Path, dir: OwnedFd, dev: u64, _relaxed: bool) -> Self {
        Self {
            dir,
            root: root.to_path_buf(),
            dev,
            lost: AtomicBool::new(false),
            #[cfg(test)]
            relaxed: _relaxed,
            #[cfg(test)]
            fail_after: std::cell::Cell::new(None),
            #[cfg(test)]
            fail_sync: std::cell::Cell::new(false),
        }
    }

    /// A plain folder standing in for the drive.
    #[cfg(test)]
    pub fn for_test(root: &Path) -> Self {
        let file = File::open(root).unwrap();
        let dev = file.metadata().map_or(0, |m| m.dev());
        Self::new(root, file.into(), dev, true)
    }

    /// Makes every folder flush fail.
    #[cfg(test)]
    pub fn fail_sync(&self) {
        self.fail_sync.set(true);
    }

    /// Makes the check fail after `n` more successful checks.
    #[cfg(test)]
    pub fn fail_after(&self, n: usize) {
        self.fail_after.set(Some(n));
    }

    fn current_device(&self) -> io::Result<u64> {
        #[cfg(test)]
        if self.relaxed {
            return std::fs::metadata(&self.root).map(|m| m.dev());
        }
        transfer::mount_device(&self.root)
    }

    /// True once a check failed. Nothing may touch the drive after that,
    /// not even to clean up.
    fn lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }

    /// Errors when the player went away since the job began.
    pub fn check(&self) -> Result<(), String> {
        const GONE: &str = "The player was unplugged or remounted while it was being written";
        if self.lost() {
            return Err(GONE.into());
        }
        #[cfg(test)]
        if let Some(n) = self.fail_after.get() {
            if n == 0 {
                self.lost.store(true, Ordering::SeqCst);
                return Err(GONE.into());
            }
            self.fail_after.set(Some(n - 1));
        }
        match self.current_device() {
            Ok(dev) if dev == self.dev => Ok(()),
            _ => {
                self.lost.store(true, Ordering::SeqCst);
                Err(GONE.into())
            }
        }
    }

    /// Opens the folder one component at a time, never following a symlink.
    fn dir_of(&self, names: &[&OsStr], create: bool) -> Result<OwnedFd, Errno> {
        let mut current = openat(&self.dir, ".", DIR_FLAGS, Mode::empty())?;
        for name in names {
            if create {
                match mkdirat(&current, *name, Mode::from_bits_truncate(0o755)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(e) => return Err(e),
                }
            }
            current = openat(&current, *name, DIR_FLAGS, Mode::empty())?;
        }
        Ok(current)
    }

    pub fn free_space(&self) -> Option<u64> {
        fstatvfs(&self.dir)
            .ok()
            .map(|s| s.f_bavail.saturating_mul(s.f_frsize))
    }

    pub fn read(&self, rel: &RelPath) -> io::Result<Vec<u8>> {
        let names = parts(&rel.path);
        let (name, parents) = names
            .split_last()
            .ok_or_else(|| io::Error::other("empty path"))?;
        let parent = self.dir_of(parents, false)?;
        let fd = openat(
            &parent,
            *name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let mut file = File::from(fd);
        // Drop cached pages so a read after a write checks the device.
        let _ = fadvise(&file, 0, None, Advice::DontNeed);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    pub fn is_dir(&self, rel: &Path) -> bool {
        self.dir_of(&parts(rel), false).is_ok()
    }

    /// Writes through a unique temp name, then renames. Creates the folders.
    /// Removes only the temp file it made, and only while the player is still there.
    pub fn write(&self, rel: &RelPath, data: &mut dyn Read) -> Result<(), String> {
        self.check()?;
        let fail = |e: String| format!("{}: {e}", rel.text());
        let names = parts(&rel.path);
        let (name, parents) = names
            .split_last()
            .ok_or_else(|| fail("empty path".into()))?;
        let parent = self.dir_of(parents, true).map_err(|e| fail(text(e)))?;
        let (temp, mut file) = create_temp(&parent, name).map_err(fail)?;
        let result = (|| -> Result<(), String> {
            io::copy(data, &mut file).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            self.check()?;
            renameat(&parent, &temp, &parent, *name).map_err(text)?;
            // vfat writes the renamed entry with size 0 until the inode is flushed.
            file.sync_all().map_err(|e| e.to_string())
        })();
        if result.is_err() && !self.lost() {
            let _ = unlinkat(&parent, &temp, AtFlags::empty());
        }
        result.map_err(fail)
    }

    /// Renames a file, replacing the target.
    pub fn rename(&self, from: &RelPath, to: &RelPath) -> Result<(), String> {
        self.check()?;
        let (from_names, to_names) = (parts(&from.path), parts(&to.path));
        let (Some((from_name, from_dir)), Some((to_name, to_dir))) =
            (from_names.split_last(), to_names.split_last())
        else {
            return Err("empty path".into());
        };
        let from_parent = self
            .dir_of(from_dir, false)
            .map_err(|e| format!("{}: {}", from.text(), text(e)))?;
        let to_parent = self
            .dir_of(to_dir, false)
            .map_err(|e| format!("{}: {}", to.text(), text(e)))?;
        renameat(&from_parent, *from_name, &to_parent, *to_name)
            .map_err(|e| format!("{} to {}: {}", from.text(), to.text(), text(e)))
    }

    /// Flushes a file. Call it after `rename`: vfat writes the renamed entry
    /// with size 0 until the file itself is flushed.
    pub fn sync_file(&self, rel: &RelPath) -> Result<(), String> {
        self.check()?;
        let fail = |e: Errno| format!("{}: {}", rel.text(), text(e));
        let names = parts(&rel.path);
        let (name, parents) = names.split_last().ok_or("empty path")?;
        let parent = self.dir_of(parents, false).map_err(fail)?;
        let fd = openat(
            &parent,
            *name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(fail)?;
        fsync(&fd).map_err(fail)
    }

    /// Returns false when the file was not there or could not be removed.
    pub fn remove_file(&self, rel: &RelPath) -> Result<bool, String> {
        self.check()?;
        let names = parts(&rel.path);
        let Some((name, parents)) = names.split_last() else {
            return Ok(false);
        };
        let Ok(parent) = self.dir_of(parents, false) else {
            return Ok(false);
        };
        Ok(unlinkat(&parent, *name, AtFlags::empty()).is_ok())
    }

    /// Removes a folder when it is empty. Other failures are ignored.
    pub fn remove_dir(&self, rel: &Path) -> Result<(), String> {
        self.check()?;
        let names = parts(rel);
        if let Some((name, parents)) = names.split_last()
            && let Ok(parent) = self.dir_of(parents, false)
        {
            let _ = unlinkat(&parent, *name, AtFlags::REMOVEDIR);
        }
        Ok(())
    }

    /// Removes a folder and everything in it, without following symlinks.
    /// Returns the file count.
    pub fn remove_tree(&self, rel: &Path) -> Result<usize, String> {
        let names = parts(rel);
        let Some((name, parents)) = names.split_last() else {
            return Ok(0);
        };
        match self.dir_of(parents, false) {
            Ok(parent) => self.remove_tree_in(&parent, name),
            Err(Errno::NOENT | Errno::NOTDIR | Errno::LOOP) => Ok(0),
            Err(e) => Err(text(e)),
        }
    }

    fn remove_tree_in(&self, parent: &OwnedFd, name: &OsStr) -> Result<usize, String> {
        let dir = match openat(parent, name, DIR_FLAGS, Mode::empty()) {
            Ok(dir) => dir,
            Err(Errno::NOENT | Errno::NOTDIR | Errno::LOOP) => return Ok(0),
            Err(e) => return Err(text(e)),
        };
        let mut children = Vec::new();
        for entry in Dir::read_from(&dir).map_err(text)? {
            let entry = entry.map_err(text)?;
            let child = OsStr::from_bytes(entry.file_name().to_bytes()).to_owned();
            if child == "." || child == ".." {
                continue;
            }
            let is_dir = match entry.file_type() {
                FileType::Directory => true,
                FileType::Unknown => statat(&dir, &child, AtFlags::SYMLINK_NOFOLLOW)
                    .is_ok_and(|s| FileType::from_raw_mode(s.st_mode) == FileType::Directory),
                _ => false,
            };
            children.push((child, is_dir));
        }
        let mut count = 0;
        for (child, is_dir) in children {
            if is_dir {
                count += self.remove_tree_in(&dir, &child)?;
            } else {
                self.check()?;
                unlinkat(&dir, &child, AtFlags::empty()).map_err(text)?;
                count += 1;
            }
        }
        self.check()?;
        unlinkat(parent, name, AtFlags::REMOVEDIR).map_err(text)?;
        Ok(count)
    }

    /// Flushes a folder. The empty path is the drive root.
    pub fn sync_dir(&self, rel: &Path) -> Result<(), String> {
        #[cfg(test)]
        if self.fail_sync.get() {
            return Err("flush failed".into());
        }
        let dir = self.dir_of(&parts(rel), false).map_err(text)?;
        fsync(&dir).map_err(text)
    }
}

fn create_temp(parent: &OwnedFd, name: &OsStr) -> Result<(OsString, File), String> {
    for _ in 0..16 {
        let mut temp = OsString::from(".");
        temp.push(name);
        temp.push(format!(".{}.part", temp_suffix()));
        let flags =
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        match openat(parent, &temp, flags, Mode::from_bits_truncate(0o644)) {
            Ok(fd) => return Ok((temp, File::from(fd))),
            Err(Errno::EXIST) => {}
            Err(e) => return Err(text(e)),
        }
    }
    Err("no free temporary name".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn key(name: &str) -> Key {
        RelPath::parse(name).unwrap().key
    }

    #[test]
    fn paths_compare_the_way_vfat_does() {
        assert_eq!(key(".rockbox/rockbox.sansa"), key(".ROCKBOX/ROCKBOX.SANSA"));
        assert_eq!(key(".rockbox\\wps\\a.bmp"), key(".rockbox/wps/A.BMP"));
        assert_ne!(key(".rockbox/a"), key(".rockbox/b"));
        let parsed = RelPath::parse(".Rockbox\\Wps\\A.bmp").unwrap();
        assert_eq!(parsed.text(), ".Rockbox/Wps/A.bmp");
    }

    #[test]
    fn odd_paths_are_refused() {
        for name in [
            "",
            "/",
            "/etc/x",
            "a//b",
            "a/./b",
            ".rockbox/./rockbox.sansa",
            "../x",
            "a/../b",
            "a/b/",
            "C:/x",
            "a/b.",
            "a/b ",
            "a/b*",
            "a/b\u{7}",
        ] {
            assert!(RelPath::parse(name).is_err(), "{name:?}");
        }
        assert!(RelPath::parse(".rockbox/wps/a.r.i._(x)/y.bmp").is_ok());
    }

    #[test]
    fn scopes_cover_only_their_folders() {
        assert!(BUILD.allows(&key(".rockbox/rocks/a.rock")));
        assert!(BUILD.allows(&key(".ROCKBOX/bookmark.ignore")));
        assert!(!BUILD.allows(&key("Tracks/user.mp3")));
        assert!(!BUILD.allows(&key("clppa.bin")));
        assert!(!BUILD.allows(&key(".rockbox")));
        assert!(FONTS.allows(&key(".rockbox/fonts/x.fnt")));
        assert!(!FONTS.allows(&key(".rockbox/rocks/x.rock")));
        assert!(!FONTS.allows(&key(".rockbox/fonts")));
        assert!(VOICE.allows(&key(".rockbox/langs/english.voice")));
        assert!(!VOICE.allows(&key(".rockbox/fonts/x.fnt")));
        for ok in [
            "themes/a.cfg",
            "wps/a/b.bmp",
            "fonts/a.fnt",
            "icons/a.bmp",
            "backdrops/a.bmp",
            "fmpresets/a.fmr",
        ] {
            assert!(THEME.allows(&key(&format!(".rockbox/{ok}"))), "{ok}");
        }
        assert!(!THEME.allows(&key(".rockbox/rocks/a.rock")));
        assert!(!THEME.allows(&key(".rockbox/rockbox.sansa")));
    }

    #[test]
    fn the_players_own_data_is_never_in_scope() {
        for name in [
            "config.cfg",
            "CONFIG.CFG",
            "fixed.cfg",
            ".resume.cfg",
            ".playlist_control",
            "database_idx.tcd",
            "database_state.tcd",
            "rbutil.log",
            "playlist_catalog/mine.m3u",
            "most-recent.bmark",
        ] {
            assert!(!BUILD.allows(&key(&format!(".rockbox/{name}"))), "{name}");
        }
    }

    #[test]
    fn a_missing_mount_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        // A plain folder shares its parent's device, so it is not a mount.
        assert!(Drive::open(dir.path()).is_err());
        assert!(Drive::open(Path::new("/nonexistent/drive")).is_err());
    }

    fn rel(name: &str) -> RelPath {
        RelPath::parse(name).unwrap()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut found: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    }

    #[test]
    fn writes_stop_when_the_volume_changes() {
        let dir = tempfile::tempdir().unwrap();
        let drive = Drive::for_test(dir.path());
        // The first write makes two checks, before it starts and before the rename.
        drive.fail_after(1);
        assert!(drive.write(&rel(".rockbox/a"), &mut &b"x"[..]).is_err());
        assert!(!dir.path().join(".rockbox/a").exists());

        let drive = Drive::for_test(dir.path());
        drive.write(&rel(".rockbox/a"), &mut &b"x"[..]).unwrap();
        drive.fail_after(0);
        assert!(drive.remove_file(&rel(".rockbox/a")).is_err());
        assert!(dir.path().join(".rockbox/a").exists());
        assert!(drive.remove_dir(Path::new(".rockbox")).is_err());
        assert!(drive.remove_tree(Path::new(".rockbox")).is_err());
        assert!(dir.path().join(".rockbox/a").exists());
    }

    #[test]
    fn a_volume_mounted_later_on_the_same_path_is_never_written() {
        let parent = tempfile::tempdir().unwrap();
        let mount = parent.path().join("mount");
        fs::create_dir(&mount).unwrap();
        let drive = Drive::for_test(&mount);
        fs::write(mount.join("keep"), "old volume").unwrap();
        // Another volume takes the path. It is on the same device here, so the
        // device id check cannot tell.
        fs::rename(&mount, parent.path().join("old")).unwrap();
        fs::create_dir(&mount).unwrap();
        fs::write(mount.join("theirs"), "new volume").unwrap();
        drive.check().unwrap();

        drive.write(&rel(".rockbox/a"), &mut &b"x"[..]).unwrap();
        assert_eq!(drive.read(&rel("keep")).unwrap(), b"old volume");
        assert!(drive.remove_file(&rel("keep")).unwrap());
        assert!(parent.path().join("old/.rockbox/a").exists());
        assert!(!parent.path().join("old/keep").exists());
        assert_eq!(names(&mount), ["theirs"]);
        assert!(drive.read(&rel("theirs")).is_err());
    }

    #[test]
    fn symlinks_on_the_drive_are_never_followed() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("music.mp3"), "song").unwrap();
        let dir = tempfile::tempdir().unwrap();
        symlink(outside.path(), dir.path().join(".rockbox")).unwrap();
        let drive = Drive::for_test(dir.path());

        assert!(drive.write(&rel(".rockbox/a"), &mut &b"x"[..]).is_err());
        assert!(!drive.remove_file(&rel(".rockbox/music.mp3")).unwrap());
        assert_eq!(drive.remove_tree(Path::new(".rockbox")).unwrap(), 0);
        assert!(drive.read(&rel(".rockbox/music.mp3")).is_err());
        assert!(!drive.is_dir(Path::new(".rockbox")));
        assert_eq!(names(outside.path()), ["music.mp3"]);

        let inner = tempfile::tempdir().unwrap();
        let tree = dir.path().join("tree");
        fs::create_dir_all(&tree).unwrap();
        fs::write(tree.join("own"), "x").unwrap();
        symlink(inner.path(), tree.join("link")).unwrap();
        fs::write(inner.path().join("other"), "y").unwrap();
        assert_eq!(drive.remove_tree(Path::new("tree")).unwrap(), 2);
        assert!(!tree.exists());
        assert_eq!(names(inner.path()), ["other"]);
    }

    #[test]
    fn nothing_is_cleaned_up_after_the_player_went_away() {
        let dir = tempfile::tempdir().unwrap();
        let drive = Drive::for_test(dir.path());
        drive.fail_after(1);
        assert!(drive.write(&rel(".rockbox/a"), &mut &b"x"[..]).is_err());
        let left = names(&dir.path().join(".rockbox"));
        assert_eq!(left.len(), 1, "{left:?}");
        assert!(left[0].starts_with(".a.") && left[0].ends_with(".part"));
        // After that, every call fails without touching the drive.
        assert!(drive.write(&rel(".rockbox/b"), &mut &b"x"[..]).is_err());
        assert!(drive.remove_file(&rel(".rockbox/a")).is_err());
        assert_eq!(names(&dir.path().join(".rockbox")), left);
    }

    #[test]
    fn temp_files_never_replace_an_existing_file() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("read failed"))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let rockbox = dir.path().join(".rockbox");
        fs::create_dir(&rockbox).unwrap();
        for old in [".a.part", ".a.00000000.part"] {
            fs::write(rockbox.join(old), "mine").unwrap();
        }
        let drive = Drive::for_test(dir.path());
        assert!(drive.write(&rel(".rockbox/a"), &mut Broken).is_err());
        assert_eq!(names(&rockbox), [".a.00000000.part", ".a.part"]);
        drive.write(&rel(".rockbox/a"), &mut &b"new"[..]).unwrap();
        assert_eq!(names(&rockbox), [".a.00000000.part", ".a.part", "a"]);
        assert_eq!(fs::read(rockbox.join(".a.part")).unwrap(), b"mine");
        assert_eq!(fs::read(rockbox.join(".a.00000000.part")).unwrap(), b"mine");
        assert_eq!(fs::read(rockbox.join("a")).unwrap(), b"new");
    }
}
