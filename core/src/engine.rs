//! The coordinator. One thread owns all job state and reacts to messages from
//! the UI and from worker threads. Nothing polls.

use crate::bootloader;
use crate::card::{CardInfo, Udisks, Volumes};
use crate::drive::Drive;
use crate::musicbrainz::Recording;
use crate::paths::{Config, Dirs, State, now_secs, read_json, read_json_checked, write_json};
use crate::player::{self, Firmware, PlayerState};
use crate::proc::{self, Cancel};
use crate::queue::{self, Job, Stage};
use crate::rockbox::{self, Build, Channel, Theme};
use crate::slskd::{PeerFile, Slskd, Transfer, TransferStatus};
use crate::tools::{self, Resolved, Staged, Tool};
use crate::ytdlp::{self, Found, Line};
use crate::{fixer, net, notify, selfupdate, transfer};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const MAX_FINDING: usize = 2;
const MAX_DOWNLOADS: usize = 3;
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq)]
pub struct JobView {
    pub id: u64,
    pub title: String,
    pub status: String,
    pub stage: &'static str,
    pub progress: f64,
    pub can_cancel: bool,
    pub can_retry: bool,
}

impl From<&Job> for JobView {
    fn from(job: &Job) -> Self {
        Self {
            id: job.id,
            title: job.title.clone(),
            status: job.status(),
            stage: job.stage_name(),
            progress: job.progress(),
            can_cancel: job.can_cancel(),
            can_retry: job.can_retry(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardView {
    pub kind: &'static str,
    pub text: String,
    pub can_eject: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    Jobs(Vec<JobView>),
    JobUpdate(JobView),
    Waiting(usize),
    Card(CardView),
    SponsorBlock(bool),
    Tools(String),
    Updated(String),
    /// Update check status. `checking` is true while one runs.
    UpdateCheck {
        checking: bool,
        text: String,
    },
    SoulseekStatus {
        ready: bool,
        text: String,
    },
    PeerResults(Vec<PeerFile>),
    PeerError(String),
    PeerSearching(bool),
    /// The player, its drives and the firmware on it.
    Player(PlayerState),
    /// Progress or result text of a Rockbox action. `busy` is true while one runs.
    RockboxStatus {
        busy: bool,
        text: String,
    },
    RockboxChannel(Channel),
    /// The newest build of the chosen channel and whether the player lacks it.
    RockboxLatest {
        channel: Channel,
        version: String,
        update_available: bool,
        can_roll_back: bool,
    },
    Themes(Vec<Theme>),
    /// The player's database auto update setting. None when it cannot be read.
    DatabaseAutoUpdate(Option<bool>),
    /// The firmware file is on the player and the drive is ejected. True until
    /// the player is unplugged and comes back.
    FlashPending(bool),
    /// The last Rockbox action failed.
    RockboxFailed(bool),
    /// Songs were copied to the card.
    SongsCopied,
}

pub const UNPLUG_TEXT: &str = "Unplug the player now. It flashes the bootloader and powers off by itself in about a minute. Do not touch it.";
pub const RESTORE_TEXT: &str = "Unplug the player. Then turn it on while holding Left (|<<), so the stock firmware starts and flashes the file. After that, use Uninstall to remove the .rockbox folder.";

pub type Emit = Arc<dyn Fn(UiEvent) + Send + Sync>;

enum Msg {
    AddLinks(Vec<String>),
    Cancel(u64),
    Retry(u64),
    ClearFinished,
    Eject,
    SetSponsorBlock(bool),
    SoulseekConnect,
    SoulseekStarted(Result<Arc<Slskd>, String>),
    SoulseekExited(Arc<Slskd>),
    SearchPeers(Recording),
    PeersFound(u64, Result<Vec<PeerFile>, String>),
    DownloadPeer(usize),
    SoulseekQueued {
        id: u64,
        generation: u64,
        transfer: Transfer,
    },
    Discovered {
        id: u64,
        generation: u64,
        result: Result<Found, String>,
    },
    Progress {
        id: u64,
        generation: u64,
        fraction: Option<f64>,
        speed: Option<f64>,
        processing: bool,
    },
    Downloaded {
        id: u64,
        generation: u64,
        result: Result<PathBuf, String>,
    },
    Fixed {
        id: u64,
        generation: u64,
        result: Result<PathBuf, String>,
    },
    Udisks(Udisks),
    Card(Result<Volumes, String>),
    Mounted(Result<PathBuf, String>),
    InternalMounted(Result<PathBuf, String>),
    InternalEjected {
        result: Result<(), String>,
        text: String,
        flash: bool,
    },
    Mtp(bool),
    Player(u64, PlayerState),
    Autoupdate(u64, Option<bool>),
    RockboxCheck,
    RockboxChecked {
        channel: Channel,
        result: Result<Build, String>,
    },
    RockboxQueue(RockboxJob),
    RockboxNote(String),
    RockboxDone {
        job: RockboxJob,
        result: Result<String, String>,
    },
    RockboxThemes,
    ThemesListed(Result<Vec<Theme>, String>),
    SetRockboxChannel(Channel),
    Free(Option<u64>),
    CopyStarted(u64),
    Copied(Option<u64>),
    CopyFailed {
        id: Option<u64>,
        reason: String,
    },
    TransferDone,
    Ejected(Result<(), String>),
    ToolStaged(Staged),
    AppUpdated(String),
    CheckUpdates,
    UpdateNote(String),
    UpdatesDone(UpdateReport),
    Shutdown(Sender<()>),
}

#[derive(Default)]
struct GateState {
    // A count, because the next job can take its guard before the last one drops.
    running: usize,
    stopping: bool,
}

/// Shared by the coordinator, the Rockbox worker and `Engine::shutdown`, so
/// shutdown can wait for a running install without the coordinator's help.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<GateState>, Condvar)>);

/// Held by the Rockbox worker. Dropping it, even after a panic or a failed
/// thread spawn, releases its count.
struct GateGuard(Gate);

impl Gate {
    fn state(&self) -> MutexGuard<'_, GateState> {
        (self.0).0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// None once shutdown began, so no job starts after shutdown looked.
    fn acquire(&self) -> Option<GateGuard> {
        let mut state = self.state();
        if state.stopping {
            return None;
        }
        state.running += 1;
        Some(GateGuard(self.clone()))
    }

    fn stop(&self) {
        self.state().stopping = true;
    }

    fn wait_idle(&self) {
        let mut state = self.state();
        while state.running > 0 {
            state = (self.0).1.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.0.state().running -= 1;
        (self.0.0).1.notify_all();
    }
}

type Spawner = fn(&str, Box<dyn FnOnce() + Send>) -> std::io::Result<()>;

fn spawn_thread(name: &str, work: Box<dyn FnOnce() + Send>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(work)
        .map(|_| ())
}

type Ejector = Arc<dyn Fn(&CardInfo) -> Result<(), String> + Send + Sync>;

#[derive(Clone)]
pub struct Engine {
    tx: Sender<Msg>,
    gate: Gate,
}

impl Engine {
    pub fn add_links(&self, links: Vec<String>) {
        let _ = self.tx.send(Msg::AddLinks(links));
    }
    pub fn cancel(&self, id: u64) {
        let _ = self.tx.send(Msg::Cancel(id));
    }
    pub fn retry(&self, id: u64) {
        let _ = self.tx.send(Msg::Retry(id));
    }
    pub fn clear_finished(&self) {
        let _ = self.tx.send(Msg::ClearFinished);
    }
    pub fn eject(&self) {
        let _ = self.tx.send(Msg::Eject);
    }
    pub fn set_sponsorblock(&self, on: bool) {
        let _ = self.tx.send(Msg::SetSponsorBlock(on));
    }
    pub fn soulseek_connect(&self) {
        let _ = self.tx.send(Msg::SoulseekConnect);
    }
    /// Checks the app and every managed tool now, ignoring the daily limit.
    pub fn check_updates(&self) {
        let _ = self.tx.send(Msg::CheckUpdates);
    }
    pub fn search_peers(&self, recording: Recording) {
        let _ = self.tx.send(Msg::SearchPeers(recording));
    }
    pub fn download_peer(&self, index: usize) {
        let _ = self.tx.send(Msg::DownloadPeer(index));
    }
    /// Looks up the newest build of the chosen channel.
    pub fn rockbox_check(&self) {
        let _ = self.tx.send(Msg::RockboxCheck);
    }
    /// Installs the newest build of the chosen channel on the player.
    pub fn rockbox_update(&self) {
        self.queue_rockbox(RockboxJob::Update);
    }
    /// Installs the build that was on the host before the last update.
    pub fn rockbox_rollback(&self) {
        self.queue_rockbox(RockboxJob::Rollback);
    }
    pub fn rockbox_install_fonts(&self) {
        self.queue_rockbox(RockboxJob::Fonts);
    }
    pub fn rockbox_install_voice(&self) {
        self.queue_rockbox(RockboxJob::Voice);
    }
    /// Installs a theme by the id in a `Themes` event.
    pub fn rockbox_install_theme(&self, id: &str) {
        self.queue_rockbox(RockboxJob::Theme(id.into()));
    }
    /// Removes what the install log lists. `full` also removes settings and the database.
    pub fn rockbox_uninstall(&self, full: bool) {
        self.queue_rockbox(RockboxJob::Uninstall(full));
    }
    pub fn set_database_autoupdate(&self, on: bool) {
        self.queue_rockbox(RockboxJob::Autoupdate(on));
    }
    /// Loads the theme list for the installed (or newest) build.
    pub fn rockbox_install_bootloader(&self) {
        self.queue_rockbox(RockboxJob::Bootloader);
    }
    pub fn rockbox_restore_stock(&self) {
        self.queue_rockbox(RockboxJob::RestoreStock);
    }
    /// Rockbox build, then the bootloader, for a player on the stock firmware.
    pub fn rockbox_install_first(&self) {
        self.queue_rockbox(RockboxJob::FirstInstall);
    }
    pub fn rockbox_themes(&self) {
        let _ = self.tx.send(Msg::RockboxThemes);
    }
    pub fn set_rockbox_channel(&self, channel: Channel) {
        let _ = self.tx.send(Msg::SetRockboxChannel(channel));
    }
    fn queue_rockbox(&self, job: RockboxJob) {
        let _ = self.tx.send(Msg::RockboxQueue(job));
    }
    /// Stops running downloads and saves the queue. Waits briefly for that,
    /// and as long as it takes for a running Rockbox install to finish.
    pub fn shutdown(&self) {
        self.shutdown_within(Duration::from_secs(2));
    }

    fn shutdown_within(&self, ack_timeout: Duration) {
        // No Rockbox job may start from here on, whatever the coordinator is doing.
        self.gate.stop();
        let (ack, done) = channel();
        if self.tx.send(Msg::Shutdown(ack)).is_ok() {
            let _ = done.recv_timeout(ack_timeout);
        }
        self.gate.wait_idle();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RockboxJob {
    Update,
    Rollback,
    Fonts,
    Voice,
    Theme(String),
    Uninstall(bool),
    Autoupdate(bool),
    Bootloader,
    RestoreStock,
    FirstInstall,
}

impl RockboxJob {
    /// The player flashes a firmware file after these, so the job ends with an eject.
    fn flashes(&self) -> bool {
        matches!(
            self,
            Self::Bootloader | Self::RestoreStock | Self::FirstInstall
        )
    }

    fn ejects(&self) -> bool {
        self.flashes() || matches!(self, Self::Update | Self::Rollback)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CardNote {
    Safe,
    EjectError(String),
    MountError(String),
}

struct Core {
    dirs: Dirs,
    cfg: Config,
    emit: Emit,
    tx: Sender<Msg>,
    jobs: Vec<Job>,
    next_id: u64,
    dirty: bool,
    jobs_writable: bool,
    config_writable: bool,
    bundled: Option<PathBuf>,
    tools: Resolved,
    staged: Vec<Staged>,
    appimage: Option<PathBuf>,
    /// The version on disk, newer than the running one after an update.
    app_version: String,
    updating: bool,
    manual_update: bool,
    check_again: bool,
    cancels: HashMap<u64, Arc<Cancel>>,
    udisks: Option<Udisks>,
    ejector: Option<Ejector>,
    eject_deferred: bool,
    gate: Gate,
    spawner: Spawner,
    card: Option<CardInfo>,
    free: Option<u64>,
    note: Option<CardNote>,
    mounting: bool,
    ejecting: bool,
    ejected_block: Option<String>,
    eject_after_copy: bool,
    transferring: bool,
    transfer_failed: bool,
    stopping: bool,
    card_cleanup_pending: bool,
    stop_transfer: Arc<AtomicBool>,
    batch_copied: usize,
    last_waiting: Option<usize>,
    last_card: Option<CardView>,
    last_tools: String,
    soulseek: Option<Arc<Slskd>>,
    soulseek_starting: bool,
    selected_recording: Option<Recording>,
    peer_results: Vec<PeerFile>,
    peer_generation: u64,
    internal: Option<CardInfo>,
    internal_mounting: bool,
    internal_mount_failed: bool,
    internal_ejected: Option<String>,
    flash_pending: bool,
    flash_gone: bool,
    mtp: bool,
    player_generation: u64,
    player: Option<PlayerState>,
    rockbox_checks: bool,
    rockbox_checking: bool,
    rockbox_busy: bool,
    rockbox_queue: VecDeque<RockboxJob>,
    latest: Option<Build>,
    last_latest: Option<(Channel, String, bool, bool)>,
    themes: Vec<Theme>,
}

/// What the coordinator listens to besides the UI.
#[derive(Debug, Clone)]
pub struct Options {
    pub card: bool,
    pub updates: bool,
    pub bundled_tools: Option<PathBuf>,
}

/// Starts the coordinator and everything it listens to.
pub fn start(dirs: Dirs, appimage: Option<PathBuf>, emit: Emit) -> Engine {
    let options = Options {
        card: true,
        updates: true,
        bundled_tools: tools::bundled_dir(),
    };
    start_with(dirs, appimage, emit, options)
}

pub fn start_with(dirs: Dirs, appimage: Option<PathBuf>, emit: Emit, options: Options) -> Engine {
    let (tx, rx) = channel();
    let gate = Gate::default();
    let engine = Engine {
        tx: tx.clone(),
        gate: gate.clone(),
    };
    let _ = dirs.create();
    tools::clean_stages(&dirs.tools);

    let (core, resolved) = build_core(&dirs, &appimage, emit, &options, &tx, gate);

    // Queued Soulseek downloads resume once slskd is connected.
    if core
        .jobs
        .iter()
        .any(|job| job.soulseek.is_some() && job.stage == Stage::Waiting)
    {
        let _ = tx.send(Msg::SoulseekConnect);
    }

    if options.card {
        let mtp = tx.clone();
        player::watch_mtp(move |on| {
            let _ = mtp.send(Msg::Mtp(on));
        });
        let (connected, changed) = (tx.clone(), tx.clone());
        Udisks::watch(
            move |udisks| {
                let _ = connected.send(Msg::Udisks(udisks));
            },
            move |card| {
                let _ = changed.send(Msg::Card(card));
            },
        );
    }
    if options.updates {
        spawn_updater(
            tx,
            dirs,
            resolved,
            appimage,
            selfupdate::CURRENT.into(),
            Vec::new(),
            false,
        );
    }

    std::thread::Builder::new()
        .name("coordinator".into())
        .spawn(move || core.run(rx))
        .expect("spawn coordinator");
    engine
}

fn build_core(
    dirs: &Dirs,
    appimage: &Option<PathBuf>,
    emit: Emit,
    options: &Options,
    tx: &Sender<Msg>,
    gate: Gate,
) -> (Core, Resolved) {
    let bundled = options.bundled_tools.clone();
    let (saved_jobs, jobs_ok) = read_json_checked::<Vec<Job>>(&dirs.jobs_file);
    let mut jobs = queue::restore(saved_jobs);
    // Download folders can outlive a lost jobs file; never reuse their ids.
    let next_id = jobs
        .iter()
        .map(|j| j.id)
        .chain(download_folder_ids(&dirs.downloads))
        .max()
        .unwrap_or(0)
        + 1;
    for job in &mut jobs {
        job.running = false;
    }
    if jobs_ok && !dirs.jobs_file.with_extension("json.bad").exists() {
        remove_stale_downloads(&dirs.downloads, &jobs);
    }

    let (cfg, config_ok) = read_json_checked(&dirs.config_file);
    let jobs_writable = jobs_ok || !dirs.jobs_file.exists();
    let config_writable = config_ok || !dirs.config_file.exists();
    let resolved = Resolved::load(&dirs.tools, bundled.as_deref());
    let core = Core {
        cfg,
        jobs_writable,
        config_writable,
        emit,
        tx: tx.clone(),
        jobs,
        next_id,
        dirty: true,
        bundled,
        tools: resolved.clone(),
        staged: Vec::new(),
        appimage: appimage.clone(),
        app_version: selfupdate::CURRENT.into(),
        updating: options.updates,
        manual_update: false,
        check_again: false,
        cancels: HashMap::new(),
        udisks: None,
        ejector: None,
        eject_deferred: false,
        gate,
        spawner: spawn_thread,
        card: None,
        free: None,
        note: None,
        mounting: false,
        ejecting: false,
        ejected_block: None,
        eject_after_copy: false,
        transferring: false,
        transfer_failed: false,
        stopping: false,
        card_cleanup_pending: false,
        stop_transfer: Arc::new(AtomicBool::new(false)),
        batch_copied: 0,
        last_waiting: None,
        last_card: None,
        last_tools: String::new(),
        soulseek: None,
        soulseek_starting: false,
        selected_recording: None,
        peer_results: Vec::new(),
        peer_generation: 0,
        internal: None,
        internal_mounting: false,
        internal_mount_failed: false,
        internal_ejected: None,
        flash_pending: false,
        flash_gone: false,
        mtp: false,
        player_generation: 0,
        player: None,
        rockbox_checks: options.updates,
        rockbox_checking: false,
        rockbox_busy: false,
        rockbox_queue: VecDeque::new(),
        latest: None,
        last_latest: None,
        themes: Vec::new(),
        dirs: dirs.clone(),
    };
    (core, resolved)
}

/// Job ids of `<id>-<generation>` folders in the downloads dir.
fn download_folder_ids(downloads: &Path) -> Vec<u64> {
    fs::read_dir(downloads)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name();
                    name.to_str()?.split('-').next()?.parse().ok()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn remove_stale_downloads(downloads: &Path, jobs: &[Job]) {
    let Ok(entries) = fs::read_dir(downloads) else {
        return;
    };
    for entry in entries.flatten() {
        let keep = jobs.iter().any(|j| {
            j.downloaded.as_ref().and_then(|p| p.parent()) == Some(entry.path().as_path())
        });
        if !keep {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn current_attempt(job: &Job, id: u64, generation: u64, stage: Stage) -> bool {
    job.id == id && job.generation == generation && job.stage == stage
}

fn clean_card_temps(tracks: &Path) {
    if let Ok(entries) = fs::read_dir(tracks) {
        for entry in entries.flatten() {
            if fixer::is_temp(&entry.path()) && entry.file_type().is_ok_and(|t| t.is_file()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn spawn<F: FnOnce() + Send + 'static>(name: &str, work: F) {
    let _ = std::thread::Builder::new().name(name.into()).spawn(work);
}

pub fn format_free(bytes: u64) -> String {
    let (value, unit) = if bytes >= 1_000_000_000_000 {
        (bytes as f64 / 1e12, "TB")
    } else if bytes >= 1_000_000_000 {
        (bytes as f64 / 1e9, "GB")
    } else {
        (bytes as f64 / 1e6, "MB")
    };
    if value >= 100.0 || unit == "MB" {
        format!("{value:.0} {unit}")
    } else {
        format!("{value:.1} {unit}")
    }
}

fn outbox_files(outbox: &Path) -> Vec<PathBuf> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(outbox)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && !fixer::is_temp(&e.path()))
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (modified, e.path())
        })
        .collect();
    files.sort();
    files.into_iter().map(|(_, p)| p).collect()
}

impl Core {
    fn run(mut self, rx: Receiver<Msg>) {
        self.refresh_all();
        if self.rockbox_checks {
            self.rockbox_check();
        }
        self.schedule();
        for msg in rx {
            self.handle(msg);
            self.schedule();
            self.flush();
        }
    }

    fn job_mut(&mut self, id: u64) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    fn show(&self, id: u64) {
        if let Some(job) = self.jobs.iter().find(|j| j.id == id) {
            (self.emit)(UiEvent::JobUpdate(job.into()));
        }
    }

    fn show_all(&mut self) {
        (self.emit)(UiEvent::Jobs(self.jobs.iter().map(JobView::from).collect()));
        self.dirty = true;
    }

    fn refresh_all(&mut self) {
        (self.emit)(UiEvent::SponsorBlock(self.cfg.sponsorblock));
        (self.emit)(UiEvent::RockboxChannel(self.cfg.rockbox_channel));
        self.show_all();
        self.refresh_waiting();
        self.refresh_card();
        self.refresh_tools();
    }

    fn flush(&mut self) {
        if self.dirty && self.jobs_writable {
            self.dirty = false;
            let _ = write_json(&self.dirs.jobs_file, &self.jobs);
        }
    }

    fn refresh_waiting(&mut self) {
        let count = outbox_files(&self.dirs.outbox).len();
        if self.last_waiting != Some(count) {
            self.last_waiting = Some(count);
            (self.emit)(UiEvent::Waiting(count));
        }
        self.refresh_card();
    }

    fn refresh_tools(&mut self) {
        let missing = self.tools.missing();
        let text = if missing.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> = missing.iter().map(|t| t.key()).collect();
            format!("Getting {}", names.join(", "))
        };
        if text != self.last_tools {
            self.last_tools = text.clone();
            (self.emit)(UiEvent::Tools(text));
        }
    }

    fn card_view(&self) -> CardView {
        let view = |kind, text: String, can_eject| CardView {
            kind,
            text,
            can_eject,
        };
        if self.ejecting {
            return view("busy", "Ejecting the Sansa".into(), false);
        }
        if let Some(note) = &self.note {
            match note {
                CardNote::Safe => return view("safe", "Safe to unplug the Sansa".into(), false),
                CardNote::EjectError(e) => {
                    return view(
                        "error",
                        format!("Could not eject: {e}"),
                        self.card.is_some(),
                    );
                }
                CardNote::MountError(e) => {
                    return view("error", format!("Could not mount the Sansa: {e}"), false);
                }
            }
        }
        let waiting = self.last_waiting.unwrap_or(0);
        match &self.card {
            Some(card) if self.ejected_block.as_ref() == Some(&card.block) => {
                view("safe", "Safe to unplug the Sansa".into(), false)
            }
            Some(_) if self.transferring => {
                view("copying", "Copying songs to the Sansa".into(), true)
            }
            Some(_) => {
                let text = match self.free {
                    Some(free) => format!("Sansa connected, {} free", format_free(free)),
                    None => "Sansa connected".into(),
                };
                view("connected", text, true)
            }
            None if waiting > 0 => view("plug", "Plug in your Sansa".into(), false),
            None => view("none", String::new(), false),
        }
    }

    fn refresh_card(&mut self) {
        let view = self.card_view();
        if self.last_card.as_ref() != Some(&view) {
            self.last_card = Some(view.clone());
            (self.emit)(UiEvent::Card(view));
        }
    }

    // Messages -----------------------------------------------------------

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::SoulseekConnect => {
                if self.soulseek_starting || self.soulseek.is_some() {
                    return;
                }
                self.soulseek_starting = true;
                (self.emit)(UiEvent::SoulseekStatus {
                    ready: false,
                    text: "Connecting to Soulseek".into(),
                });
                let tx = self.tx.clone();
                let dirs = self.dirs.clone();
                let keep: Vec<String> = self
                    .jobs
                    .iter()
                    .filter_map(|job| job.soulseek.as_ref()?.transfer.as_ref())
                    .map(|transfer| transfer.destination.clone())
                    .collect();
                spawn("soulseek-start", move || {
                    let root = dirs.tools.join("slskd");
                    clean_soulseek_files(&dirs.data.join("slskd"), &keep);
                    let result = (|| {
                        let account = crate::slskd::account(&dirs.soulseek_file)?;
                        let installed = match crate::slskd_tools::find(&root) {
                            Some(found) => found,
                            None => {
                                let agent = net::agent();
                                let release = crate::slskd_tools::fetch_latest(&agent)?;
                                crate::slskd_tools::install(&agent, &release, &root)?
                            }
                        };
                        crate::slskd_tools::prune(&root, &installed.version);
                        Slskd::start(
                            &installed.executable,
                            &dirs.data.join("slskd"),
                            &account.username,
                            &account.password,
                        )
                        .map(Arc::new)
                    })();
                    match result {
                        Ok(client) => {
                            if tx.send(Msg::SoulseekStarted(Ok(client.clone()))).is_err() {
                                client.stop();
                            }
                            client.wait_for_stop();
                            client.stop();
                            let _ = tx.send(Msg::SoulseekExited(client));
                        }
                        Err(error) => {
                            let _ = tx.send(Msg::SoulseekStarted(Err(error)));
                        }
                    }
                });
            }
            Msg::SoulseekStarted(result) => {
                self.soulseek_starting = false;
                if self.stopping {
                    if let Ok(client) = result {
                        client.stop();
                    }
                    return;
                }
                match result {
                    Ok(client) => {
                        self.soulseek = Some(client);
                        (self.emit)(UiEvent::SoulseekStatus {
                            ready: true,
                            text: "Connected to Soulseek".into(),
                        });
                    }
                    Err(text) => (self.emit)(UiEvent::SoulseekStatus { ready: false, text }),
                }
            }
            Msg::SoulseekExited(client) => {
                if self
                    .soulseek
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &client))
                {
                    self.soulseek = None;
                    self.peer_generation += 1;
                    (self.emit)(UiEvent::PeerSearching(false));
                    (self.emit)(UiEvent::PeerError(
                        "Soulseek disconnected. Press Connect to try again.".into(),
                    ));
                    (self.emit)(UiEvent::SoulseekStatus {
                        ready: false,
                        text: "Soulseek disconnected. Press Connect to try again.".into(),
                    });
                }
            }
            Msg::SearchPeers(recording) => {
                self.peer_generation += 1;
                let generation = self.peer_generation;
                self.peer_results.clear();
                (self.emit)(UiEvent::PeerResults(Vec::new()));
                (self.emit)(UiEvent::PeerError(String::new()));
                let Some(client) = self.soulseek.clone() else {
                    (self.emit)(UiEvent::PeerSearching(false));
                    (self.emit)(UiEvent::PeerError("Connect to Soulseek first".into()));
                    return;
                };
                let query = format!("{} {}", recording.artist, recording.title);
                self.selected_recording = Some(recording);
                (self.emit)(UiEvent::PeerSearching(true));
                let tx = self.tx.clone();
                spawn("soulseek-search", move || {
                    let _ = tx.send(Msg::PeersFound(generation, client.search(&query)));
                });
            }
            Msg::PeersFound(generation, result) if generation == self.peer_generation => {
                (self.emit)(UiEvent::PeerSearching(false));
                match result {
                    Ok(files) => {
                        self.peer_results = files;
                        (self.emit)(UiEvent::PeerResults(self.peer_results.clone()));
                    }
                    Err(error) => (self.emit)(UiEvent::PeerError(error)),
                }
            }
            Msg::PeersFound(..) => {}
            Msg::DownloadPeer(index) => {
                if let (Some(peer), Some(recording)) = (
                    self.peer_results.get(index).cloned(),
                    self.selected_recording.clone(),
                ) {
                    let mut job = Job::new(self.next_id, String::new());
                    self.next_id += 1;
                    job.title = format!("{} - {}", recording.artist, recording.title);
                    job.found = true;
                    job.stage = Stage::Waiting;
                    job.metadata = Some(fixer::Metadata {
                        title: recording.title,
                        artist: recording.artist,
                        album: recording.release,
                    });
                    job.soulseek = Some(queue::SoulseekJob {
                        peer,
                        transfer: None,
                    });
                    self.jobs.push(job);
                    self.show_all();
                }
            }
            Msg::SoulseekQueued {
                id,
                generation,
                transfer,
            } => {
                if let Some(job) = self
                    .jobs
                    .iter_mut()
                    .find(|job| current_attempt(job, id, generation, Stage::Downloading))
                    && let Some(source) = job.soulseek.as_mut()
                {
                    source.transfer = Some(transfer);
                    self.dirty = true;
                }
            }
            Msg::AddLinks(links) => {
                for url in links {
                    let job = Job::new(self.next_id, url);
                    self.next_id += 1;
                    self.jobs.push(job);
                }
                self.show_all();
            }
            Msg::Cancel(id) => self.cancel(id),
            Msg::Retry(id) => {
                if let Some(job) = self.job_mut(id).filter(|j| j.can_retry()) {
                    if let Some(source) = job.soulseek.as_mut() {
                        source.transfer = None;
                    }
                    job.generation += 1;
                    job.stage = job.retry_stage();
                    job.reason.clear();
                    job.fraction = None;
                    job.speed = None;
                    job.processing = false;
                    self.transfer_failed = false;
                    self.show_all();
                    self.refresh_waiting();
                }
            }
            Msg::ClearFinished => {
                self.jobs.retain(|j| !j.is_finished() || j.running);
                self.show_all();
            }
            Msg::SetSponsorBlock(on) => {
                self.cfg.sponsorblock = on;
                if self.config_writable {
                    let _ = write_json(&self.dirs.config_file, &self.cfg);
                }
                (self.emit)(UiEvent::SponsorBlock(on));
            }
            Msg::Discovered {
                id,
                generation,
                result,
            } => {
                if self
                    .jobs
                    .iter()
                    .any(|j| j.id == id && j.generation == generation)
                {
                    self.discovered(id, result);
                }
            }
            Msg::Progress {
                id,
                generation,
                fraction,
                speed,
                processing,
            } => {
                if let Some(job) = self
                    .job_mut(id)
                    .filter(|j| j.stage == Stage::Downloading && j.generation == generation)
                {
                    job.fraction = fraction.or(job.fraction);
                    job.speed = speed;
                    job.processing = processing;
                    self.show(id);
                }
            }
            Msg::Downloaded {
                id,
                generation,
                result,
            } => {
                if self
                    .jobs
                    .iter()
                    .any(|j| current_attempt(j, id, generation, Stage::Downloading))
                {
                    self.downloaded(id, result);
                } else if let Ok(file) = result
                    && let Some(dir) = file.parent()
                {
                    let _ = fs::remove_dir_all(dir);
                }
            }
            Msg::Fixed {
                id,
                generation,
                result,
            } => {
                if self
                    .jobs
                    .iter()
                    .any(|j| current_attempt(j, id, generation, Stage::Fixing))
                {
                    self.fixed(id, result);
                } else {
                    if let Ok(file) = result {
                        let _ = fs::remove_file(file);
                    }
                    if let Some(job) = self.jobs.iter_mut().find(|j| {
                        j.id == id && j.generation == generation && j.stage == Stage::Canceled
                    }) {
                        job.running = false;
                        if let Some(file) = job.downloaded.take()
                            && let Some(dir) = file.parent()
                        {
                            let _ = fs::remove_dir_all(dir);
                        }
                    }
                    self.show_all();
                }
            }
            Msg::Udisks(udisks) => {
                let eject = udisks.clone();
                self.ejector = Some(Arc::new(move |card| eject.eject(card)));
                self.udisks = Some(udisks);
            }
            Msg::Card(result) => self.on_volumes(result),
            Msg::InternalMounted(result) => {
                self.internal_mounting = false;
                if let Err(e) = result {
                    self.internal_mount_failed = true;
                    self.rockbox_status(false, format!("Could not mount the player: {e}"));
                }
            }
            Msg::InternalEjected {
                result,
                text,
                flash,
            } => {
                let text = match result {
                    Ok(()) => {
                        self.internal_ejected = self.internal.as_ref().map(|c| c.block.clone());
                        if flash {
                            self.flash_pending = true;
                            self.flash_gone = false;
                            (self.emit)(UiEvent::FlashPending(true));
                        }
                        text
                    }
                    Err(e) => {
                        (self.emit)(UiEvent::RockboxFailed(true));
                        format!("{text} Could not eject the player: {e}")
                    }
                };
                self.rockbox_status(false, text);
                self.rockbox_idle();
                self.refresh_player();
            }
            Msg::Mtp(on) => {
                self.mtp = on;
                self.refresh_player();
            }
            Msg::Player(generation, state) => {
                if generation == self.player_generation && self.player.as_ref() != Some(&state) {
                    self.player = Some(state.clone());
                    (self.emit)(UiEvent::Player(state));
                    self.emit_latest();
                }
            }
            Msg::Autoupdate(generation, value) => {
                if generation == self.player_generation {
                    (self.emit)(UiEvent::DatabaseAutoUpdate(value));
                }
            }
            Msg::RockboxCheck => self.rockbox_check(),
            Msg::RockboxChecked { channel, result } => {
                self.rockbox_checking = false;
                if channel != self.cfg.rockbox_channel {
                    self.rockbox_check();
                } else {
                    match result {
                        Ok(build) => {
                            self.latest = Some(build);
                            self.emit_latest();
                        }
                        Err(e) => self.rockbox_status(
                            self.rockbox_busy,
                            format!("Could not check for Rockbox updates: {e}"),
                        ),
                    }
                }
            }
            Msg::RockboxQueue(job) => {
                if self.transferring {
                    self.rockbox_status(true, "Waiting for the song copy to finish".into());
                }
                self.rockbox_queue.push_back(job);
            }
            Msg::RockboxNote(text) => self.rockbox_status(true, text),
            Msg::RockboxDone { job, result } => {
                match result {
                    // The player stays busy until the eject is over.
                    Ok(text) if !self.stopping && job.ejects() => {
                        (self.emit)(UiEvent::RockboxFailed(false));
                        self.eject_internal(text, job.flashes());
                    }
                    Ok(text) => {
                        (self.emit)(UiEvent::RockboxFailed(false));
                        self.rockbox_status(false, text);
                        self.rockbox_idle();
                    }
                    Err(e) => {
                        (self.emit)(UiEvent::RockboxFailed(true));
                        self.rockbox_status(false, e);
                        self.rockbox_idle();
                    }
                }
                self.refresh_player();
            }
            Msg::RockboxThemes => self.list_themes(),
            Msg::ThemesListed(result) => match result {
                Ok(themes) => {
                    self.themes.clone_from(&themes);
                    (self.emit)(UiEvent::Themes(themes));
                }
                Err(e) => self.rockbox_status(
                    self.rockbox_busy,
                    format!("Could not load the theme list: {e}"),
                ),
            },
            Msg::SetRockboxChannel(channel) => {
                if channel != self.cfg.rockbox_channel {
                    self.cfg.rockbox_channel = channel;
                    if self.config_writable {
                        let _ = write_json(&self.dirs.config_file, &self.cfg);
                    }
                    self.latest = None;
                    self.themes.clear();
                    (self.emit)(UiEvent::RockboxChannel(channel));
                    self.rockbox_check();
                }
            }
            Msg::Mounted(result) => {
                self.mounting = false;
                if let Err(e) = result {
                    self.note = Some(CardNote::MountError(e));
                }
                self.refresh_card();
            }
            Msg::Free(free) => {
                self.free = free;
                self.refresh_card();
            }
            Msg::CopyStarted(id) => {
                if let Some(job) = self.job_mut(id) {
                    job.stage = Stage::Copying;
                    self.show(id);
                }
            }
            Msg::Copied(id) => {
                self.batch_copied += 1;
                (self.emit)(UiEvent::SongsCopied);
                if let Some(job) = id.and_then(|id| self.job_mut(id)) {
                    job.stage = Stage::Copied;
                    job.outbox_file = None;
                }
                self.show_all();
                self.refresh_waiting();
            }
            Msg::CopyFailed { id, reason } => {
                self.transfer_failed = true;
                if let Some(job) = id.and_then(|id| self.job_mut(id)) {
                    job.stage = Stage::Failed;
                    job.reason = reason;
                }
                self.show_all();
            }
            Msg::TransferDone => self.transfer_done(),
            Msg::Ejected(result) => {
                self.ejecting = false;
                match result {
                    Ok(()) => {
                        self.note = Some(CardNote::Safe);
                        self.ejected_block = self.card.as_ref().map(|c| c.block.clone());
                    }
                    Err(e) => self.note = Some(CardNote::EjectError(e)),
                }
                self.refresh_card();
            }
            Msg::Eject => self.eject(),
            Msg::ToolStaged(staged) => self.staged.push(staged),
            Msg::AppUpdated(version) => {
                self.app_version.clone_from(&version);
                (self.emit)(UiEvent::Updated(version));
            }
            Msg::CheckUpdates => {
                self.manual_update = true;
                (self.emit)(UiEvent::UpdateCheck {
                    checking: true,
                    text: "Checking for updates".into(),
                });
                if self.updating {
                    self.check_again = true;
                } else {
                    self.check_updates();
                }
            }
            Msg::UpdateNote(text) => (self.emit)(UiEvent::UpdateCheck {
                checking: true,
                text,
            }),
            Msg::UpdatesDone(report) => {
                self.updating = false;
                if std::mem::take(&mut self.check_again) {
                    self.check_updates();
                } else {
                    let text = if std::mem::take(&mut self.manual_update) {
                        report.summary()
                    } else {
                        String::new()
                    };
                    (self.emit)(UiEvent::UpdateCheck {
                        checking: false,
                        text,
                    });
                }
            }
            Msg::Shutdown(ack) => {
                self.stopping = true;
                self.stop_transfer.store(true, Ordering::SeqCst);
                for cancel in self.cancels.values() {
                    cancel.cancel();
                }
                if let Some(client) = self.soulseek.take() {
                    client.stop();
                }
                self.rockbox_queue.clear();
                self.dirty = true;
                self.flush();
                if self.rockbox_busy {
                    self.rockbox_status(true, "Finishing the Rockbox install".into());
                }
                let _ = ack.send(());
            }
        }
    }

    fn cancel(&mut self, id: u64) {
        let Some(job) = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .filter(|j| j.can_cancel())
        else {
            return;
        };
        let fixing = job.stage == Stage::Fixing && job.running;
        job.stage = Stage::Canceled;
        job.running = fixing;
        if !fixing {
            job.downloaded = None;
        }
        if let Some(cancel) = self.cancels.remove(&id) {
            cancel.cancel();
        }
        // The worker sleeps between polls; cancel now so a retry can enqueue the file again.
        if let (Some(client), Some(transfer)) = (
            self.soulseek.clone(),
            job.soulseek.as_ref().and_then(|s| s.transfer.clone()),
        ) {
            spawn("soulseek-cancel", move || {
                let _ = client.cancel(&transfer);
            });
        }
        self.show_all();
    }

    fn discovered(&mut self, id: u64, result: Result<Found, String>) {
        self.cancels.remove(&id);
        let Some(pos) = self
            .jobs
            .iter()
            .position(|j| j.id == id && j.stage == Stage::Finding)
        else {
            return;
        };
        self.jobs[pos].running = false;
        match result {
            Err(reason) if self.stopping && reason == "Canceled" => {}
            Err(reason) => {
                self.jobs[pos].stage = Stage::Failed;
                self.jobs[pos].reason = reason;
            }
            Ok(Found::Video { title }) => {
                let job = &mut self.jobs[pos];
                job.title = title;
                job.found = true;
                job.stage = Stage::Waiting;
            }
            Ok(Found::Playlist { entries }) => {
                let playlist_url = self.jobs.remove(pos).url;
                for (offset, entry) in entries.into_iter().enumerate() {
                    let mut job = Job::new(self.next_id, entry.url.clone());
                    self.next_id += 1;
                    job.title = entry.title;
                    job.found = true;
                    job.stage = Stage::Waiting;
                    if entry.index.is_some() {
                        job.url = playlist_url.clone();
                        job.playlist_index = entry.index;
                    }
                    self.jobs.insert(pos + offset, job);
                }
            }
        }
        self.show_all();
    }

    fn downloaded(&mut self, id: u64, result: Result<PathBuf, String>) {
        self.cancels.remove(&id);
        let stopping = self.stopping;
        let Some(job) = self.job_mut(id).filter(|j| j.stage == Stage::Downloading) else {
            // Canceled while downloading: nothing will fix this file.
            if let Ok(file) = result
                && let Some(dir) = file.parent()
            {
                let _ = fs::remove_dir_all(dir);
            }
            return;
        };
        job.running = false;
        job.processing = false;
        match result {
            Err(reason) if stopping && reason == "Canceled" => {}
            Ok(file) => {
                job.downloaded = Some(file);
                job.stage = Stage::Fixing;
            }
            Err(reason) => {
                job.stage = Stage::Failed;
                job.reason = reason;
            }
        }
        self.show_all();
    }

    fn fixed(&mut self, id: u64, result: Result<PathBuf, String>) {
        self.cancels.remove(&id);
        let Some(job) = self.job_mut(id).filter(|j| j.stage == Stage::Fixing) else {
            // Canceled while fixing: the result must not reach the card.
            if let Ok(file) = result {
                let _ = fs::remove_file(file);
            }
            return;
        };
        job.running = false;
        match result {
            Ok(outbox_file) => {
                job.stage = Stage::WaitingCard;
                job.outbox_file = Some(outbox_file);
                if let Some(file) = job.downloaded.take()
                    && let Some(dir) = file.parent()
                {
                    let _ = fs::remove_dir_all(dir);
                }
                self.note = None;
                self.transfer_failed = false;
            }
            Err(reason) => {
                job.stage = Stage::Failed;
                job.reason = reason;
            }
        }
        self.show_all();
        self.refresh_waiting();
    }

    // Player and Rockbox ------------------------------------------------------

    fn rockbox_status(&self, busy: bool, text: String) {
        (self.emit)(UiEvent::RockboxStatus { busy, text });
    }

    fn on_volumes(&mut self, result: Result<Volumes, String>) {
        match result {
            Ok(volumes) => {
                self.on_card(Ok(volumes.card));
                self.on_internal(volumes.internal);
            }
            Err(e) => {
                self.on_card(Err(e));
                self.on_internal(None);
            }
        }
    }

    fn on_internal(&mut self, info: Option<CardInfo>) {
        let previous = self.internal.take();
        let appeared = info
            .as_ref()
            .is_some_and(|i| previous.as_ref().is_none_or(|p| p.block != i.block));
        if self.flash_pending {
            if info.is_none() {
                self.flash_gone = true;
            } else if self.flash_gone {
                self.flash_pending = false;
                (self.emit)(UiEvent::FlashPending(false));
            }
        }
        match &info {
            None => {
                self.internal_ejected = None;
                self.internal_mount_failed = false;
            }
            Some(i) if appeared => {
                self.internal_mount_failed = false;
                if self.internal_ejected.as_ref() != Some(&i.block) {
                    self.internal_ejected = None;
                }
            }
            Some(_) => {}
        }
        if let Some(i) = &info
            && i.mount.is_none()
            && !self.internal_mounting
            && !self.internal_mount_failed
            && self.internal_ejected.as_ref() != Some(&i.block)
            && let Some(udisks) = self.udisks.clone()
        {
            self.internal_mounting = true;
            let (tx, card) = (self.tx.clone(), i.clone());
            spawn("mount-player", move || {
                let _ = tx.send(Msg::InternalMounted(udisks.mount(&card)));
            });
        }
        self.internal = info;
        self.refresh_player();
        if appeared && self.rockbox_checks {
            self.rockbox_check();
        }
    }

    fn refresh_player(&mut self) {
        self.player_generation += 1;
        let generation = self.player_generation;
        let volumes = Volumes {
            card: self.card.clone(),
            internal: self.internal.clone(),
        };
        let (mtp, tx) = (self.mtp, self.tx.clone());
        spawn("player-state", move || {
            let state = player::read_state(&volumes, mtp);
            let autoupdate = state
                .internal
                .as_ref()
                .and_then(|v| rockbox::read_database_autoupdate(&v.mount));
            let _ = tx.send(Msg::Autoupdate(generation, autoupdate));
            let _ = tx.send(Msg::Player(generation, state));
        });
    }

    fn installed_version(&self) -> Option<&str> {
        match &self.player.as_ref()?.firmware {
            Firmware::Rockbox { version } => Some(version),
            _ => None,
        }
    }

    fn emit_latest(&mut self) {
        let Some(latest) = &self.latest else {
            return;
        };
        let update_available = self
            .installed_version()
            .is_some_and(|v| rockbox::update_available(v, latest));
        let key = (
            latest.channel,
            latest.version.clone(),
            update_available,
            rockbox::has_previous(&self.dirs.rockbox),
        );
        if self.last_latest.as_ref() != Some(&key) {
            self.last_latest = Some(key.clone());
            (self.emit)(UiEvent::RockboxLatest {
                channel: key.0,
                version: key.1,
                update_available: key.2,
                can_roll_back: key.3,
            });
        }
    }

    fn rockbox_check(&mut self) {
        if self.rockbox_checking {
            return;
        }
        self.rockbox_checking = true;
        let (tx, dir, channel) = (
            self.tx.clone(),
            self.dirs.rockbox.clone(),
            self.cfg.rockbox_channel,
        );
        spawn("rockbox-check", move || {
            let result =
                rockbox::fetch_build_info(&net::agent(), &dir).and_then(|info| info.build(channel));
            let _ = tx.send(Msg::RockboxChecked { channel, result });
        });
    }

    fn list_themes(&self) {
        let version = self.installed_version().map(str::to_string);
        let latest = self.latest.as_ref().map(|b| b.version.clone());
        let (tx, dir, channel) = (
            self.tx.clone(),
            self.dirs.rockbox.clone(),
            self.cfg.rockbox_channel,
        );
        spawn("rockbox-themes", move || {
            let agent = net::agent();
            let result = version
                .or(latest)
                .map(Ok)
                .unwrap_or_else(|| {
                    rockbox::fetch_build_info(&agent, &dir)
                        .and_then(|info| info.build(channel))
                        .map(|build| build.version)
                })
                .and_then(|version| rockbox::list_themes(&agent, &version));
            let _ = tx.send(Msg::ThemesListed(result));
        });
    }

    /// Runs the next queued Rockbox job once no song copy or other job is running.
    fn start_rockbox(&mut self) {
        let blocked = self.rockbox_busy || self.transferring || self.ejecting;
        if blocked || self.stopping || self.rockbox_queue.is_empty() {
            return;
        }
        let Some(job) = self.rockbox_queue.pop_front() else {
            return;
        };
        let root = self.internal.as_ref().and_then(|i| i.mount.clone());
        let Some(root) = root else {
            let text = if self.mtp {
                "The player is in MTP mode. On the player, set Settings > System Settings > USB Mode to MSC."
            } else {
                "The player is not connected."
            };
            self.rockbox_status(false, text.into());
            return;
        };
        let Some(guard) = self.gate.acquire() else {
            return;
        };
        self.rockbox_busy = true;
        self.rockbox_status(true, "Starting".into());
        let tx = self.tx.clone();
        let (dirs, channel, themes) = (
            self.dirs.clone(),
            self.cfg.rockbox_channel,
            self.themes.clone(),
        );
        let work = move || {
            let note = |text: &str| {
                let _ = tx.send(Msg::RockboxNote(text.into()));
            };
            // The drive handle closes inside the closure, before the eject can start.
            let run = || {
                Drive::open(&root).and_then(|drive| {
                    run_rockbox(&job, &dirs.rockbox, &drive, channel, &themes, &note)
                })
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
                .unwrap_or_else(|_| Err("The Rockbox job stopped unexpectedly".into()));
            let _ = tx.send(Msg::RockboxDone { job, result });
            drop(guard);
        };
        if let Err(e) = (self.spawner)("rockbox", Box::new(work)) {
            // The closure and its gate guard are gone, so shutdown is not held up.
            self.rockbox_busy = false;
            self.rockbox_status(false, format!("Could not start the Rockbox job: {e}"));
        }
    }

    /// The player is free for the next Rockbox job or song copy.
    fn rockbox_idle(&mut self) {
        self.rockbox_busy = false;
    }

    /// Unmounts the internal drive and powers it off, so the player can restart.
    /// The job stays busy until `InternalEjected` arrives.
    fn eject_internal(&mut self, done: String, flash: bool) {
        let (Some(eject), Some(card)) = (self.ejector.clone(), self.internal.clone()) else {
            self.rockbox_status(false, done);
            self.rockbox_idle();
            return;
        };
        self.rockbox_status(true, "Ejecting the player".into());
        let tx = self.tx.clone();
        let text = if flash {
            done
        } else {
            format!("{done} Safe to unplug. Restart the player to use it.")
        };
        spawn("eject-player", move || {
            rustix::fs::sync();
            let _ = tx.send(Msg::InternalEjected {
                result: eject(&card),
                text,
                flash,
            });
        });
    }

    // Card ---------------------------------------------------------------

    fn on_card(&mut self, result: Result<Option<CardInfo>, String>) {
        match result {
            Err(e) => {
                self.card = None;
                self.note = Some(CardNote::MountError(e));
            }
            Ok(None) => {
                if self.card.is_some() {
                    self.free = None;
                    self.mounting = false;
                    self.ejected_block = None;
                    if !matches!(self.note, Some(CardNote::Safe)) {
                        self.note = None;
                    }
                }
                self.card = None;
                self.ejected_block = None;
            }
            Ok(Some(info)) => {
                let appeared = self.card.as_ref().is_none_or(|c| c.block != info.block);
                let newly_mounted = info.mount.is_some()
                    && (appeared || self.card.as_ref().is_some_and(|c| c.mount.is_none()));
                if appeared && self.ejected_block.as_ref() != Some(&info.block) {
                    self.note = None;
                    self.free = None;
                    self.ejected_block = None;
                    self.transfer_failed = false;
                }
                if self.ejected_block.as_ref() == Some(&info.block) {
                    self.note = Some(CardNote::Safe);
                }
                if newly_mounted {
                    self.transfer_failed = false;
                    self.card_cleanup_pending = true;
                }
                self.card = Some(info.clone());
                let wanted = appeared || self.last_waiting.unwrap_or(0) > 0;
                let skip = self.mounting
                    || self.ejecting
                    || self.ejected_block.as_ref() == Some(&info.block)
                    || matches!(self.note, Some(CardNote::MountError(_)));
                if info.mount.is_none() && wanted && !skip {
                    self.start_mount(info);
                }
            }
        }
        self.refresh_card();
    }

    fn read_free_space(&self, mount: PathBuf) {
        let tx = self.tx.clone();
        spawn("free-space", move || {
            let _ = tx.send(Msg::Free(transfer::free_space(&mount)));
        });
    }

    fn start_mount(&mut self, card: CardInfo) {
        let Some(udisks) = self.udisks.clone() else {
            return;
        };
        self.mounting = true;
        let tx = self.tx.clone();
        spawn("mount", move || {
            let _ = tx.send(Msg::Mounted(udisks.mount(&card)));
        });
    }

    fn eject(&mut self) {
        if self.card.is_none() || self.ejecting {
            return;
        }
        self.note = None;
        if self.transferring {
            self.stop_transfer.store(true, Ordering::SeqCst);
            self.eject_after_copy = true;
        } else {
            self.run_eject();
        }
        self.refresh_card();
    }

    /// Waits while a Rockbox job holds the player.
    fn run_eject(&mut self) {
        if self.rockbox_busy {
            self.eject_deferred = true;
            return;
        }
        let (Some(eject), Some(card)) = (self.ejector.clone(), self.card.clone()) else {
            return;
        };
        self.ejecting = true;
        let tx = self.tx.clone();
        spawn("eject", move || {
            rustix::fs::sync();
            let _ = tx.send(Msg::Ejected(eject(&card)));
        });
    }

    fn transfer_done(&mut self) {
        self.transferring = false;
        self.stop_transfer.store(false, Ordering::SeqCst);
        let stopped = self.eject_after_copy;
        let card_present = self.card.as_ref().is_some_and(|c| c.mount.is_some());
        let more = !outbox_files(&self.dirs.outbox).is_empty();
        let batch_over = stopped || !card_present || !more || self.transfer_failed;
        if batch_over && self.batch_copied > 0 {
            let count = std::mem::take(&mut self.batch_copied);
            if self.ejected_block.is_none() && !stopped {
                let tx = self.tx.clone();
                notify::songs_copied(count, move || {
                    let _ = tx.send(Msg::Eject);
                });
            }
        }
        // A transfer that failed for a reason on the card goes back to waiting.
        for job in &mut self.jobs {
            if job.stage == Stage::Copying {
                job.stage = Stage::WaitingCard;
            }
        }
        self.show_all();
        if let Some(card) = &self.card
            && let Some(mount) = &card.mount
        {
            self.read_free_space(mount.clone());
        }
        if self.eject_after_copy {
            self.eject_after_copy = false;
            self.run_eject();
        }
        self.refresh_waiting();
        self.refresh_player();
    }

    fn start_transfer(&mut self, mount: &Path) {
        let files = outbox_files(&self.dirs.outbox);
        let work: Vec<(Option<u64>, PathBuf)> = files
            .into_iter()
            .map(|file| {
                let id = self
                    .jobs
                    .iter()
                    .find(|j| j.outbox_file.as_ref() == Some(&file))
                    .map(|j| j.id);
                (id, file)
            })
            .collect();
        self.transferring = true;
        self.card_cleanup_pending = false;
        let tx = self.tx.clone();
        let stop = self.stop_transfer.clone();
        let tracks = mount.join("Tracks");
        spawn("transfer", move || {
            let device = transfer::mount_device(tracks.parent().unwrap());
            if device.is_ok() {
                clean_card_temps(&tracks);
            }
            let _ = tx.send(Msg::Free(transfer::free_space(tracks.parent().unwrap())));
            for (id, file) in work {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                if let Some(id) = id {
                    let _ = tx.send(Msg::CopyStarted(id));
                }
                match device
                    .as_ref()
                    .map_err(|e| std::io::Error::other(e.to_string()))
                    .and_then(|device| transfer::copy_to_card_device(&file, &tracks, *device))
                {
                    Ok(_) => {
                        let _ = tx.send(Msg::Copied(id));
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::CopyFailed {
                            id,
                            reason: e.to_string(),
                        });
                        break;
                    }
                }
            }
            let _ = tx.send(Msg::TransferDone);
        });
    }

    // Scheduling -----------------------------------------------------------

    fn busy(&self) -> bool {
        self.jobs.iter().any(|j| j.running)
    }

    fn check_updates(&mut self) {
        self.updating = true;
        spawn_updater(
            self.tx.clone(),
            self.dirs.clone(),
            self.tools.clone(),
            self.appimage.clone(),
            self.app_version.clone(),
            // Their stage folder is still waiting to be installed.
            self.staged.iter().map(|s| s.tool.key()).collect(),
            true,
        );
    }

    fn swap_tools(&mut self) {
        if self.staged.is_empty() || self.busy() {
            return;
        }
        for staged in self.staged.drain(..) {
            if let Err(e) = tools::install(&staged, &self.dirs.tools) {
                eprintln!(
                    "clipplus-manager: could not install {}: {e}",
                    staged.tool.key()
                );
            }
        }
        self.tools = Resolved::load(&self.dirs.tools, self.bundled.as_deref());
        self.refresh_tools();
    }

    fn schedule(&mut self) {
        if self.stopping {
            return;
        }
        self.swap_tools();
        if self.staged.is_empty() {
            self.start_jobs();
        }
        self.start_rockbox();
        if self.eject_deferred && !self.rockbox_busy && self.rockbox_queue.is_empty() {
            self.eject_deferred = false;
            self.run_eject();
        }
        let waiting = self.last_waiting.unwrap_or(0) > 0;
        let idle = !self.ejecting && !self.eject_after_copy && self.ejected_block.is_none();
        if (waiting || self.card_cleanup_pending)
            && idle
            && !self.transferring
            && !self.rockbox_busy
            && !self
                .jobs
                .iter()
                .any(|j| j.running && matches!(j.stage, Stage::Fixing | Stage::Canceled))
            && !self.transfer_failed
            && let Some(mount) = self.card.as_ref().and_then(|c| c.mount.clone())
        {
            self.start_transfer(&mount);
        }
        if waiting
            && idle
            && !self.mounting
            && !matches!(self.note, Some(CardNote::MountError(_)))
            && let Some(card) = self.card.clone()
            && card.mount.is_none()
        {
            self.start_mount(card);
        }
        self.refresh_card();
    }

    fn start_jobs(&mut self) {
        let (Some(ytdlp), Some(deno), Some(ffmpeg)) =
            (self.tools.ytdlp(), self.tools.deno(), self.tools.ffmpeg())
        else {
            return;
        };
        let mut finding = self
            .jobs
            .iter()
            .filter(|j| j.running && j.stage == Stage::Finding)
            .count();
        let mut downloading = self
            .jobs
            .iter()
            .filter(|j| j.running && j.stage == Stage::Downloading)
            .count();
        let mut changed = false;
        for i in 0..self.jobs.len() {
            if self.jobs[i].running {
                continue;
            }
            let (id, stage, generation) =
                (self.jobs[i].id, self.jobs[i].stage, self.jobs[i].generation);
            let cancel = Arc::new(Cancel::default());
            let tx = self.tx.clone();
            match stage {
                Stage::Finding if finding < MAX_FINDING => {
                    finding += 1;
                    let (url, ytdlp, deno, cancel) = (
                        self.jobs[i].url.clone(),
                        ytdlp.clone(),
                        deno.clone(),
                        cancel.clone(),
                    );
                    spawn("find", move || {
                        let result = discover(&ytdlp, &deno, &url, &cancel);
                        let _ = tx.send(Msg::Discovered {
                            id,
                            generation,
                            result,
                        });
                    });
                }
                Stage::Waiting if downloading < MAX_DOWNLOADS => {
                    if let Some(source) = self.jobs[i].soulseek.clone() {
                        let Some(client) = self.soulseek.clone() else {
                            continue;
                        };
                        downloading += 1;
                        self.jobs[i].stage = Stage::Downloading;
                        self.jobs[i].fraction = None;
                        self.jobs[i].speed = None;
                        let dir = self.dirs.downloads.join(format!("{id}-{generation}"));
                        let worker_cancel = cancel.clone();
                        spawn("soulseek-download", move || {
                            let result = soulseek_download(
                                &client,
                                &source,
                                &dir,
                                id,
                                generation,
                                &worker_cancel,
                                &tx,
                            );
                            let _ = tx.send(Msg::Downloaded {
                                id,
                                generation,
                                result,
                            });
                        });
                        self.jobs[i].running = true;
                        self.cancels.insert(id, cancel);
                        changed = true;
                        continue;
                    }
                    downloading += 1;
                    self.jobs[i].stage = Stage::Downloading;
                    self.jobs[i].fraction = None;
                    self.jobs[i].speed = None;
                    let spec = DownloadJob {
                        id,
                        url: self.jobs[i].url.clone(),
                        playlist_index: self.jobs[i].playlist_index,
                        dir: self.dirs.downloads.join(format!("{id}-{generation}")),
                        generation,
                        ytdlp: ytdlp.clone(),
                        deno: deno.clone(),
                        ffmpeg_dir: ffmpeg
                            .ffmpeg
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_default(),
                        sponsorblock: self.cfg.sponsorblock,
                    };
                    let cancel = cancel.clone();
                    spawn("download", move || {
                        let result = download(&spec, &cancel, &tx);
                        let _ = tx.send(Msg::Downloaded {
                            id,
                            generation,
                            result,
                        });
                    });
                }
                Stage::Fixing => {
                    let Some(file) = self.jobs[i].downloaded.clone() else {
                        self.jobs[i].stage = Stage::Waiting;
                        changed = true;
                        continue;
                    };
                    let (ffmpeg, outbox) = (ffmpeg.clone(), self.dirs.outbox.clone());
                    let metadata = self.jobs[i].metadata.clone();
                    spawn("fix", move || {
                        let result =
                            fixer::fix_with_metadata(&ffmpeg, &file, &outbox, metadata.as_ref());
                        let _ = tx.send(Msg::Fixed {
                            id,
                            generation,
                            result,
                        });
                    });
                }
                _ => continue,
            }
            self.jobs[i].running = true;
            self.cancels.insert(id, cancel);
            changed = true;
        }
        if changed {
            self.show_all();
        }
    }
}

// Workers ------------------------------------------------------------------

/// Drops slskd files that no job waits for. Call while slskd is stopped.
fn clean_soulseek_files(data: &Path, keep: &[String]) {
    let _ = fs::remove_dir_all(data.join("incomplete"));
    if let Ok(entries) = fs::read_dir(data.join("downloads")) {
        for entry in entries.flatten() {
            if !keep.iter().any(|d| entry.file_name() == d.as_str()) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

fn soulseek_download(
    client: &Slskd,
    source: &queue::SoulseekJob,
    dir: &Path,
    id: u64,
    generation: u64,
    cancel: &Cancel,
    tx: &Sender<Msg>,
) -> Result<PathBuf, String> {
    let mut recovered = source
        .transfer
        .as_ref()
        .and_then(|transfer| client.completed_file(transfer));
    let transfer = match (&source.transfer, recovered.is_some()) {
        (Some(transfer), true) => transfer.clone(),
        _ => {
            let transfer = client.enqueue(&source.peer)?;
            let _ = tx.send(Msg::SoulseekQueued {
                id,
                generation,
                transfer: transfer.clone(),
            });
            transfer
        }
    };
    let mut finishing_since = None;
    let mut last_progress = -1.0;
    let mut delay = Duration::from_secs(2);
    loop {
        if cancel.is_cancelled() {
            let _ = client.cancel(&transfer);
            return Err("Canceled".into());
        }
        let status = match recovered.take() {
            Some(path) => TransferStatus::Complete(path),
            None => client.transfer(&transfer)?,
        };
        match status {
            TransferStatus::Failed(reason) => return Err(reason),
            TransferStatus::Active { progress } => {
                if progress >= 1.0 {
                    let since = finishing_since.get_or_insert_with(Instant::now);
                    if since.elapsed() > Duration::from_secs(60) {
                        return Err("Downloaded file did not arrive".into());
                    }
                } else {
                    finishing_since = None;
                }
                // slskd has no transfer events, so poll, and slow down while a peer queues us.
                delay = if progress != last_progress {
                    Duration::from_secs(2)
                } else {
                    (delay * 2).min(Duration::from_secs(30))
                };
                last_progress = progress;
                let _ = tx.send(Msg::Progress {
                    id,
                    generation,
                    fraction: Some(progress.clamp(0.0, 1.0)),
                    speed: None,
                    processing: false,
                });
            }
            TransferStatus::Complete(source) => {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                let name = source.file_name().ok_or("Downloaded file has no name")?;
                let target = dir.join(name);
                fs::rename(&source, &target).map_err(|e| format!("Move downloaded file: {e}"))?;
                if let Some(folder) = source.parent() {
                    let _ = fs::remove_dir(folder);
                }
                return Ok(target);
            }
        }
        std::thread::sleep(delay);
    }
}

fn discover(ytdlp: &Path, deno: &Path, url: &str, cancel: &Cancel) -> Result<Found, String> {
    let args = ytdlp::discovery_args(url, Some(deno));
    let mut json = String::new();
    let out = proc::run(ytdlp, &args, cancel, |line| json.push_str(line))
        .map_err(|e| format!("yt-dlp: {e}"))?;
    if cancel.is_cancelled() {
        return Err("Canceled".into());
    }
    if !out.success {
        return Err(failure(url, &out.stderr));
    }
    ytdlp::parse_discovery(&json)
}

/// The row gets plain text; the full yt-dlp message goes to the log.
fn failure(url: &str, stderr: &str) -> String {
    eprintln!(
        "clipplus-manager: yt-dlp failed for {url}: {}",
        stderr.trim()
    );
    ytdlp::plain_reason(stderr)
}

struct DownloadJob {
    generation: u64,
    id: u64,
    url: String,
    playlist_index: Option<u32>,
    dir: PathBuf,
    ytdlp: PathBuf,
    deno: PathBuf,
    ffmpeg_dir: PathBuf,
    sponsorblock: bool,
}

fn download(job: &DownloadJob, cancel: &Cancel, tx: &Sender<Msg>) -> Result<PathBuf, String> {
    fs::create_dir_all(&job.dir).map_err(|e| e.to_string())?;
    let args = ytdlp::download_args(&ytdlp::DownloadSpec {
        url: &job.url,
        playlist_index: job.playlist_index,
        dir: &job.dir,
        ffmpeg_dir: &job.ffmpeg_dir,
        deno: Some(&job.deno),
        sponsorblock: job.sponsorblock,
    });
    let mut file: Option<PathBuf> = None;
    let mut last = Instant::now() - PROGRESS_EVERY;
    let mut processing = false;
    let result = proc::run(&job.ytdlp, &args, cancel, |line| {
        match ytdlp::parse_line(line) {
            Line::File(path) => file = Some(path),
            Line::Progress(p) => {
                if last.elapsed() >= PROGRESS_EVERY || p.fraction == Some(1.0) {
                    last = Instant::now();
                    let _ = tx.send(Msg::Progress {
                        id: job.id,
                        generation: job.generation,
                        fraction: p.fraction,
                        speed: p.speed,
                        processing,
                    });
                }
            }
            Line::Processing => {
                processing = true;
                let _ = tx.send(Msg::Progress {
                    id: job.id,
                    generation: job.generation,
                    fraction: Some(1.0),
                    speed: None,
                    processing,
                });
            }
            Line::Other => {}
        }
    });
    let fail = |reason: String| {
        let _ = fs::remove_dir_all(&job.dir);
        Err(reason)
    };
    let out = match result {
        Ok(out) => out,
        Err(e) => return fail(format!("yt-dlp: {e}")),
    };
    if cancel.is_cancelled() {
        return fail("Canceled".into());
    }
    if !out.success {
        return fail(failure(&job.url, &out.stderr));
    }
    let found = file.filter(|f| f.exists()).or_else(|| {
        fs::read_dir(&job.dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "mp3"))
    });
    match found {
        Some(path) => Ok(path),
        None => fail("yt-dlp produced no file".into()),
    }
}

// Rockbox ------------------------------------------------------------------

fn run_rockbox(
    job: &RockboxJob,
    work: &Path,
    drive: &Drive,
    channel: Channel,
    themes: &[Theme],
    progress: &dyn Fn(&str),
) -> Result<String, String> {
    let agent = net::agent();
    let firmware = work.join("firmware");
    let resolve = || {
        let info = rockbox::fetch_build_info(&agent, work)?;
        let build = info.build(channel)?;
        Ok::<_, String>((info, build))
    };
    match job {
        RockboxJob::Update => {
            let (_, build) = resolve()?;
            let version = rockbox::install_build(&agent, work, drive, &build, progress)?;
            Ok(format!("Rockbox {version} is installed."))
        }
        RockboxJob::Rollback => {
            let version = rockbox::install_previous(work, drive, progress)?;
            Ok(format!("Rockbox {version} is installed."))
        }
        RockboxJob::Fonts => {
            let (_, build) = resolve()?;
            rockbox::install_fonts(&agent, work, drive, &build, progress)?;
            Ok("Fonts are installed.".into())
        }
        RockboxJob::Voice => {
            let (info, build) = resolve()?;
            rockbox::install_voice(&agent, work, drive, &info, &build, "english", progress)?;
            Ok("The voice file is installed.".into())
        }
        RockboxJob::Theme(id) => {
            let theme = themes
                .iter()
                .find(|t| &t.id == id)
                .ok_or("Open the theme list first")?;
            rockbox::install_theme(&agent, work, drive, theme, progress)?;
            Ok(format!("Theme {} is installed.", theme.name))
        }
        RockboxJob::Uninstall(full) => {
            let count = rockbox::uninstall(drive, *full)?;
            Ok(format!("Removed {count} Rockbox files."))
        }
        RockboxJob::Bootloader => {
            bootloader::install(&agent, &firmware, drive, progress)?;
            Ok(UNPLUG_TEXT.into())
        }
        RockboxJob::RestoreStock => {
            bootloader::restore_stock(&agent, &firmware, drive, progress)?;
            Ok(RESTORE_TEXT.into())
        }
        RockboxJob::FirstInstall => first_install(
            || {
                let (_, build) = resolve()?;
                rockbox::install_build(&agent, work, drive, &build, progress).map(drop)
            },
            || bootloader::install(&agent, &firmware, drive, progress),
        ),
        RockboxJob::Autoupdate(on) => {
            rockbox::set_database_autoupdate(drive, *on)?;
            let state = if *on { "on" } else { "off" };
            Ok(format!("Database auto update is {state}."))
        }
    }
}

// Updates ------------------------------------------------------------------

/// Records the check and says whether it should run. `force` skips the daily limit.
/// The build goes on first, and the bootloader file only after it succeeded,
/// so the player never flashes a bootloader with no Rockbox behind it.
fn first_install(
    build: impl FnOnce() -> Result<(), String>,
    bootloader: impl FnOnce() -> Result<(), String>,
) -> Result<String, String> {
    build()?;
    bootloader()?;
    Ok(UNPLUG_TEXT.into())
}

fn record_check(state: &mut State, key: &str, now: u64, force: bool) -> bool {
    if !force && !state.due(key, now) {
        return false;
    }
    state.checked.insert(key.into(), now);
    true
}

/// What one update run did.
#[derive(Debug, Default)]
struct UpdateReport {
    updated: Vec<String>,
    app: bool,
    errors: Vec<String>,
}

impl UpdateReport {
    fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.updated.is_empty() {
            parts.push(format!("Updated {}.", self.updated.join(", ")));
        }
        if !self.errors.is_empty() {
            parts.push(format!("Could not update {}.", self.errors.join("; ")));
        }
        if parts.is_empty() && !self.app {
            parts.push("Everything is up to date.".into());
        }
        parts.join(" ")
    }
}

fn spawn_updater(
    tx: Sender<Msg>,
    dirs: Dirs,
    resolved: Resolved,
    appimage: Option<PathBuf>,
    app_version: String,
    staged: Vec<&'static str>,
    force: bool,
) {
    spawn("updates", move || {
        let agent = net::agent();
        let mut state: State = read_json(&dirs.state_file);
        let mut report = UpdateReport::default();
        let note = |text: String| {
            let _ = tx.send(Msg::UpdateNote(text));
        };
        for tool in Tool::ALL {
            let have = resolved.version(tool).map(str::to_string);
            if staged.contains(&tool.key())
                || !record_check(&mut state, tool.key(), now_secs(), force)
            {
                continue;
            }
            let _ = write_json(&dirs.state_file, &state);
            let staged = tools::fetch_latest(&agent, tool).and_then(|latest| {
                if have
                    .as_deref()
                    .is_none_or(|h| tools::is_newer(&latest.version, h))
                {
                    note(format!("Downloading {} {}", tool.key(), latest.version));
                    tools::stage(&agent, tool, &latest, &dirs.tools).map(Some)
                } else {
                    Ok(None)
                }
            });
            match staged {
                Ok(Some(staged)) => {
                    report
                        .updated
                        .push(format!("{} to {}", tool.key(), staged.version));
                    let _ = tx.send(Msg::ToolStaged(staged));
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("clipplus-manager: {} update: {e}", tool.key());
                    report.errors.push(format!("{}: {e}", tool.key()));
                }
            }
        }
        let slskd_root = dirs.tools.join("slskd");
        if let Some(installed) = crate::slskd_tools::find(&slskd_root)
            && record_check(&mut state, "slskd", now_secs(), force)
        {
            let _ = write_json(&dirs.state_file, &state);
            let update = crate::slskd_tools::fetch_latest(&agent).and_then(|latest| {
                if tools::is_newer(&latest.version, &installed.version) {
                    note(format!("Downloading slskd {}", latest.version));
                    crate::slskd_tools::install(&agent, &latest, &slskd_root)
                        .map(|_| Some(latest.version))
                } else {
                    Ok(None)
                }
            });
            match update {
                Ok(Some(version)) => report.updated.push(format!("slskd to {version}")),
                Ok(None) => {}
                Err(e) => {
                    eprintln!("clipplus-manager: slskd update: {e}");
                    report.errors.push(format!("slskd: {e}"));
                }
            }
        }
        if let Some(appimage) = appimage {
            let now = now_secs();
            if force || state.due("app", now) {
                let result = selfupdate::update(&agent, &appimage, &app_version, &|v| {
                    note(format!("Downloading ClipPlus Manager {v}"))
                });
                // Saved after the attempt, so quitting mid-download retries next launch.
                state.checked.insert("app".into(), now);
                let _ = write_json(&dirs.state_file, &state);
                match result {
                    Ok(Some(version)) => {
                        report.app = true;
                        let _ = tx.send(Msg::AppUpdated(version));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("clipplus-manager: app update: {e}");
                        report.errors.push(format!("ClipPlus Manager: {e}"));
                    }
                }
            }
        }
        let _ = tx.send(Msg::UpdatesDone(report));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_waits_for_overlapping_jobs() {
        let gate = Gate::default();
        let first = gate.acquire().unwrap();
        let second = gate.acquire().unwrap();
        drop(first);
        let (tx, rx) = std::sync::mpsc::channel();
        let waiter = gate.clone();
        let handle = std::thread::spawn(move || {
            waiter.wait_idle();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(second);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn soulseek_cleanup_keeps_only_awaited_downloads() {
        let data = tempfile::tempdir().unwrap();
        for dir in ["downloads/kept", "downloads/stale", "incomplete/partial"] {
            fs::create_dir_all(data.path().join(dir)).unwrap();
        }
        clean_soulseek_files(data.path(), &["kept".into()]);
        assert!(data.path().join("downloads/kept").exists());
        assert!(!data.path().join("downloads/stale").exists());
        assert!(!data.path().join("incomplete").exists());
    }

    #[test]
    fn update_summary_names_what_changed() {
        let mut report = UpdateReport::default();
        assert_eq!(report.summary(), "Everything is up to date.");
        report.app = true;
        assert_eq!(report.summary(), "");
        report.updated.push("deno to 2.5.0".into());
        report.errors.push("yt-dlp: offline".into());
        assert_eq!(
            report.summary(),
            "Updated deno to 2.5.0. Could not update yt-dlp: offline."
        );
    }

    #[test]
    fn stale_attempt_does_not_match_current_job() {
        let mut job = Job::new(1, "https://example.test".into());
        job.stage = Stage::Downloading;
        job.generation = 2;
        assert!(!current_attempt(&job, 1, 1, Stage::Downloading));
        assert!(current_attempt(&job, 1, 2, Stage::Downloading));
    }

    #[test]
    fn missing_tool_check_is_recorded_before_download() {
        let mut state = State::default();
        assert!(record_check(&mut state, "yt-dlp", 100, false));
        assert!(!record_check(&mut state, "yt-dlp", 101, false));
        assert!(record_check(&mut state, "yt-dlp", 102, true));
        assert!(!record_check(&mut state, "yt-dlp", 103, false));
        assert!(record_check(&mut state, "yt-dlp", 102 + 86400, false));
    }

    #[test]
    fn cleans_only_matching_card_temps() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join(fixer::temp_name());
        let other = dir.path().join(".sansa-other.part");
        fs::write(&temp, b"x").unwrap();
        fs::write(&other, b"x").unwrap();
        clean_card_temps(dir.path());
        assert!(!temp.exists());
        assert!(other.exists());
    }

    #[test]
    fn free_space_text() {
        assert_eq!(format_free(114_300_000_000), "114 GB");
        assert_eq!(format_free(9_540_000_000), "9.5 GB");
        assert_eq!(format_free(2_100_000_000_000), "2.1 TB");
        assert_eq!(format_free(512_000_000), "512 MB");
    }

    #[test]
    fn outbox_listing_skips_temp_files_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.mp3"), b"a").unwrap();
        fs::write(dir.path().join(fixer::temp_name()), b"t").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        let files = outbox_files(dir.path());
        assert_eq!(files, vec![dir.path().join("a.mp3")]);
    }

    #[test]
    fn stale_download_folders_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["1", "2", "junk"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        let mut kept = Job::new(1, "https://a.example".into());
        kept.downloaded = Some(dir.path().join("1/x.mp3"));
        let other = Job::new(2, "https://b.example".into());
        remove_stale_downloads(dir.path(), &[kept, other]);
        assert!(dir.path().join("1").exists());
        assert!(!dir.path().join("2").exists());
        assert!(!dir.path().join("junk").exists());
    }

    #[test]
    fn download_folder_ids_are_read_from_names() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["3-0", "7-2", "junk", "12"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        let mut ids = download_folder_ids(dir.path());
        ids.sort_unstable();
        assert_eq!(ids, [3, 7, 12]);
        assert!(download_folder_ids(&dir.path().join("missing")).is_empty());
    }

    struct Rig {
        core: Core,
        tx: Sender<Msg>,
        rx: Receiver<Msg>,
        events: Arc<std::sync::Mutex<Vec<UiEvent>>>,
        _dir: tempfile::TempDir,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let dirs = Dirs::under(&dir.path().join("data"), &dir.path().join("config"));
        dirs.create().unwrap();
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = events.clone();
        let emit: Emit = Arc::new(move |event| log.lock().unwrap().push(event));
        let options = Options {
            card: false,
            updates: false,
            bundled_tools: None,
        };
        let (tx, rx) = channel();
        let (core, _) = build_core(&dirs, &None, emit, &options, &tx, Gate::default());
        Rig {
            core,
            tx,
            rx,
            events,
            _dir: dir,
        }
    }

    fn volume(mount: &str) -> CardInfo {
        CardInfo {
            block: format!("/block{mount}"),
            device: "/dev/sdb1".into(),
            drive: None,
            mount: Some(PathBuf::from(format!("/nonexistent/{mount}"))),
        }
    }

    #[test]
    fn the_player_stays_busy_until_the_update_eject_is_over() {
        let mut rig = rig();
        let core = &mut rig.core;
        core.internal = Some(volume("internal"));
        core.card = Some(volume("card"));
        core.ejector = Some(Arc::new(|_| Ok(())));
        core.rockbox_busy = true;
        core.handle(Msg::RockboxDone {
            job: RockboxJob::Update,
            result: Ok("Rockbox is installed.".into()),
        });
        assert!(core.rockbox_busy, "the eject is still to come");

        core.handle(Msg::RockboxQueue(RockboxJob::Fonts));
        core.start_rockbox();
        assert_eq!(
            core.rockbox_queue.len(),
            1,
            "no job starts during the eject"
        );

        core.handle(Msg::Eject);
        assert!(
            !core.ejecting && core.eject_deferred,
            "the card eject waits too"
        );

        loop {
            let msg = rig.rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if matches!(msg, Msg::InternalEjected { .. }) {
                let core = &mut rig.core;
                core.handle(msg);
                break;
            }
        }
        let core = &mut rig.core;
        assert!(!core.rockbox_busy);
        core.start_rockbox();
        assert!(core.rockbox_queue.is_empty() && core.rockbox_busy);
    }

    #[test]
    fn first_install_writes_the_build_before_the_bootloader() {
        let order = std::cell::RefCell::new(Vec::new());
        let text = first_install(
            || Ok(order.borrow_mut().push("build")),
            || Ok(order.borrow_mut().push("bootloader")),
        )
        .unwrap();
        assert_eq!(*order.borrow(), ["build", "bootloader"]);
        assert_eq!(text, UNPLUG_TEXT);

        let order = std::cell::RefCell::new(Vec::new());
        let result = first_install(
            || Err("no network".into()),
            || Ok(order.borrow_mut().push("bootloader")),
        );
        assert!(result.is_err() && order.borrow().is_empty());
    }

    #[test]
    fn bootloader_jobs_wait_in_the_rockbox_queue_and_need_the_player() {
        let mut rig = rig();
        let core = &mut rig.core;
        for job in [RockboxJob::Bootloader, RockboxJob::RestoreStock] {
            core.handle(Msg::RockboxQueue(job));
        }
        core.rockbox_busy = true;
        core.start_rockbox();
        assert_eq!(core.rockbox_queue.len(), 2, "queued behind a running job");
        core.rockbox_busy = false;
        core.start_rockbox();
        assert_eq!(core.rockbox_queue.len(), 1);
        assert!(!core.rockbox_busy, "no drive, so nothing started");
        let events = rig.events.lock().unwrap();
        assert!(events.iter().any(|e| matches!(
            e,
            UiEvent::RockboxStatus { busy: false, text } if text.contains("not connected")
        )));
    }

    #[test]
    fn the_player_is_ejected_after_a_bootloader_write_and_flash_ends_on_return() {
        let mut rig = rig();
        let core = &mut rig.core;
        core.internal = Some(volume("internal"));
        core.ejector = Some(Arc::new(|_| Ok(())));
        core.rockbox_busy = true;
        core.handle(Msg::RockboxDone {
            job: RockboxJob::Bootloader,
            result: Ok(UNPLUG_TEXT.into()),
        });
        assert!(core.rockbox_busy, "busy until the eject is over");
        loop {
            let msg = rig.rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if matches!(msg, Msg::InternalEjected { .. }) {
                rig.core.handle(msg);
                break;
            }
        }
        let core = &mut rig.core;
        assert!(!core.rockbox_busy && core.flash_pending);
        {
            let events = rig.events.lock().unwrap();
            assert!(events.contains(&UiEvent::FlashPending(true)));
            assert!(events.iter().any(|e| matches!(
                e,
                UiEvent::RockboxStatus { busy: false, text } if text == UNPLUG_TEXT
            )));
        }
        core.on_internal(Some(volume("internal")));
        assert!(core.flash_pending, "still plugged in");
        core.on_internal(None);
        core.on_internal(Some(volume("internal")));
        assert!(!core.flash_pending);
        assert_eq!(
            rig.events.lock().unwrap().last(),
            Some(&UiEvent::FlashPending(false))
        );
    }

    #[test]
    fn no_song_copy_starts_while_a_rockbox_job_holds_the_player() {
        let mut rig = rig();
        let core = &mut rig.core;
        fs::write(core.dirs.outbox.join("a.mp3"), "x").unwrap();
        core.last_waiting = Some(1);
        core.card = Some(volume("card"));
        core.rockbox_busy = true;
        core.schedule();
        assert!(!core.transferring);
        core.rockbox_busy = false;
        core.schedule();
        assert!(core.transferring);
    }

    #[test]
    fn the_coordinator_drops_queued_jobs_on_shutdown() {
        let mut rig = rig();
        let core = &mut rig.core;
        core.rockbox_busy = true;
        core.rockbox_queue.push_back(RockboxJob::Fonts);
        let (ack, done) = channel();
        core.handle(Msg::Shutdown(ack));
        assert!(done.try_recv().is_ok());
        assert!(core.rockbox_queue.is_empty(), "queued jobs do not start");
        let says = rig.events.lock().unwrap().iter().any(|e| {
            matches!(e, UiEvent::RockboxStatus { text, .. } if text == "Finishing the Rockbox install")
        });
        assert!(says);

        let core = &mut rig.core;
        core.handle(Msg::RockboxDone {
            job: RockboxJob::Update,
            result: Ok("Rockbox is installed.".into()),
        });
        assert!(!core.rockbox_busy);
        assert!(core.ejector.is_none() && !core.eject_deferred);
    }

    fn within(limit: Duration, work: impl FnOnce() + Send + 'static) -> bool {
        let (done, finished) = channel();
        std::thread::spawn(move || {
            work();
            let _ = done.send(());
        });
        finished.recv_timeout(limit).is_ok()
    }

    #[test]
    fn shutdown_waits_for_a_job_even_when_the_coordinator_answers_late() {
        let gate = Gate::default();
        let (tx, rx) = channel();
        let engine = Engine {
            tx,
            gate: gate.clone(),
        };
        // A coordinator that takes far longer than the acknowledgement timeout.
        std::thread::spawn(move || {
            if let Ok(Msg::Shutdown(ack)) = rx.recv() {
                std::thread::sleep(Duration::from_millis(300));
                let _ = ack.send(());
            }
        });
        let guard = gate.acquire().unwrap();
        let finished = Arc::new(AtomicBool::new(false));
        let flag = finished.clone();
        let waiter = std::thread::spawn(move || {
            engine.shutdown_within(Duration::from_millis(20));
            flag.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            !finished.load(Ordering::SeqCst),
            "shutdown left a running job"
        );
        assert!(
            gate.acquire().is_none(),
            "a new job started during shutdown"
        );
        drop(guard);
        waiter.join().unwrap();
        assert!(finished.load(Ordering::SeqCst));
    }

    #[test]
    fn a_worker_that_fails_to_start_does_not_hold_up_shutdown() {
        let mut rig = rig();
        let gate = rig.core.gate.clone();
        let engine = Engine {
            tx: rig.tx.clone(),
            gate,
        };
        let core = &mut rig.core;
        core.spawner = |_, _| Err(std::io::Error::other("no threads"));
        core.internal = Some(volume("internal"));
        core.rockbox_queue.push_back(RockboxJob::Fonts);
        core.start_rockbox();
        assert!(!core.rockbox_busy);
        let reported = rig.events.lock().unwrap().iter().any(|e| {
            matches!(e, UiEvent::RockboxStatus { busy: false, text } if text.contains("Could not start"))
        });
        assert!(reported);
        // Nothing answers the shutdown message here, and the gate is clear.
        assert!(within(Duration::from_secs(10), move || engine
            .shutdown_within(Duration::from_millis(20))));
    }

    #[test]
    fn a_panicking_worker_still_clears_the_gate() {
        let gate = Gate::default();
        let worker = {
            let guard = gate.acquire().unwrap();
            std::thread::spawn(move || {
                let _guard = guard;
                panic!("worker failed");
            })
        };
        assert!(worker.join().is_err());
        assert!(within(Duration::from_secs(10), move || gate.wait_idle()));
    }
}
