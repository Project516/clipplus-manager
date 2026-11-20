//! A managed local slskd process and its blocking API.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};
use tungstenite::{Message, client::IntoClientRequest};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PeerFile {
    pub username: String,
    pub filename: String,
    pub size: u64,
    pub length: Option<u64>,
    pub bitrate: Option<u64>,
    // Saved queues from older versions have no peer fields.
    #[serde(default)]
    pub free_slot: bool,
    #[serde(default)]
    pub queue_length: u64,
    #[serde(default)]
    pub upload_speed: u64,
}

fn peer_order(a: &PeerFile, b: &PeerFile) -> Ordering {
    b.free_slot
        .cmp(&a.free_slot)
        .then(a.queue_length.cmp(&b.queue_length))
}

fn is_mp3(file: &PeerFile) -> bool {
    file.filename
        .rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("mp3"))
}

// Same order as rank() in web/src/soulseek.js. A peer with no free slot or a
// long queue often never starts the download.
fn file_order(a: &PeerFile, b: &PeerFile) -> Ordering {
    peer_order(a, b)
        .then(is_mp3(b).cmp(&is_mp3(a)))
        .then(b.upload_speed.cmp(&a.upload_speed))
}

fn album_order(a: &PeerFile, b: &PeerFile) -> Ordering {
    peer_order(a, b).then(b.upload_speed.cmp(&a.upload_speed))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub id: String,
    pub username: String,
    pub destination: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    pub password: String,
}

/// The saved account, or a new random one saved first. Soulseek creates an
/// account the first time an unused name logs in.
pub fn account(path: &Path) -> Result<Account, String> {
    match fs::read(path) {
        Ok(bytes) => {
            return serde_json::from_slice(&bytes)
                .map_err(|e| format!("Soulseek account file {}: {e}", path.display()));
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("Soulseek account file: {e}"));
        }
        Err(_) => {}
    }
    let account = Account {
        username: format!("clipplus{}", &random_hex()?[..10]),
        password: random_hex()?,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("Soulseek account file: {e}"))?;
    file.write_all(&serde_json::to_vec_pretty(&account).map_err(|e| e.to_string())?)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("Soulseek account file: {e}"))?;
    Ok(account)
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransferStatus {
    Active { progress: f64 },
    Complete(PathBuf),
    Failed(String),
}

pub struct Slskd {
    child: Mutex<Child>,
    enqueue_lock: Mutex<()>,
    output_closed: Mutex<mpsc::Receiver<Result<(), String>>>,
    agent: ureq::Agent,
    base: String,
    key: String,
    downloads: PathBuf,
}

