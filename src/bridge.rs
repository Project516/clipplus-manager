//! The Qt side: one QObject that is both the job list model and the app state.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        #[qobject]
        type QAbstractListModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvector.h");
        type QVector_i32 = cxx_qt_lib::QVector<i32>;
    }

    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, waiting)]
        #[qproperty(QString, card_kind)]
        #[qproperty(QString, card_text)]
        #[qproperty(bool, card_can_eject)]
        #[qproperty(bool, sponsor_block)]
        #[qproperty(QString, tools_note)]
        #[qproperty(QString, updated_version)]
        #[qproperty(QString, restart_error)]
        #[qproperty(QString, update_note)]
        #[qproperty(bool, checking_updates)]
        #[qproperty(bool, has_finished)]
        #[qproperty(bool, player_connected)]
        #[qproperty(bool, player_mtp)]
        #[qproperty(f64, internal_total)]
        #[qproperty(f64, internal_free)]
        #[qproperty(f64, sd_total)]
        #[qproperty(f64, sd_free)]
        #[qproperty(QString, firmware_kind)]
        #[qproperty(QString, firmware_version)]
        #[qproperty(bool, bootloader_pending)]
        #[qproperty(bool, flash_pending)]
        #[qproperty(QString, flash_kind)]
        #[qproperty(bool, rockbox_busy)]
        #[qproperty(bool, rockbox_failed)]
        #[qproperty(QString, rockbox_status)]
        #[qproperty(QString, rockbox_phase)]
        #[qproperty(QString, rockbox_channel)]
        #[qproperty(QString, rockbox_latest)]
        #[qproperty(bool, rockbox_update_available)]
        #[qproperty(bool, rockbox_can_roll_back)]
        #[qproperty(bool, database_auto_update)]
        #[qproperty(bool, database_auto_update_known)]
        #[qproperty(bool, songs_copied)]
        #[qproperty(QString, theme_list)]
        #[qproperty(QString, search_results)]
        #[qproperty(QString, search_error)]
        #[qproperty(bool, searching)]
        #[qproperty(bool, soulseek_ready)]
        #[qproperty(QString, soulseek_status)]
        #[qproperty(QString, peer_results)]
        #[qproperty(QString, peer_error)]
        #[qproperty(bool, peer_searching)]
        type Backend = super::BackendRust;

        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &Backend, parent: &QModelIndex) -> i32;

        #[cxx_override]
        fn data(self: &Backend, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &Backend) -> QHash_i32_QByteArray;

        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut Backend>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );

        #[inherit]
        fn index(self: &Backend, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;

        /// # Safety
        /// Every call must be followed by `end_insert_rows`.
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut Backend>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );

        /// # Safety
        /// Must follow `begin_insert_rows`.
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut Backend>);

        /// # Safety
        /// Every call must be followed by `end_reset_model`.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut Backend>);

        /// # Safety
        /// Must follow `begin_reset_model`.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut Backend>);

        #[qsignal]
        fn album_queued(self: Pin<&mut Backend>);

        #[qsignal]
        fn raise_requested(self: Pin<&mut Backend>);

        #[qinvokable]
        fn add_links(self: &Backend, text: &QString);

        #[qinvokable]
        fn search_music(self: Pin<&mut Backend>, query: &QString);

        #[qinvokable]
        fn soulseek_connect(self: &Backend);

        #[qinvokable]
        fn check_updates(self: &Backend);

        #[qinvokable]
        fn find_peers(self: Pin<&mut Backend>, index: i32);

        #[qinvokable]
        fn search_albums(self: Pin<&mut Backend>, query: &QString);

        #[qinvokable]
        fn cancel_album_search(self: &Backend);

        #[qinvokable]
        fn download_album(self: &Backend, index: i32);

        #[qinvokable]
        fn download_peer(self: &Backend, index: i32);

        #[qinvokable]
        fn cancel(self: &Backend, id: i64);

        #[qinvokable]
        fn retry(self: &Backend, id: i64);

        #[qinvokable]
        fn clear_finished(self: &Backend);

        #[qinvokable]
        fn eject(self: &Backend);

        #[qinvokable]
        fn set_sponsor_block_enabled(self: Pin<&mut Backend>, on: bool);

        #[qinvokable]
        fn restart(self: Pin<&mut Backend>) -> bool;

        #[qinvokable]
        fn rockbox_check(self: &Backend);

        #[qinvokable]
        fn rockbox_update(self: &Backend);

        #[qinvokable]
        fn rockbox_rollback(self: &Backend);

        #[qinvokable]
        fn rockbox_install_fonts(self: &Backend);

        #[qinvokable]
        fn rockbox_install_voice(self: &Backend);

        #[qinvokable]
        fn rockbox_install_theme(self: &Backend, id: &QString);

        #[qinvokable]
        fn rockbox_uninstall(self: &Backend, full: bool);

        #[qinvokable]
        fn rockbox_install_bootloader(self: &Backend);

        #[qinvokable]
        fn rockbox_restore_stock(self: &Backend);

        #[qinvokable]
        fn rockbox_install_first(self: &Backend);

        #[qinvokable]
        fn eject_player(self: &Backend);

        #[qinvokable]
        fn load_rockbox_themes(self: &Backend);

        #[qinvokable]
        fn choose_rockbox_channel(self: &Backend, channel: &QString);

        #[qinvokable]
        fn change_database_auto_update(self: Pin<&mut Backend>, on: bool);

        #[qinvokable]
        fn dismiss_songs_copied(self: Pin<&mut Backend>);

        #[qinvokable]
        fn shutdown(self: &Backend);
    }

    impl cxx_qt::Threading for Backend {}
    impl cxx_qt::Initialize for Backend {}
}

