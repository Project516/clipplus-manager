//! Safe writes to the player's internal drive. Paths are compared the way
//! vfat does, only a few folders under `.rockbox` may change, and every
//! change checks that the same volume is still mounted.

use crate::transfer;
use std::fs::{self, File};
use std::io::{self, Read};
#[cfg(test)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

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

/// The mounted internal drive. The device id is taken when the job starts.
pub struct Drive {
    root: PathBuf,
    dev: u64,
    #[cfg(test)]
    relaxed: bool,
    #[cfg(test)]
    fail_after: std::cell::Cell<Option<usize>>,
}

impl Drive {
    /// Fails when nothing is mounted at `root`.
    pub fn open(root: &Path) -> Result<Self, String> {
        let dev = transfer::mount_device(root).map_err(|_| "The player is not mounted")?;
        Ok(Self {
            root: root.to_path_buf(),
            dev,
            #[cfg(test)]
            relaxed: false,
            #[cfg(test)]
            fail_after: std::cell::Cell::new(None),
        })
    }

    /// A plain folder standing in for the drive.
    #[cfg(test)]
    pub fn for_test(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            dev: fs::metadata(root).map_or(0, |m| m.dev()),
            relaxed: true,
            fail_after: std::cell::Cell::new(None),
        }
    }

    /// Makes the check fail after `n` more successful checks.
    #[cfg(test)]
    pub fn fail_after(&self, n: usize) {
        self.fail_after.set(Some(n));
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn free_space(&self) -> Option<u64> {
        transfer::free_space(&self.root)
    }

    fn current_device(&self) -> io::Result<u64> {
        #[cfg(test)]
        if self.relaxed {
            return fs::metadata(&self.root).map(|m| m.dev());
        }
        transfer::mount_device(&self.root)
    }

    /// Errors when the volume was unplugged or replaced since the job began.
    pub fn check(&self) -> Result<(), String> {
        #[cfg(test)]
        if let Some(n) = self.fail_after.get() {
            if n == 0 {
                return Err("The player changed while it was being written".into());
            }
            self.fail_after.set(Some(n - 1));
        }
        match self.current_device() {
            Ok(dev) if dev == self.dev => Ok(()),
            _ => Err("The player was unplugged or remounted while it was being written".into()),
        }
    }

    pub fn path(&self, rel: &RelPath) -> PathBuf {
        self.root.join(&rel.path)
    }

    /// Writes through a temp name, then renames. Creates the folders.
    pub fn write(&self, rel: &RelPath, data: &mut dyn Read) -> Result<(), String> {
        self.check()?;
        let dest = self.path(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", rel.text()))?;
        }
        let name = dest
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let temp = dest.with_file_name(format!(".{name}.part"));
        let copy = (|| -> Result<(), String> {
            let mut file = File::create(&temp).map_err(|e| e.to_string())?;
            io::copy(data, &mut file).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            self.check()?;
            fs::rename(&temp, &dest).map_err(|e| e.to_string())
        })();
        if copy.is_err() {
            let _ = fs::remove_file(&temp);
        }
        copy.map_err(|e| format!("{}: {e}", rel.text()))
    }

    /// Returns false when the file was not there or could not be removed.
    pub fn remove_file(&self, rel: &RelPath) -> Result<bool, String> {
        self.check()?;
        Ok(fs::remove_file(self.path(rel)).is_ok())
    }

    /// Removes a folder when it is empty. Other failures are ignored.
    pub fn remove_dir(&self, rel: &Path) -> Result<(), String> {
        self.check()?;
        let _ = fs::remove_dir(self.root.join(rel));
        Ok(())
    }

    /// Removes a folder and everything in it. Returns the file count.
    pub fn remove_tree(&self, rel: &Path) -> Result<usize, String> {
        let dir = self.root.join(rel);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.to_string()),
        };
        let mut count = 0;
        for entry in entries.flatten() {
            let child = rel.join(entry.file_name());
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                count += self.remove_tree(&child)?;
            } else {
                self.check()?;
                fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
                count += 1;
            }
        }
        self.check()?;
        fs::remove_dir(&dir).map_err(|e| e.to_string())?;
        Ok(count)
    }

    pub fn sync_dir(&self, dir: &Path) {
        if let Ok(file) = File::open(dir) {
            let _ = file.sync_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn writes_stop_when_the_volume_changes() {
        let dir = tempfile::tempdir().unwrap();
        let drive = Drive::for_test(dir.path());
        let rel = RelPath::parse(".rockbox/a").unwrap();
        // The first write makes two checks, before it starts and before the rename.
        drive.fail_after(1);
        assert!(drive.write(&rel, &mut &b"x"[..]).is_err());
        assert!(!dir.path().join(".rockbox/a").exists());
        assert!(!dir.path().join(".rockbox/.a.part").exists());
        let drive = Drive::for_test(dir.path());
        drive.write(&rel, &mut &b"x"[..]).unwrap();
        drive.fail_after(0);
        assert!(drive.remove_file(&rel).is_err());
        assert!(dir.path().join(".rockbox/a").exists());
        assert!(drive.remove_dir(Path::new(".rockbox")).is_err());
        assert!(drive.remove_tree(Path::new(".rockbox")).is_err());
        assert!(dir.path().join(".rockbox/a").exists());
    }
}