fn random_hex() -> Result<String, String> {
    let mut bytes = [0u8; 24];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

impl Slskd {
    /// Starts slskd and waits for its Soulseek login. Call from a worker thread.
    pub fn start(
        executable: &Path,
        data_dir: &Path,
        username: &str,
        password: &str,
    ) -> Result<Self, String> {
        if username.trim().is_empty() || password.is_empty() {
            return Err("Enter a Soulseek username and password".into());
        }
        fs::create_dir_all(data_dir).map_err(|e| format!("slskd directory: {e}"))?;
        fs::set_permissions(data_dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        // The app keeps the queue. slskd's own records would make it resume old transfers
        // outside their folders, and its volatile mode loses tables between connections.
        match fs::remove_dir_all(data_dir.join("data")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("slskd data: {e}"));
            }
            _ => {}
        }
        let port = free_port("127.0.0.1:0")?;
        // Another Soulseek client may hold slskd's default listen port.
        let listen_port = free_port("0.0.0.0:0")?;
        let key = random_hex()?;
        let mut command = Command::new(executable);
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("SLSKD_") {
                command.env_remove(name);
            }
        }
        // SAFETY: only prctl runs after fork and before exec.
        unsafe {
            command.pre_exec(|| {
                Ok(rustix::process::set_parent_process_death_signal(Some(
                    rustix::process::Signal::TERM,
                ))?)
            });
        }
        let mut child = command
            .env("SLSKD_APP_DIR", data_dir)
            .env("SLSKD_SLSK_USERNAME", username)
            .env("SLSKD_SLSK_PASSWORD", password)
            .env("SLSKD_HTTP_IP_ADDRESS", "127.0.0.1")
            .env("SLSKD_HTTP_PORT", port.to_string())
            .env("SLSKD_SLSK_LISTEN_PORT", listen_port.to_string())
            .env("SLSKD_NO_HTTPS", "true")
            .env("SLSKD_API_KEY", &key)
            .env("SLSKD_NO_VERSION_CHECK", "true")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start slskd: {e}"))?;
        let stdout = child.stdout.take().ok_or("Could not read slskd output")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut logged_in = false;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if !logged_in && line.contains("Logged in to the Soulseek server") {
                    logged_in = true;
                    let _ = tx.send(Ok(()));
                }
                if line.contains("Application terminated unexpectedly")
                    || (!logged_in && line.contains("Failed to log in"))
                {
                    let _ = tx.send(Err("Soulseek login failed".to_string()));
                }
            }
        });
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(Ok(())) => {}
            other => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match other {
                    Ok(Err(e)) => e,
                    _ => "Could not connect to Soulseek".into(),
                });
            }
        }
        Ok(Self {
            child: Mutex::new(child),
            enqueue_lock: Mutex::new(()),
            output_closed: Mutex::new(rx),
            agent: crate::net::agent(),
            base: format!("http://127.0.0.1:{port}"),
            key,
            downloads: data_dir.join("downloads"),
        })
    }

    fn request(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        let url = format!("{}{path}", self.base);
        let result = match method {
            "GET" => self.agent.get(&url).header("X-API-Key", &self.key).call(),
            "POST" => self
                .agent
                .post(&url)
                .header("X-API-Key", &self.key)
                .header("Content-Type", "application/json")
                .send(body.unwrap_or(Value::Null).to_string()),
            "DELETE" => self
                .agent
                .delete(&url)
                .header("X-API-Key", &self.key)
                .call(),
            _ => return Err("Unsupported request".into()),
        };
        let mut response = result.map_err(|e| format!("slskd request failed: {e}"))?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("slskd response: {e}"))?;
        if text.is_empty() {
            Ok(Value::Null)
        } else {
            serde_json::from_str(&text).map_err(|e| format!("slskd response: {e}"))
        }
    }

    /// Searches peers without periodically polling slskd.
    pub fn search(&self, query: &str) -> Result<Vec<PeerFile>, String> {
        let url = self.base.replacen("http://", "ws://", 1) + "/hub/search";
        let mut request = url.into_client_request().map_err(|e| e.to_string())?;
        request.headers_mut().insert(
            "X-API-Key",
            self.key.parse().map_err(|_| "Invalid API key")?,
        );
        let port = self.base.rsplit(':').next().ok_or("Invalid slskd URL")?;
        let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
            .map_err(|e| format!("slskd connection: {e}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(90)))
            .map_err(|e| e.to_string())?;
        let (mut socket, _) = tungstenite::client(request, stream)
            .map_err(|e| format!("slskd search connection: {e}"))?;
        socket
            .send(Message::Text(
                "{\"protocol\":\"json\",\"version\":1}\u{1e}".into(),
            ))
            .map_err(|e| e.to_string())?;
        // Wait for the SignalR handshake before creating the search.
        let handshake = socket
            .read()
            .map_err(|e| format!("slskd search handshake: {e}"))?;
        if !handshake
            .to_text()
            .map_err(|e| e.to_string())?
            .starts_with("{}\u{1e}")
        {
            return Err("slskd search handshake failed".into());
        }
        // slskd documents searchTimeout in seconds, but Soulseek.NET reads it as milliseconds.
        let search = self.request(
            "POST",
            "/api/v0/searches",
            Some(json!({"searchText": query, "searchTimeout": 15000, "responseLimit": 50})),
        )?;
        let id = search
            .get("id")
            .and_then(Value::as_str)
            .ok_or("slskd returned no search ID")?;
        let mut last_ping = Instant::now();
        loop {
            let message = socket.read().map_err(|e| format!("slskd search: {e}"))?;
            if last_ping.elapsed() >= Duration::from_secs(10) {
                socket
                    .send(Message::Text("{\"type\":6}\u{1e}".into()))
                    .map_err(|e| format!("slskd search: {e}"))?;
                last_ping = Instant::now();
            }
            if let Message::Text(text) = message {
                for record in text.split('\u{1e}').filter(|s| !s.is_empty()) {
                    let Ok(event) = serde_json::from_str::<Value>(record) else {
                        continue;
                    };
                    let Some(item) = event
                        .get("arguments")
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                    else {
                        continue;
                    };
                    if event.get("target").and_then(Value::as_str) != Some("UPDATE")
                        || item.get("id").and_then(Value::as_str) != Some(id)
                        || item.get("endedAt").is_none_or(Value::is_null)
                    {
                        continue;
                    }
                    let state = item.get("state").and_then(Value::as_str).unwrap_or("");
                    if state.contains("Errored") || state.contains("Cancelled") {
                        return Err(format!("Soulseek search {state}"));
                    }
                    let responses =
                        self.request("GET", &format!("/api/v0/searches/{id}/responses"), None)?;
                    return Ok(parse_results(&responses));
                }
            }
        }
    }

    pub fn directory(&self, username: &str, directory: &str) -> Result<Vec<PeerFile>, String> {
        let response = self.request(
            "POST",
            &format!("/api/v0/users/{}/directory", encode(username)),
            Some(json!({"directory": directory})),
        )?;
        let files = parse_directory(&response, username, directory);
        if files.is_empty() {
            return Err("No audio files found in this folder".into());
        }
        Ok(files)
    }

    pub fn enqueue(&self, file: &PeerFile) -> Result<Transfer, String> {
        let _lock = self
            .enqueue_lock
            .lock()
            .map_err(|_| "Soulseek queue lock failed")?;
        let destination = random_hex()?;
        let response = self.request("POST", "/api/v0/transfers/downloads/batches", Some(json!({"username":file.username,"files":[{"filename":file.filename,"size":file.size}],"options":{"destination":destination}})))?;
        if let Some(failure) = response
            .get("failures")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
        {
            return Err(failure
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Could not queue download")
                .to_string());
        }
        let id = response
            .pointer("/batch/transfers/0/id")
            .and_then(Value::as_str)
            .ok_or("slskd returned no transfer ID")?
            .to_string();
        Ok(Transfer {
            id,
            username: file.username.clone(),
            destination,
            size: file.size,
        })
    }

    pub fn transfer(&self, transfer: &Transfer) -> Result<TransferStatus, String> {
        if !valid_component(&transfer.destination) || !valid_component(&transfer.id) {
            return Err("Invalid transfer ID".into());
        }
        let response = self.request(
            "GET",
            &format!(
                "/api/v0/transfers/downloads/{}/{}",
                encode(&transfer.username),
                transfer.id
            ),
            None,
        )?;
        let state = response.get("state").and_then(Value::as_str).unwrap_or("");
        if state.contains("Completed") && !state.contains("Succeeded") {
            return Ok(TransferStatus::Failed(
                response
                    .get("exception")
                    .and_then(Value::as_str)
                    .unwrap_or(state)
                    .to_string(),
            ));
        }
        if state.contains("Succeeded") {
            if let Some(error) = response
                .get("exception")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                return Ok(TransferStatus::Failed(error.to_string()));
            }
            if let Some(path) = self.completed_file(transfer) {
                return Ok(TransferStatus::Complete(path));
            }
        }
        Ok(TransferStatus::Active {
            progress: response
                .get("percentComplete")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                / 100.0,
        })
    }

    pub fn completed_file(&self, transfer: &Transfer) -> Option<PathBuf> {
        if !valid_component(&transfer.destination) {
            return None;
        }
        fs::read_dir(self.downloads.join(&transfer.destination))
            .ok()?
            .flatten()
            .find_map(|entry| {
                (entry.file_type().is_ok_and(|t| t.is_file())
                    && entry.metadata().is_ok_and(|m| m.len() == transfer.size))
                .then(|| entry.path())
            })
    }

    pub fn cancel(&self, transfer: &Transfer) -> Result<(), String> {
        if !valid_component(&transfer.id) {
            return Err("Invalid transfer ID".into());
        }
        self.request(
            "DELETE",
            &format!(
                "/api/v0/transfers/downloads/{}/{}",
                encode(&transfer.username),
                transfer.id
            ),
            None,
        )
        .map(|_| ())
    }

    pub fn stop(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn wait_for_stop(&self) {
        if let Ok(receiver) = self.output_closed.lock() {
            let _ = receiver.recv();
        }
    }
}

impl Drop for Slskd {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn free_port(address: &str) -> Result<u16, String> {
    TcpListener::bind(address)
        .and_then(|s| s.local_addr())
        .map(|a| a.port())
        .map_err(|e| e.to_string())
}

fn valid_component(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn parse_results(value: &Value) -> Vec<PeerFile> {
    let mut files: Vec<PeerFile> = value
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|response| {
            let username = response
                .get("username")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let free_slot = response
                .get("hasFreeUploadSlot")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let queue_length = response
                .get("queueLength")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let upload_speed = response
                .get("uploadSpeed")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            response
                .get("files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(move |file| {
                    let filename = file.get("filename")?.as_str()?.to_string();
                    let size = file.get("size")?.as_u64()?;
                    if username.is_empty() || filename.is_empty() || size == 0 {
                        return None;
                    }
                    Some(PeerFile {
                        username: username.clone(),
                        filename,
                        size,
                        length: file.get("length").and_then(Value::as_u64),
                        bitrate: file.get("bitRate").and_then(Value::as_u64),
                        free_slot,
                        queue_length,
                        upload_speed,
                    })
                })
        })
        .collect();
    files.sort_by(file_order);
    files
}

fn is_audio(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| {
        [
            "mp3", "flac", "m4a", "ogg", "opus", "wav", "aac", "wma", "aiff", "alac",
        ]
        .iter()
        .any(|allowed| ext.eq_ignore_ascii_case(allowed))
    })
}