use crate::{INITIAL_LINKS, INSTANCE};
use clipplus_manager_core::engine::{self, Emit, Engine, JobView, UiEvent};
use clipplus_manager_core::instance::{Command, Instance};
use clipplus_manager_core::paths::Dirs;
use clipplus_manager_core::player::{Firmware, PlayerState};
use clipplus_manager_core::queue::parse_links;
use clipplus_manager_core::rockbox::{Channel, Theme};
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant, QVector,
};
use std::sync::Arc;

const ROLE_BASE: i32 = 0x100;
const ROLES: [&str; 7] = [
    "jobId",
    "title",
    "status",
    "stage",
    "progress",
    "canCancel",
    "canRetry",
];

#[derive(Default)]
pub struct BackendRust {
    waiting: i32,
    card_kind: QString,
    card_text: QString,
    card_can_eject: bool,
    sponsor_block: bool,
    tools_note: QString,
    updated_version: QString,
    restart_error: QString,
    update_note: QString,
    checking_updates: bool,
    has_finished: bool,
    player_connected: bool,
    player_mtp: bool,
    internal_total: f64,
    internal_free: f64,
    sd_total: f64,
    sd_free: f64,
    firmware_kind: QString,
    firmware_version: QString,
    bootloader_pending: bool,
    flash_pending: bool,
    flash_kind: QString,
    rockbox_busy: bool,
    rockbox_failed: bool,
    rockbox_status: QString,
    rockbox_phase: QString,
    rockbox_channel: QString,
    rockbox_latest: QString,
    rockbox_update_available: bool,
    rockbox_can_roll_back: bool,
    database_auto_update: bool,
    database_auto_update_known: bool,
    songs_copied: bool,
    theme_list: QString,
    search_results: QString,
    search_error: QString,
    searching: bool,
    search_generation: u64,
    music_rows: Vec<clipplus_manager_core::musicbrainz::Recording>,
    soulseek_ready: bool,
    soulseek_status: QString,
    peer_results: QString,
    peer_error: QString,
    peer_searching: bool,
    rows: Vec<JobView>,
    engine: Option<Engine>,
    instance: Option<Instance>,
}

