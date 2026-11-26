//! Managed copies of ffmpeg, yt-dlp and deno: where they are, which is newest,
//! how to fetch and verify a newer one, and how to swap it in.

use crate::fixer::Ffmpeg;
use crate::net;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tool {
    Ffmpeg,
    Ytdlp,
    Deno,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Ytdlp, Tool::Deno, Tool::Ffmpeg];

    pub fn key(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Ytdlp => "yt-dlp",
            Tool::Deno => "deno",
        }
    }

    fn files(self) -> &'static [&'static str] {
        match self {
            Tool::Ffmpeg => &["ffmpeg", "ffprobe"],
            Tool::Ytdlp => &["yt-dlp"],
            Tool::Deno => &["deno"],
        }
    }

    fn version_args(self) -> &'static [&'static str] {
        match self {
            Tool::Ffmpeg => &["-version"],
            _ => &["--version"],
        }
    }
}

// Versions -------------------------------------------------------------

fn parse_version(text: &str) -> Vec<u64> {
    text.trim_start_matches(['v', 'n'])
        .split(['.', '-'])
        .map(|part| {
            part.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

/// True when `candidate` is a higher dotted version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (mut a, mut b) = (parse_version(candidate), parse_version(current));
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}

// Checksum files --------------------------------------------------------

fn is_sha256(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Finds the hash for `name` in a `sha256sum`-style file. A file holding one
/// bare hash, or one hash with any name, also works when `name` is None.
pub fn find_sha(text: &str, name: Option<&str>) -> Option<String> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(hash), file) = (parts.next(), parts.next()) else {
            continue;
        };
        if !is_sha256(hash) {
            continue;
        }
        let file = file.map(|f| f.trim_start_matches('*'));
        match (name, file) {
            (Some(want), Some(got)) if want == got => return Some(hash.to_lowercase()),
            (None, _) => return Some(hash.to_lowercase()),
            _ => {}
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

fn is_dotted(s: &str) -> bool {
    s.split('.')
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The FFmpeg release in a stable-branch GPL asset name, such as `9.0.2` from
/// `ffmpeg-n9.0.2-982-g836df4e858-linux64-gpl-9.0.tar.xz`. A build exactly on
/// a tag has no count and hash. Master and shared builds give None.
fn ffmpeg_version(name: &str) -> Option<&str> {
    let (build, branch) = name
        .strip_prefix("ffmpeg-n")?
        .strip_suffix(".tar.xz")?
        .split_once("-linux64-gpl-")?;
    let mut parts = build.split('-');
    let version = parts.next()?;
    let git_ok = match (parts.next(), parts.next(), parts.next()) {
        (None, ..) => true,
        (Some(count), Some(hash), None) => {
            count.bytes().all(|b| b.is_ascii_digit()) && hash.starts_with('g')
        }
        _ => false,
    };
    (git_ok && is_dotted(version) && is_dotted(branch)).then_some(version)
}

#[derive(Debug, PartialEq, Eq)]
pub struct FfmpegBuild {
    pub version: String,
    pub name: String,
    pub url: String,
    pub sums_url: String,
}

/// The newest stable-branch GPL build in one BtbN release.
pub fn pick_ffmpeg(release: &Value) -> Option<FfmpegBuild> {
    let assets = release.get("assets")?.as_array()?;
    let link = |a: &Value| a.get("browser_download_url")?.as_str().map(str::to_string);
    let sums_url = assets
        .iter()
        .find(|a| a.get("name").and_then(Value::as_str) == Some("checksums.sha256"))
        .and_then(link)?;
    let mut best: Option<FfmpegBuild> = None;
    for asset in assets {
        let Some(name) = asset.get("name").and_then(Value::as_str) else {
            continue;
        };
        let (Some(version), Some(url)) = (ffmpeg_version(name), link(asset)) else {
            continue;
        };
        if best.as_ref().is_none_or(|b| is_newer(version, &b.version)) {
            best = Some(FfmpegBuild {
                version: version.to_string(),
                name: name.to_string(),
                url,
                sums_url: sums_url.clone(),
            });
        }
    }
    best
}

/// Daily builds change the commit count, not the release, so the release
/// version is what decides whether to download.
fn latest_ffmpeg(agent: &ureq::Agent) -> Result<Latest, String> {
    let releases = net::get_json(
        agent,
        "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases?per_page=5",
    )?;
    let build = releases
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| {
            r.get("tag_name")
                .and_then(Value::as_str)
                .is_some_and(|t| t.starts_with("autobuild-"))
        })
        .find_map(pick_ffmpeg)
        .ok_or_else(|| "ffmpeg: no stable build listed".to_string())?;
    let sha256 = find_sha(&net::get_text(agent, &build.sums_url)?, Some(&build.name))
        .ok_or_else(|| format!("ffmpeg: no checksum for {}", build.name))?;
    Ok(Latest {
        version: build.version,
        url: build.url,
        sha256,
    })
}

/// Version and asset links from a GitHub `releases/latest` answer.
pub fn release_assets(
    release: &Value,
    asset: &str,
    sums: &str,
) -> Option<(String, String, String)> {
    let url_of = |wanted: &str| {
        release
            .get("assets")?
            .as_array()?
            .iter()
            .find(|a| a.get("name").and_then(Value::as_str) == Some(wanted))
            .and_then(|a| a.get("browser_download_url")?.as_str())
            .map(str::to_string)
    };
    let tag = release.get("tag_name")?.as_str()?;
    Some((
        tag.trim_start_matches('v').to_string(),
        url_of(asset)?,
        url_of(sums)?,
    ))
}

pub fn fetch_latest(agent: &ureq::Agent, tool: Tool) -> Result<Latest, String> {
    let github = |repo: &str, asset: &str, sums: &str, sums_name: Option<&str>| {
        let release = net::get_json(
            agent,
            &format!("https://api.github.com/repos/{repo}/releases/latest"),
        )?;
        let (version, url, sums_url) = release_assets(&release, asset, sums)
            .ok_or_else(|| format!("{repo}: release has no {asset}"))?;
        let sha256 = find_sha(&net::get_text(agent, &sums_url)?, sums_name)
            .ok_or_else(|| format!("{repo}: no checksum for {asset}"))?;
        Ok(Latest {
            version,
            url,
            sha256,
        })
    };
    match tool {
        Tool::Ytdlp => github(
            "yt-dlp/yt-dlp",
            "yt-dlp_linux",
            "SHA2-256SUMS",
            Some("yt-dlp_linux"),
        ),
        Tool::Deno => github(
            "denoland/deno",
            "deno-x86_64-unknown-linux-gnu.zip",
            "deno-x86_64-unknown-linux-gnu.zip.sha256sum",
            None,
        ),
        Tool::Ffmpeg => latest_ffmpeg(agent),
    }
}

// Where the tools are ----------------------------------------------------

#[derive(Debug, Clone)]
pub struct Found {
    pub dir: PathBuf,
    pub version: String,
}

#[derive(Debug, Clone, Default)]
pub struct Resolved {
    found: BTreeMap<Tool, Found>,
}

fn read_versions(dir: &Path) -> BTreeMap<String, String> {
    crate::paths::read_json(&dir.join("versions.json"))
}

fn usable(dir: &Path, tool: Tool) -> bool {
    tool.files().iter().all(|name| {
        fs::metadata(dir.join(name))
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

impl Resolved {
    /// Picks, per tool, the higher version of the managed and the bundled copy.
    /// A tie goes to the managed copy.
    pub fn load(tools: &Path, bundled: Option<&Path>) -> Self {
        let mut found = BTreeMap::new();
        for tool in Tool::ALL {
            let candidates = [Some(tools), bundled]
                .into_iter()
                .flatten()
                .filter_map(|dir| {
                    let version = read_versions(dir).remove(tool.key())?;
                    usable(dir, tool).then(|| Found {
                        dir: dir.to_path_buf(),
                        version,
                    })
                });
            let mut best: Option<Found> = None;
            for candidate in candidates {
                if best
                    .as_ref()
                    .is_none_or(|b| is_newer(&candidate.version, &b.version))
                {
                    best = Some(candidate);
                }
            }
            if let Some(best) = best {
                found.insert(tool, best);
            }
        }
        Self { found }
    }

    pub fn get(&self, tool: Tool) -> Option<&Found> {
        self.found.get(&tool)
    }

    pub fn version(&self, tool: Tool) -> Option<&str> {
        self.get(tool).map(|f| f.version.as_str())
    }

    fn path(&self, tool: Tool) -> Option<PathBuf> {
        self.get(tool).map(|f| f.dir.join(tool.files()[0]))
    }

    pub fn ytdlp(&self) -> Option<PathBuf> {
        self.path(Tool::Ytdlp)
    }

    pub fn deno(&self) -> Option<PathBuf> {
        self.path(Tool::Deno)
    }

    pub fn ffmpeg_dir(&self) -> Option<PathBuf> {
        self.get(Tool::Ffmpeg).map(|f| f.dir.clone())
    }

    pub fn ffmpeg(&self) -> Option<Ffmpeg> {
        self.ffmpeg_dir().map(|dir| Ffmpeg {
            ffmpeg: dir.join("ffmpeg"),
            ffprobe: dir.join("ffprobe"),
        })
    }

    /// What a download needs that is not there yet.
    pub fn missing(&self) -> Vec<Tool> {
        Tool::ALL
            .into_iter()
            .filter(|t| !self.found.contains_key(t))
            .collect()
    }
}

/// The bundled tools of an AppImage, or a directory named by the environment.
pub fn bundled_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLIPPLUS_BUNDLED_TOOLS") {
        return Some(PathBuf::from(dir));
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.parent()?.join("lib/clipplus-manager/tools");
    dir.is_dir().then_some(dir)
}

// Fetching ---------------------------------------------------------------

#[derive(Debug)]
pub struct Staged {
    pub tool: Tool,
    pub version: String,
    pub dir: PathBuf,
}

fn extract_ffmpeg(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(liblzma::read::XzDecoder::new(io::BufReader::new(file)));
    let mut left = vec!["ffmpeg", "ffprobe"];
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let in_bin = path
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|d| d == "bin");
        if let Some(pos) = left.iter().position(|n| *n == name).filter(|_| in_bin) {
            entry.unpack(dest.join(name)).map_err(|e| e.to_string())?;
            left.swap_remove(pos);
            if left.is_empty() {
                return Ok(());
            }
        }
    }
    Err(format!("archive lacks {}", left.join(", ")))
}

fn extract_deno(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut entry = zip.by_name("deno").map_err(|e| e.to_string())?;
    let mut out = File::create(dest.join("deno")).map_err(|e| e.to_string())?;
    io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    Ok(())
}

fn check_runs(path: &Path, tool: Tool) -> Result<(), String> {
    const TEXT_FILE_BUSY: i32 = 26;
    let run = || {
        Command::new(path)
            .args(tool.version_args())
            .stdin(Stdio::null())
            .output()
    };
    // A child forked by another thread can briefly hold the just-written
    // file open, so exec fails with ETXTBSY until that child execs.
    let mut result = run();
    for _ in 0..20 {
        match &result {
            Err(e) if e.raw_os_error() == Some(TEXT_FILE_BUSY) => {
                std::thread::sleep(std::time::Duration::from_millis(25));
                result = run();
            }
            _ => break,
        }
    }
    let out = result.map_err(|e| format!("{}: {e}", path.display()))?;
    if out.status.success() && !out.stdout.is_empty() {
        Ok(())
    } else {
        Err(format!("{} does not run", path.display()))
    }
}

/// Downloads `latest` into a temp folder inside `tools`, verifies the checksum
/// and checks that the new binaries run. Nothing in use is touched.
pub fn stage(
    agent: &ureq::Agent,
    tool: Tool,
    latest: &Latest,
    tools: &Path,
) -> Result<Staged, String> {
    let dir = tools.join(format!(".stage-{}", tool.key()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        let download = dir.join("download");
        let got = net::download(agent, &latest.url, &download)?;
        if got != latest.sha256 {
            return Err(format!("{}: checksum mismatch", tool.key()));
        }
        match tool {
            Tool::Ytdlp => fs::rename(&download, dir.join("yt-dlp")).map_err(|e| e.to_string())?,
            Tool::Deno => extract_deno(&download, &dir)?,
            Tool::Ffmpeg => extract_ffmpeg(&download, &dir)?,
        }
        let _ = fs::remove_file(&download);
        for name in tool.files() {
            let path = dir.join(name);
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
            check_runs(&path, tool)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(Staged {
            tool,
            version: latest.version.clone(),
            dir,
        }),
        Err(e) => {
            let _ = fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

/// Moves a staged tool into `tools` and records its version. Each move is an
/// atomic rename.
pub fn install(staged: &Staged, tools: &Path) -> io::Result<()> {
    for name in staged.tool.files() {
        fs::rename(staged.dir.join(name), tools.join(name))?;
    }
    let mut versions = read_versions(tools);
    versions.insert(staged.tool.key().to_string(), staged.version.clone());
    crate::paths::write_json(&tools.join("versions.json"), &versions)?;
    let _ = fs::remove_dir_all(&staged.dir);
    Ok(())
}

pub fn clean_stages(tools: &Path) {
    for tool in Tool::ALL {
        let _ = fs::remove_dir_all(tools.join(format!(".stage-{}", tool.key())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn version_order() {
        assert!(is_newer("2026.10.26", "2026.9.30"));
        assert!(is_newer("v2.9.8", "2.9.7"));
        assert!(is_newer("9.0", "8.1"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("2026.10.26.1", "2026.10.26"));
        assert!(!is_newer("2.9.8", "v2.9.8"));
        assert!(!is_newer("1.0", "1.0.0"));
        assert!(!is_newer("8.1", "9.0"));
    }

    #[test]
    fn finds_hashes_in_sum_files() {
        let sums = format!("{A}  yt-dlp\n{B} *yt-dlp_linux\nnot a hash  other\n");
        assert_eq!(find_sha(&sums, Some("yt-dlp_linux")).as_deref(), Some(B));
        assert_eq!(find_sha(&sums, Some("yt-dlp")).as_deref(), Some(A));
        assert_eq!(find_sha(&sums, Some("missing")), None);
        assert_eq!(
            find_sha(&format!("{A}  deno.zip\n"), None).as_deref(),
            Some(A)
        );
        assert_eq!(find_sha(&format!("{A}\n"), None).as_deref(), Some(A));
        assert_eq!(find_sha("short  file", None), None);
    }

    #[test]
    fn reads_the_ffmpeg_release_from_asset_names() {
        let v = ffmpeg_version;
        assert_eq!(
            v("ffmpeg-n9.0.2-982-g836df4e858-linux64-gpl-9.0.tar.xz"),
            Some("9.0.2")
        );
        assert_eq!(v("ffmpeg-n9.0.2-linux64-gpl-9.0.tar.xz"), Some("9.0.2"));
        assert_eq!(
            v("ffmpeg-n10.1-3-gabc-linux64-gpl-10.1.tar.xz"),
            Some("10.1")
        );
        assert_eq!(v("ffmpeg-N-129308-g17de9f8597-linux64-gpl.tar.xz"), None);
        assert_eq!(
            v("ffmpeg-n9.0.2-1-gabc-linux64-gpl-shared-9.0.tar.xz"),
            None
        );
        assert_eq!(v("ffmpeg-n9.0.2-1-gabc-linux64-lgpl-9.0.tar.xz"), None);
        assert_eq!(v("ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz"), None);
        assert_eq!(v("ffmpeg-n9.1-dev-4-gabc-linux64-gpl-9.1.tar.xz"), None);
        assert_eq!(v("ffmpeg-n9.0.2-1-gabc-winarm64-gpl-9.0.zip"), None);
    }

    #[test]
    fn picks_the_newest_stable_gpl_ffmpeg() {
        let names = [
            "checksums.sha256",
            "ffmpeg-N-129308-g17de9f8597-linux64-gpl.tar.xz",
            "ffmpeg-n8.1.3-41-ga8029b6528-linux64-gpl-8.1.tar.xz",
            "ffmpeg-n9.0.2-982-g836df4e858-linux64-gpl-shared-9.0.tar.xz",
            "ffmpeg-n9.0.2-982-g836df4e858-linux64-gpl-9.0.tar.xz",
            "ffmpeg-n9.0.2-982-g836df4e858-linux64-lgpl-9.0.tar.xz",
        ];
        let release = serde_json::json!({
            "tag_name": "autobuild-2026-11-26-12-56",
            "assets": names.map(|n| serde_json::json!({
                "name": n,
                "browser_download_url": format!("https://x/{n}"),
            })),
        });
        let build = pick_ffmpeg(&release).unwrap();
        assert_eq!(build.version, "9.0.2");
        assert_eq!(build.name, names[4]);
        assert_eq!(build.url, format!("https://x/{}", names[4]));
        assert_eq!(build.sums_url, "https://x/checksums.sha256");

        let no_sums = serde_json::json!({"assets": [
            {"name": names[4], "browser_download_url": "https://x/a"}]});
        assert_eq!(pick_ffmpeg(&no_sums), None);
        assert_eq!(pick_ffmpeg(&serde_json::json!({"assets": []})), None);
    }

    #[test]
    fn a_new_ffmpeg_build_of_the_same_release_is_not_newer() {
        assert!(!is_newer("9.0.2", "9.0.2"));
        assert!(is_newer("9.0.3", "9.0.2"));
        assert!(is_newer("9.0.2", "9.0"));
        assert!(is_newer("9.1", "9.0.2"));
    }

    #[test]
    fn reads_release_assets() {
        let release: Value = serde_json::from_str(
            r#"{"tag_name":"v2.9.8","assets":[
                {"name":"deno.zip","browser_download_url":"https://x/deno.zip"},
                {"name":"deno.zip.sha256sum","browser_download_url":"https://x/sum"}]}"#,
        )
        .unwrap();
        let (version, url, sums) =
            release_assets(&release, "deno.zip", "deno.zip.sha256sum").unwrap();
        assert_eq!(
            (version.as_str(), url.as_str(), sums.as_str()),
            ("2.9.8", "https://x/deno.zip", "https://x/sum")
        );
        assert!(release_assets(&release, "other", "deno.zip.sha256sum").is_none());
    }

    fn fake_tool(dir: &Path, tool: Tool, version: &str) {
        fs::create_dir_all(dir).unwrap();
        for name in tool.files() {
            let path = dir.join(name);
            fs::write(&path, "#!/bin/sh\necho v\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut versions = read_versions(dir);
        versions.insert(tool.key().into(), version.into());
        crate::paths::write_json(&dir.join("versions.json"), &versions).unwrap();
    }

    #[test]
    fn higher_version_wins_and_ties_go_to_managed() {
        let root = tempfile::tempdir().unwrap();
        let (tools, bundled) = (root.path().join("tools"), root.path().join("bundled"));
        fake_tool(&tools, Tool::Ytdlp, "2026.10.01");
        fake_tool(&bundled, Tool::Ytdlp, "2026.10.26");
        fake_tool(&tools, Tool::Deno, "2.9.8");
        fake_tool(&bundled, Tool::Deno, "2.9.8");
        let resolved = Resolved::load(&tools, Some(&bundled));
        assert_eq!(resolved.ytdlp().unwrap(), bundled.join("yt-dlp"));
        assert_eq!(resolved.deno().unwrap(), tools.join("deno"));
        assert_eq!(resolved.missing(), vec![Tool::Ffmpeg]);
        assert!(resolved.ffmpeg().is_none());
    }

    #[test]
    fn invalid_copies_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        let (tools, bundled) = (root.path().join("tools"), root.path().join("bundled"));
        fake_tool(&tools, Tool::Deno, "9.9.9");
        fs::set_permissions(tools.join("deno"), fs::Permissions::from_mode(0o644)).unwrap();
        fake_tool(&bundled, Tool::Deno, "2.0.0");
        assert_eq!(
            Resolved::load(&tools, Some(&bundled)).version(Tool::Deno),
            Some("2.0.0")
        );
        assert!(Resolved::load(&tools, None).deno().is_none());
    }

    #[test]
    fn install_swaps_files_and_records_the_version() {
        let root = tempfile::tempdir().unwrap();
        let tools = root.path().join("tools");
        fake_tool(&tools, Tool::Ffmpeg, "8.1");
        let stage_dir = tools.join(".stage-ffmpeg");
        fake_tool(&stage_dir, Tool::Ffmpeg, "9.0");
        fs::write(stage_dir.join("ffmpeg"), "new").unwrap();
        let staged = Staged {
            tool: Tool::Ffmpeg,
            version: "9.0".into(),
            dir: stage_dir.clone(),
        };
        install(&staged, &tools).unwrap();
        assert_eq!(fs::read(tools.join("ffmpeg")).unwrap(), b"new");
        assert_eq!(read_versions(&tools)["ffmpeg"], "9.0");
        assert!(!stage_dir.exists());
    }

    #[test]
    fn extracts_ffmpeg_from_a_tar_xz() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.tar.xz");
        {
            let encoder = liblzma::write::XzEncoder::new(File::create(&archive).unwrap(), 1);
            let mut builder = tar::Builder::new(encoder);
            for (path, body) in [
                ("ffmpeg-n9.0.2-982-g836df4e858-linux64-gpl-9.0/LICENSE", "x"),
                (
                    "ffmpeg-n9.0.2-982-g836df4e858-linux64-gpl-9.0/bin/ffmpeg",
                    "ff",
                ),
                (
                    "ffmpeg-n9.0.2-982-g836df4e858-linux64-gpl-9.0/bin/ffplay",
                    "pl",
                ),
                (
                    "ffmpeg-n9.0.2-982-g836df4e858-linux64-gpl-9.0/bin/ffprobe",
                    "pr",
                ),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(body.len() as u64);
                header.set_mode(0o755);
                builder
                    .append_data(&mut header, path, body.as_bytes())
                    .unwrap();
            }
            builder
                .into_inner()
                .unwrap()
                .finish()
                .unwrap()
                .flush()
                .unwrap();
        }
        let out = dir.path().join("out");
        fs::create_dir(&out).unwrap();
        extract_ffmpeg(&archive, &out).unwrap();
        assert_eq!(fs::read(out.join("ffmpeg")).unwrap(), b"ff");
        assert_eq!(fs::read(out.join("ffprobe")).unwrap(), b"pr");
        assert!(!out.join("ffplay").exists());
    }

    #[test]
    fn extracts_deno_from_a_zip() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("deno.zip");
        {
            let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
            zip.start_file("deno", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"binary").unwrap();
            zip.finish().unwrap();
        }
        extract_deno(&archive, dir.path()).unwrap();
        assert_eq!(fs::read(dir.path().join("deno")).unwrap(), b"binary");
    }
}