pub fn album_candidates(files: Vec<PeerFile>) -> Vec<PeerFile> {
    let mut folders: Vec<PeerFile> = Vec::new();
    for file in files.into_iter().filter(|file| is_audio(&file.filename)) {
        let Some((directory, _)) = file.filename.rsplit_once(['\\', '/']) else {
            continue;
        };
        if directory.is_empty() {
            continue;
        }
        if let Some(folder) = folders
            .iter_mut()
            .find(|folder| folder.username == file.username && folder.filename == directory)
        {
            folder.size += 1;
        } else {
            folders.push(PeerFile {
                username: file.username,
                filename: directory.to_string(),
                size: 1,
                free_slot: file.free_slot,
                queue_length: file.queue_length,
                upload_speed: file.upload_speed,
                ..PeerFile::default()
            });
        }
    }
    folders.sort_by(album_order);
    folders
}

/// Folders named for the album with every song first, then by peer queue and speed.
pub fn rank_album_folders(folders: &mut [PeerFile], album: &crate::musicbrainz::Album) {
    let wanted = crate::musicbrainz::words(&album.title);
    let tier = |folder: &PeerFile| {
        let name = folder
            .filename
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or_default();
        let have: HashSet<String> = crate::musicbrainz::words(name).into_iter().collect();
        let named = wanted.iter().all(|word| have.contains(word));
        u8::from(!named) * 2 + u8::from(folder.size < u64::from(album.tracks))
    };
    folders.sort_by(|a, b| tier(a).cmp(&tier(b)).then(album_order(a, b)));
}