impl cxx_qt::Initialize for qobject::Backend {
    fn initialize(mut self: Pin<&mut Self>) {
        let qt = self.qt_thread();
        let emit: Emit = {
            let qt = self.qt_thread();
            Arc::new(move |event| {
                let _ = qt.queue(move |backend| backend.apply(event));
            })
        };
        let appimage = std::env::var_os("APPIMAGE").map(Into::into);
        let engine = engine::start(Dirs::from_env(), appimage, emit);

        let links = INITIAL_LINKS.get().cloned().unwrap_or_default();
        if !links.is_empty() {
            engine.add_links(links);
        }
        if let Some((instance, opened)) = INSTANCE.lock().ok().and_then(|mut slot| slot.take()) {
            self.as_mut().rust_mut().instance = Some(instance);
            let engine = engine.clone();
            std::thread::spawn(move || {
                for command in opened {
                    match command {
                        Command::Open(urls) => {
                            if !urls.is_empty() {
                                engine.add_links(parse_links(&urls.join(" ")));
                            }
                            let _ = qt.queue(|backend| backend.raise_requested());
                        }
                        Command::Eject => engine.eject(),
                    }
                }
            });
        }
        self.as_mut().rust_mut().engine = Some(engine);
    }
}

impl qobject::Backend {
    fn apply(mut self: Pin<&mut Self>, event: UiEvent) {
        match event {
            UiEvent::Jobs(rows) => self.set_rows(rows),
            UiEvent::JobUpdate(row) => self.update_row(row),
            UiEvent::Waiting(n) => self.set_waiting(i32::try_from(n).unwrap_or(i32::MAX)),
            UiEvent::Card(card) => {
                self.as_mut().set_card_kind(QString::from(card.kind));
                self.as_mut().set_card_text(QString::from(&card.text));
                self.set_card_can_eject(card.can_eject);
            }
            UiEvent::SponsorBlock(on) => self.set_sponsor_block(on),
            UiEvent::Tools(text) => self.set_tools_note(QString::from(&text)),
            UiEvent::Updated(version) => {
                self.set_updated_version(QString::from(&format!("v{version}")));
            }
            UiEvent::UpdateCheck { checking, text } => {
                self.as_mut().set_checking_updates(checking);
                self.set_update_note(QString::from(&text));
            }
            UiEvent::SoulseekStatus { ready, text } => {
                self.as_mut().set_soulseek_ready(ready);
                self.set_soulseek_status(QString::from(&text));
            }
            UiEvent::AlbumQueued => self.album_queued(),
            UiEvent::PeerResults(rows) => {
                let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into());
                self.set_peer_results(QString::from(&json));
            }
            UiEvent::PeerError(error) => self.set_peer_error(QString::from(&error)),
            UiEvent::PeerSearching(searching) => self.set_peer_searching(searching),
            UiEvent::Player(state) => self.set_player(&state),
            UiEvent::RockboxStatus { busy, text } => {
                if busy {
                    // A new job clears the last failure, so the next one is a change.
                    self.as_mut().set_rockbox_failed(false);
                }
                self.as_mut().set_rockbox_busy(busy);
                self.set_rockbox_status(QString::from(&text));
            }
            UiEvent::RockboxChannel(channel) => {
                let key = QString::from(channel_key(channel));
                if *self.rockbox_channel() != key {
                    // The old channel's build says nothing about the new one.
                    self.as_mut().set_rockbox_latest(QString::default());
                    self.as_mut().set_rockbox_update_available(false);
                }
                self.set_rockbox_channel(key);
            }
            UiEvent::RockboxLatest {
                version,
                update_available,
                can_roll_back,
                ..
            } => {
                self.as_mut().set_rockbox_latest(QString::from(&version));
                self.as_mut().set_rockbox_update_available(update_available);
                self.set_rockbox_can_roll_back(can_roll_back);
            }
            UiEvent::Themes(themes) => {
                self.set_theme_list(QString::from(&themes_json(&themes)));
            }
            UiEvent::DatabaseAutoUpdate(value) => {
                self.as_mut().set_database_auto_update(value == Some(true));
                self.set_database_auto_update_known(value.is_some());
            }
            UiEvent::FlashPending(on) => self.set_flash_pending(on),
            UiEvent::FlashKind(kind) => self.set_flash_kind(QString::from(kind)),
            UiEvent::RockboxFailed(failed) => self.set_rockbox_failed(failed),
            UiEvent::SongsCopied => self.set_songs_copied(true),
            UiEvent::RockboxPhase(phase) => self.set_rockbox_phase(QString::from(phase)),
        }
    }

    fn set_player(mut self: Pin<&mut Self>, state: &PlayerState) {
        let bytes = |v: Option<u64>| v.map_or(-1.0, |n| n as f64);
        let (internal, card) = (state.internal.as_ref(), state.card.as_ref());
        self.as_mut().set_player_connected(state.connected);
        self.as_mut().set_player_mtp(state.mtp);
        self.as_mut()
            .set_internal_total(bytes(internal.map(|v| v.total)));
        self.as_mut()
            .set_internal_free(bytes(internal.map(|v| v.free)));
        self.as_mut().set_sd_total(bytes(card.map(|v| v.total)));
        self.as_mut().set_sd_free(bytes(card.map(|v| v.free)));
        let (kind, version) = match &state.firmware {
            Firmware::Stock => ("stock", ""),
            Firmware::StockWithRockboxFiles => ("stock-rockbox", ""),
            Firmware::Rockbox { version } => ("rockbox", version.as_str()),
            Firmware::Unknown => ("", ""),
        };
        self.as_mut().set_firmware_kind(QString::from(kind));
        self.as_mut().set_firmware_version(QString::from(version));
        self.set_bootloader_pending(state.bootloader_pending);
    }

    fn set_rows(mut self: Pin<&mut Self>, rows: Vec<JobView>) {
        let has_finished = rows
            .iter()
            .any(|r| matches!(r.stage, "done" | "failed" | "canceled"));
        let old_len = self.rust().rows.len();
        let appended = rows.len() >= old_len
            && self
                .rust()
                .rows
                .iter()
                .zip(&rows)
                .all(|(a, b)| a.id == b.id);
        let none = QModelIndex::default();
        if appended {
            let mut rows = rows;
            let tail = rows.split_off(old_len);
            let changed: Vec<usize> = (0..old_len)
                .filter(|i| self.rust().rows[*i] != rows[*i])
                .collect();
            self.as_mut().rust_mut().rows = rows;
            for i in changed {
                self.as_mut().row_changed(i);
            }
            if !tail.is_empty() {
                let last = (old_len + tail.len() - 1) as i32;
                // SAFETY: every begin is closed by the matching end below.
                unsafe {
                    self.as_mut().begin_insert_rows(&none, old_len as i32, last);
                    self.as_mut().rust_mut().rows.extend(tail);
                    self.as_mut().end_insert_rows();
                }
            }
        } else {
            unsafe {
                self.as_mut().begin_reset_model();
                self.as_mut().rust_mut().rows = rows;
                self.as_mut().end_reset_model();
            }
        }
        self.set_has_finished(has_finished);
    }

    fn update_row(mut self: Pin<&mut Self>, row: JobView) {
        let Some(i) = self.rust().rows.iter().position(|r| r.id == row.id) else {
            return;
        };
        if self.rust().rows[i] != row {
            self.as_mut().rust_mut().rows[i] = row;
            self.row_changed(i);
        }
    }

    fn row_changed(mut self: Pin<&mut Self>, i: usize) {
        let index = self.index(i as i32, 0, &QModelIndex::default());
        self.as_mut()
            .data_changed(&index, &index, &QVector::default());
    }

    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        i32::try_from(self.rust().rows.len()).unwrap_or(i32::MAX)
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = usize::try_from(index.row())
            .ok()
            .and_then(|i| self.rust().rows.get(i))
        else {
            return QVariant::default();
        };
        match role - ROLE_BASE {
            0 => QVariant::from(&(row.id as i64)),
            1 => QVariant::from(&QString::from(&row.title)),
            2 => QVariant::from(&QString::from(&row.status)),
            3 => QVariant::from(&QString::from(row.stage)),
            4 => QVariant::from(&row.progress),
            5 => QVariant::from(&row.can_cancel),
            6 => QVariant::from(&row.can_retry),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut names = QHash::<QHashPair_i32_QByteArray>::default();
        for (i, name) in ROLES.iter().enumerate() {
            names.insert(ROLE_BASE + i as i32, QByteArray::from(*name));
        }
        names
    }

    fn engine(&self) -> Option<&Engine> {
        self.rust().engine.as_ref()
    }

    fn add_links(&self, text: &QString) {
        let links = parse_links(&text.to_string());
        if let (false, Some(engine)) = (links.is_empty(), self.engine()) {
            engine.add_links(links);
        }
    }

    fn search_music(mut self: Pin<&mut Self>, query: &QString) {
        let query = query.to_string();
        let generation = self.rust().search_generation.wrapping_add(1);
        self.as_mut().rust_mut().search_generation = generation;
        self.as_mut().set_search_error(QString::default());
        self.as_mut().set_search_results(QString::from("[]"));
        self.as_mut().rust_mut().music_rows.clear();
        if query.trim().is_empty() {
            self.set_searching(false);
            return;
        }
        self.as_mut().set_searching(true);
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let result = clipplus_manager_core::musicbrainz::search(&query);
            let _ = qt.queue(move |mut backend| {
                if backend.rust().search_generation != generation {
                    return;
                }
                backend.as_mut().set_searching(false);
                match result {
                    Ok(rows) => {
                        let values: Vec<_> = rows
                            .iter()
                            .map(|r| {
                                serde_json::json!({
                                    "id": r.id, "title": r.title, "artist": r.artist,
                                    "release": r.release, "length": r.length_ms
                                })
                            })
                            .collect();
                        backend.as_mut().rust_mut().music_rows = rows;
                        backend.set_search_results(QString::from(
                            &serde_json::Value::Array(values).to_string(),
                        ));
                    }
                    Err(error) => backend.set_search_error(QString::from(&error)),
                }
            });
        });
    }

    fn soulseek_connect(&self) {
        if let Some(engine) = self.engine() {
            engine.soulseek_connect();
        }
    }

    fn check_updates(&self) {
        if let Some(engine) = self.engine() {
            engine.check_updates();
        }
    }

    fn find_peers(mut self: Pin<&mut Self>, index: i32) {
        let recording = usize::try_from(index)
            .ok()
            .and_then(|i| self.rust().music_rows.get(i))
            .cloned();
        if let (Some(recording), Some(engine)) = (recording, self.engine().cloned()) {
            self.as_mut().set_peer_error(QString::default());
            self.as_mut().set_peer_results(QString::from("[]"));
            self.as_mut().set_peer_searching(true);
            engine.search_peers(recording);
        }
    }

    fn search_albums(mut self: Pin<&mut Self>, query: &QString) {
        if let Some(engine) = self.engine().cloned() {
            self.as_mut().set_peer_error(QString::default());
            self.as_mut().set_peer_results(QString::from("[]"));
            self.as_mut().set_peer_searching(true);
            engine.search_albums(query.to_string());
        }
    }

    fn cancel_album_search(&self) {
        if let Some(engine) = self.engine() {
            engine.cancel_album_search();
        }
    }

    fn download_album(&self, index: i32) {
        if let (Some(engine), Ok(index)) = (self.engine(), usize::try_from(index)) {
            engine.download_album(index);
        }
    }

    fn download_peer(&self, index: i32) {
        if let (Some(engine), Ok(index)) = (self.engine(), usize::try_from(index)) {
            engine.download_peer(index);
        }
    }

    fn cancel(&self, id: i64) {
        if let (Some(engine), Ok(id)) = (self.engine(), u64::try_from(id)) {
            engine.cancel(id);
        }
    }

    fn retry(&self, id: i64) {
        if let (Some(engine), Ok(id)) = (self.engine(), u64::try_from(id)) {
            engine.retry(id);
        }
    }

    fn clear_finished(&self) {
        if let Some(engine) = self.engine() {
            engine.clear_finished();
        }
    }

    fn eject(&self) {
        if let Some(engine) = self.engine() {
            engine.eject();
        }
    }

    fn set_sponsor_block_enabled(mut self: Pin<&mut Self>, on: bool) {
        // Show the new value now; the engine confirms it with the same value.
        self.as_mut().set_sponsor_block(on);
        if let Some(engine) = self.engine() {
            engine.set_sponsorblock(on);
        }
    }

    /// Starts a fresh copy of the app. The caller quits this one.
    fn restart(mut self: Pin<&mut Self>) -> bool {
        let result = std::env::var_os("APPIMAGE")
            .ok_or_else(|| std::io::Error::other("AppImage path is unavailable"))
            .and_then(|appimage| {
                std::process::Command::new(appimage)
                    .arg("--restart")
                    .spawn()
            });
        match result {
            Ok(_) => {
                if let Some(instance) = &self.rust().instance {
                    instance.release();
                }
                true
            }
            Err(error) => {
                let message = format!("Could not restart: {error}");
                eprintln!("clipplus-manager: {message}");
                self.as_mut().set_restart_error(QString::from(&message));
                false
            }
        }
    }

    fn rockbox_check(&self) {
        if let Some(engine) = self.engine() {
            engine.rockbox_check();
        }
    }

    fn rockbox_update(&self) {
        if let Some(engine) = self.engine() {
            engine.rockbox_update();
        }
    }

    fn rockbox_rollback(&self) {
        if let Some(engine) = self.engine() {
            engine.rockbox_rollback();
        }
    }

    fn rockbox_install_fonts(&self) {
        if let Some(engine) = self.engine() {
            engine.rockbox_install_fonts();
        }
    }

    fn rockbox_install_voice(&self) {
        if let Some(engine) = self.engine() {
            engine.rockbox_install_voice();
        }
    }

    fn rockbox_install_theme(&self, id: &QString) {
        if let Some(engine) = self.engine() {
            engine.rockbox_install_theme(&id.to_string());
        }
    }

    fn rockbox_uninstall(&self, full: bool) {
        if let Some(engine) = self.engine() {
            engine.rockbox_uninstall(full);
        }
    }

    fn rockbox_install_bootloader(&self) {
        if let Some(engine) = self.engine() {
            engine.rockbox_install_bootloader();
        }
    }

    fn rockbox_restore_stock(&self) {
        if let Some(engine) = self.engine() {
            engine.rockbox_restore_stock();
        }
    }

    fn rockbox_install_first(&self) {
        if let Some(engine) = self.engine() {
            engine.rockbox_install_first();
        }
    }

    fn eject_player(&self) {
        if let Some(engine) = self.engine() {
            engine.eject_player();
        }
    }

    fn load_rockbox_themes(&self) {
        if let Some(engine) = self.engine() {
            engine.rockbox_themes();
        }
    }

    fn choose_rockbox_channel(&self, channel: &QString) {
        let channel = match channel.to_string().as_str() {
            "dev" => Channel::Dev,
            "daily" => Channel::Daily,
            "release" => Channel::Release,
            _ => return,
        };
        if let Some(engine) = self.engine() {
            engine.set_rockbox_channel(channel);
        }
    }

    fn change_database_auto_update(mut self: Pin<&mut Self>, on: bool) {
        // The engine re-reads the file after the write and corrects this value.
        self.as_mut().set_database_auto_update(on);
        if let Some(engine) = self.engine() {
            engine.set_database_autoupdate(on);
        }
    }

    fn dismiss_songs_copied(self: Pin<&mut Self>) {
        self.set_songs_copied(false);
    }

    fn shutdown(&self) {
        if let Some(engine) = self.engine() {
            engine.shutdown();
        }
    }
}

fn channel_key(channel: Channel) -> &'static str {
    match channel {
        Channel::Dev => "dev",
        Channel::Daily => "daily",
        Channel::Release => "release",
    }
}

fn themes_json(themes: &[Theme]) -> String {
    let values: Vec<_> = themes
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id, "name": t.name, "author": t.author,
                "size": engine::format_free(t.size), "preview": t.preview,
                "about": t.about, "installed": false
            })
        })
        .collect();
    serde_json::Value::Array(values).to_string()
}