fn parse_directory(value: &Value, username: &str, directory: &str) -> Vec<PeerFile> {
    let separator = if directory.contains('/') { '/' } else { '\\' };
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("name").and_then(Value::as_str) == Some(directory))
        .flat_map(|entry| {
            entry
                .get("files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|file| {
            let name = file.get("filename")?.as_str()?;
            let size = file.get("size")?.as_u64()?;
            if name.is_empty() || name.contains(['\\', '/']) || size == 0 || !is_audio(name) {
                return None;
            }
            Some(PeerFile {
                username: username.to_string(),
                filename: format!("{directory}{separator}{name}"),
                size,
                length: file.get("length").and_then(Value::as_u64),
                bitrate: file.get("bitRate").and_then(Value::as_u64),
                ..PeerFile::default()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn album_folders_and_directory_contents() {
        let results = parse_results(&json!([
            {"username":"peer", "files":[
                {"filename":"Artist\\Album\\01.flac", "size":100},
                {"filename":"Artist\\Album\\02.mp3", "size":200},
                {"filename":"Artist\\Album\\cover.jpg", "size":300}
            ]},
            {"username":"other", "files":[{"filename":"Artist\\Album\\01.flac", "size":100}]}
        ]));
        let folders = album_candidates(results);
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].filename, "Artist\\Album");
        assert_eq!(folders[0].size, 2);
        let files = parse_directory(
            &json!([
                {"name":"Artist\\Album", "files":[
                    {"filename":"01.flac", "size":100},
                    {"filename":"02.MP3", "size":200},
                    {"filename":"cover.jpg", "size":300},
                    {"filename":"..\\evil.mp3", "size":100}
                ]},
                {"name":"Other", "files":[{"filename":"other.mp3", "size":100}]}
            ]),
            "peer",
            "Artist\\Album",
        );
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].filename, "Artist\\Album\\01.flac");
        assert_eq!(files[1].username, "peer");
        let root = parse_directory(
            &json!([{ "name": "Album", "files": [{ "filename": "01.flac", "size": 100 }] }]),
            "peer",
            "Album",
        );
        assert_eq!(root[0].filename, "Album\\01.flac");
    }

    #[test]
    fn parses_peer_files_and_encodes_names() {
        let value = json!([{"username":"user/one","files":[{"filename":"music\\song.flac","size":100,"length":180,"bitRate":900}, {"filename":"empty","size":0}]}]);
        let files = parse_results(&value);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].length, Some(180));
        assert_eq!(encode(&files[0].username), "user%2Fone");
        assert!(!valid_component("../x"));
    }

    fn peer_response(name: &str, free: bool, queue: u64, speed: u64, file: &str) -> Value {
        json!({"username":name, "hasFreeUploadSlot":free, "queueLength":queue, "uploadSpeed":speed,
            "files":[{"filename":format!("Artist\\Album\\{file}"), "size":100}]})
    }

    #[test]
    fn ranks_files_by_slot_queue_format_then_speed() {
        let files = parse_results(&json!([
            peer_response("long", false, 518, 9, "a.mp3"),
            peer_response("short", false, 2, 1, "b.flac"),
            peer_response("slow", true, 0, 1, "c.mp3"),
            peer_response("flac", true, 0, 9, "d.flac"),
            peer_response("fast", true, 0, 5, "e.MP3"),
        ]));
        let names: Vec<_> = files.iter().map(|f| f.username.as_str()).collect();
        assert_eq!(names, ["fast", "slow", "flac", "short", "long"]);
        assert!(files[0].free_slot && files[0].upload_speed == 5);
        assert_eq!((files[4].free_slot, files[4].queue_length), (false, 518));
    }

    #[test]
    fn album_folders_carry_and_rank_by_peer_queue() {
        let results = parse_results(&json!([
            peer_response("long", false, 518, 9, "01.mp3"),
            peer_response("short", false, 2, 1, "01.mp3"),
            peer_response("slow", true, 0, 1, "01.mp3"),
            peer_response("fast", true, 0, 5, "01.mp3"),
        ]));
        let folders = album_candidates(results.into_iter().rev().collect());
        let names: Vec<_> = folders.iter().map(|f| f.username.as_str()).collect();
        assert_eq!(names, ["fast", "slow", "short", "long"]);
        assert_eq!(folders[2].queue_length, 2);
        assert!(folders[0].free_slot);
    }

    // web/src/soulseek.test.js checks the same order.
    #[test]
    fn ranks_album_folders_named_for_the_album_with_every_song_first() {
        let response = |name: &str, free: bool, files: &[&str]| {
            let files: Vec<Value> = files
                .iter()
                .map(|file| json!({"filename": file, "size": 100}))
                .collect();
            json!({"username": name, "hasFreeUploadSlot": free, "queueLength": if free { 0 } else { 3 }, "files": files})
        };
        let deluxe = "Band\\2019 - Tidewater (Deluxe)\\";
        let results = parse_results(&json!([
            response("single", true, &["Band\\Tidewater\\01.flac"]),
            response(
                "other",
                true,
                &[
                    "Band\\Low Tide\\01.mp3",
                    "Band\\Low Tide\\02.mp3",
                    "Band\\Low Tide\\03.mp3"
                ]
            ),
            response(
                "full",
                false,
                &[
                    &format!("{deluxe}01.mp3"),
                    &format!("{deluxe}02.mp3"),
                    &format!("{deluxe}03.mp3")
                ]
            ),
        ]));
        let mut folders = album_candidates(results);
        let album = crate::musicbrainz::Album {
            id: "album".into(),
            title: "Tidewater".into(),
            artist: "Band".into(),
            year: None,
            tracks: 3,
        };
        rank_album_folders(&mut folders, &album);
        let names: Vec<_> = folders.iter().map(|f| f.username.as_str()).collect();
        assert_eq!(names, ["full", "single", "other"]);
    }

    #[test]
    fn saved_files_without_peer_fields_still_load() {
        let file: PeerFile = serde_json::from_value(
            json!({"username":"peer","filename":"a.mp3","size":5,"length":null,"bitrate":null}),
        )
        .unwrap();
        assert_eq!(
            (file.free_slot, file.queue_length, file.upload_speed),
            (false, 0, 0)
        );
    }

    #[test]
    fn account_is_made_once_and_kept_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/soulseek.json");
        let made = account(&path).unwrap();
        assert!(made.username.starts_with("clipplus") && made.username.len() == 18);
        assert_eq!(made.password.len(), 48);
        assert_eq!(account(&path).unwrap(), made);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::write(&path, b"not json").unwrap();
        assert!(account(&path).is_err());
    }

    #[test]
    fn finds_completed_download_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let destination = "a".repeat(48);
        let folder = dir.path().join(&destination);
        fs::create_dir(&folder).unwrap();
        let path = folder.join("track.mp3");
        fs::write(&path, b"audio").unwrap();
        let client = Slskd {
            child: Mutex::new(Command::new("sleep").arg("60").spawn().unwrap()),
            enqueue_lock: Mutex::new(()),
            output_closed: Mutex::new(mpsc::channel().1),
            agent: crate::net::agent(),
            base: String::new(),
            key: String::new(),
            downloads: dir.path().into(),
        };
        let transfer = Transfer {
            id: "a".into(),
            username: "peer".into(),
            destination,
            size: 5,
        };
        assert_eq!(client.completed_file(&transfer), Some(path));
    }

    #[test]
    fn search_waits_for_final_event_and_fetches_results() {
        use std::io::{Read, Write};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            assert!(ws.read().unwrap().to_text().unwrap().contains("protocol"));
            ws.send(Message::Text("{}\u{1e}".into())).unwrap();
            let reply = |mut stream: TcpStream, body: &str| {
                let mut request = Vec::new();
                let mut buffer = [0u8; 8192];
                loop {
                    let size = stream.read(&mut buffer).unwrap();
                    assert!(size > 0);
                    request.extend_from_slice(&buffer[..size]);
                    if let Some(end) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8_lossy(&request);
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("x-api-key: 0123456789abcdef")
                );
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            };
            let (stream, _) = listener.accept().unwrap();
            reply(stream, r#"{"id":"abc"}"#);
            ws.send(Message::Text(
                "{\"target\":\"UPDATE\",\"arguments\":[{\"id\":\"abc\",\"endedAt\":null}]}\u{1e}"
                    .into(),
            ))
            .unwrap();
            ws.send(Message::Text("{\"target\":\"UPDATE\",\"arguments\":[{\"id\":\"abc\",\"endedAt\":\"now\",\"state\":\"Completed\"}]}\u{1e}".into())).unwrap();
            let (stream, _) = listener.accept().unwrap();
            reply(
                stream,
                r#"[{"username":"peer","files":[{"filename":"song.mp3","size":42}]}]"#,
            );
        });
        let child = Command::new("sleep").arg("60").spawn().unwrap();
        let client = Slskd {
            child: Mutex::new(child),
            enqueue_lock: Mutex::new(()),
            output_closed: Mutex::new(mpsc::channel().1),
            agent: crate::net::agent(),
            base: format!("http://{address}"),
            key: "0123456789abcdef".into(),
            downloads: PathBuf::new(),
        };
        let result = client.search("example").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].filename, "song.mp3");
        server.join().unwrap();
    }
}
