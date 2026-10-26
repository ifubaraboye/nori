//! The torrent queue behind the download and seeding views.
//!
//! librqbit owns the network; this module owns the order of the queue, the
//! concurrency cap, the seed clock, and the JSON file that survives a restart.
//! Everything runs on one background tokio runtime, so the UI thread never
//! waits on the network or on disk.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use librqbit::api::TorrentIdOrHash;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerOptions,
    ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig, TorrentStatsState,
};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Deserializer, Serialize};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::config::{self, Config};
use crate::search::magnet::{
    build_magnet, is_info_hash, normalize_hash, parse_magnet, query_escape, trackers_of,
};
use crate::torrent::{perms, playlist};

const TICK: Duration = Duration::from_millis(500);
const PERSIST_EVERY: Duration = Duration::from_secs(2);
/// How long a fresh resume may take to find peers before it is restarted.
const RESUME_GRACE: Duration = Duration::from_secs(25);

/// Where a download is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
}

impl Default for Status {
    fn default() -> Self {
        Self::Queued
    }
}

impl Status {
    pub fn is_active(self) -> bool {
        matches!(self, Status::Queued | Status::Downloading)
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Queued => "Queued",
            Status::Downloading => "Downloading",
            Status::Paused => "Paused",
            Status::Completed => "Completed",
            Status::Failed => "Failed",
        }
    }
}

impl<'de> Deserialize<'de> for Status {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.as_str() {
            "downloading" => Status::Downloading,
            "paused" => Status::Paused,
            "completed" => Status::Completed,
            "failed" => Status::Failed,
            _ => Status::Queued,
        })
    }
}

/// How a group of results chosen together should start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchMode {
    /// Start every item now, ignoring the concurrency cap for this batch.
    AllAtOnce,
    /// Start the items one at a time, in the order they were chosen.
    Queue,
}

/// One file inside a download, as the views see it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemFile {
    /// Path relative to the torrent's own folder.
    pub path: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub downloaded: i64,
}

/// One download, as the views see it.
///
/// The JSON field names are part of the on-disk queue format; keep them stable
/// so a queue written by an older build still loads.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub magnet: String,
    pub source: String,
    pub dir: String,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub size_bytes: i64,
    #[serde(default)]
    pub downloaded: i64,
    #[serde(default)]
    pub uploaded: i64,
    #[serde(default)]
    pub progress: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub seed_paused: bool,
    /// Sequential batch this item belongs to, when it was queued with others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_group: Option<String>,
    /// Set for a batch that starts together, ignoring the concurrency cap.
    #[serde(default, skip_serializing_if = "is_false")]
    pub start_all: bool,
    #[serde(default, deserialize_with = "timestamp")]
    pub added_at: i64,
    #[serde(default, deserialize_with = "timestamp")]
    pub completed_at: i64,
    /// Every real file in the torrent, kept so a finished download can still
    /// show its contents after a restart without contacting the swarm.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ItemFile>,

    /// Runtime-only counters.
    #[serde(skip)]
    pub speed: i64,
    #[serde(skip)]
    pub up_speed: i64,
    #[serde(skip)]
    pub peers: i64,
}

impl Item {
    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    /// 0..=1, already clamped.
    pub fn fraction(&self) -> f64 {
        if self.progress.is_finite() {
            self.progress.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Seconds left at the current rate, or `None` when unknown.
    pub fn eta_secs(&self) -> Option<i64> {
        if self.status != Status::Downloading || self.speed <= 0 || self.size_bytes <= 0 {
            return None;
        }
        let left = self.size_bytes.saturating_sub(self.downloaded);
        if left <= 0 {
            return None;
        }
        Some(left / self.speed)
    }

    pub fn seed_age_secs(&self) -> i64 {
        if self.completed_at <= 0 {
            return 0;
        }
        (unix_now() - self.completed_at).max(0)
    }
}

fn restored_status(status: Status) -> Status {
    match status {
        Status::Paused => Status::Paused,
        Status::Failed => Status::Failed,
        Status::Completed => Status::Completed,
        Status::Queued | Status::Downloading => Status::Queued,
    }
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Accept both epoch integers and RFC3339 timestamps.
fn timestamp<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Int(i64),
        Float(f64),
        Text(String),
    }

    match Raw::deserialize(deserializer)? {
        Raw::Int(n) => Ok(n),
        Raw::Float(n) => Ok(n as i64),
        Raw::Text(text) => {
            let text = text.trim().to_owned();
            if text.is_empty() {
                return Ok(0);
            }
            if let Ok(n) = text.parse::<i64>() {
                return Ok(n);
            }
            Ok(chrono::DateTime::parse_from_rfc3339(&text)
                .map(|t| t.timestamp())
                .unwrap_or(0))
        }
    }
}

struct Entry {
    item: Item,
    handle: Option<Arc<ManagedTorrent>>,
    /// Set once file permissions and playlists have been arranged.
    promoted: bool,
    /// Set when a start has been issued but the handle is not stored yet.
    starting: bool,
    /// Set after an unpause so a torrent that never finds peers again can be
    /// restarted from scratch.
    resume_probe: Option<ResumeProbe>,
}

struct ResumeProbe {
    started: Instant,
    baseline: i64,
}

fn resume_probe_should_restart(
    probe: &ResumeProbe,
    peers: i64,
    downloaded: i64,
    speed: i64,
) -> bool {
    peers <= 0
        && downloaded <= probe.baseline
        && speed <= 0
        && probe.started.elapsed() >= RESUME_GRACE
}

impl Entry {
    fn new(item: Item) -> Self {
        Self {
            item,
            handle: None,
            promoted: false,
            starting: false,
            resume_probe: None,
        }
    }
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    order: Vec<String>,
}

struct Shared {
    tx: UnboundedSender<Command>,
    items: RwLock<Vec<Item>>,
    changes: async_channel::Sender<()>,
    change_rx: Mutex<Option<async_channel::Receiver<()>>>,
    startup_error: Mutex<Option<String>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// A validated magnet on its way into the queue.
struct NewTorrent {
    id: String,
    name: String,
    magnet: String,
    source: String,
    dir: String,
}

enum Command {
    Add {
        id: String,
        name: String,
        magnet: String,
        source: String,
        dir: String,
    },
    AddBatch {
        items: Vec<NewTorrent>,
        mode: BatchMode,
    },
    Pause(String),
    Resume(String),
    Remove(String),
    SetMaxDownloads(i32),
    SetSeedTime(i32),
    SetSeedingEnabled(bool),
    SetTrackers(Vec<String>),
    SetSeedPaused {
        id: String,
        paused: bool,
    },
    SetAllSeedPaused(bool),
    PreviewFiles {
        magnet: String,
        response: async_channel::Sender<Result<Vec<ItemFile>, String>>,
    },
    Restore(Vec<Item>),
    Shutdown,
}

/// Handle to the queue. Cheap to clone and safe to call from the UI thread.
#[derive(Clone)]
pub struct TorrentManager {
    shared: Arc<Shared>,
}

impl TorrentManager {
    /// Start the torrent session and restore the saved queue in the background.
    pub fn new(config: Config) -> Self {
        let (tx, rx) = unbounded_channel();
        let (change_tx, change_rx) = async_channel::bounded(8);
        let shared = Arc::new(Shared {
            tx,
            items: RwLock::new(Vec::new()),
            changes: change_tx,
            change_rx: Mutex::new(Some(change_rx)),
            startup_error: Mutex::new(None),
            thread: Mutex::new(None),
        });

        let thread_shared = shared.clone();
        let handle = std::thread::Builder::new()
            .name("nori-torrents".into())
            .spawn(move || run(config, thread_shared, rx))
            .expect("spawn torrent thread");
        *shared.thread.lock() = Some(handle);

        Self { shared }
    }

    /// A queue snapshot, newest first.
    pub fn snapshot(&self) -> Vec<Item> {
        self.shared.items.read().clone()
    }

    pub fn item(&self, id: &str) -> Option<Item> {
        self.shared
            .items
            .read()
            .iter()
            .find(|item| item.id == id)
            .cloned()
    }

    /// Reflect a queue command in the UI snapshot before the network thread
    /// has finished its filesystem or session work. The engine remains the
    /// source of truth and publishes its full state immediately afterwards.
    fn update_snapshot(&self, update: impl FnOnce(&mut Vec<Item>)) {
        update(&mut self.shared.items.write());
        let _ = self.shared.changes.try_send(());
    }

    fn persist_snapshot(&self) {
        let items = self.shared.items.read().clone();
        persist_items(&items);
    }

    /// The channel that signals "the queue changed".
    pub fn changes(&self) -> async_channel::Receiver<()> {
        self.shared
            .change_rx
            .lock()
            .take()
            .expect("changes() may only be called once")
    }

    pub fn startup_error(&self) -> Option<String> {
        self.shared.startup_error.lock().clone()
    }

    /// Queue a magnet link or a bare info hash.
    pub fn add(&self, magnet: &str, source: &str) -> Result<(), String> {
        let (id, name, magnet) = parse_download_input(magnet)?;
        if id.is_empty() {
            return Err("The magnet link carries no info hash".into());
        }
        let source = source_label(source);
        self.shared
            .tx
            .send(Command::Add {
                id: id.clone(),
                name: name.clone(),
                magnet: magnet.clone(),
                source: source.clone(),
                dir: String::new(),
            })
            .map_err(|_| "Torrent manager is unavailable".to_owned())?;
        self.update_snapshot(|items| {
            if items.iter().all(|item| item.id != id) {
                items.insert(
                    0,
                    Item {
                        id,
                        name,
                        magnet,
                        source,
                        dir: String::new(),
                        status: Status::Downloading,
                        added_at: unix_now(),
                        ..Default::default()
                    },
                );
            }
        });
        Ok(())
    }

    /// Queue several magnets together. Items entering as a queue batch run one
    /// at a time in the order given; an all-at-once batch ignores the cap.
    pub fn add_batch(&self, rows: &[(String, String)], mode: BatchMode) -> Result<usize, String> {
        if rows.is_empty() {
            return Err("Choose at least one result".into());
        }
        let mut items = Vec::with_capacity(rows.len());
        let mut seen = HashSet::new();
        for (magnet, source) in rows {
            let (id, name, magnet) = parse_download_input(magnet)?;
            if id.is_empty() {
                return Err("A magnet link carries no info hash".into());
            }
            if !seen.insert(id.clone()) {
                continue;
            }
            items.push(NewTorrent {
                id,
                name,
                magnet,
                source: source_label(source),
                dir: String::new(),
            });
        }
        if items.is_empty() {
            return Err("Choose at least one result".into());
        }
        let count = items.len();
        let added_at = unix_now();
        let group = (mode == BatchMode::Queue)
            .then(|| items.first().map(|item| item.id.clone()))
            .flatten();
        let optimistic: Vec<Item> = items
            .iter()
            .map(|item| Item {
                id: item.id.clone(),
                name: item.name.clone(),
                magnet: item.magnet.clone(),
                source: item.source.clone(),
                dir: item.dir.clone(),
                status: if mode == BatchMode::AllAtOnce {
                    Status::Downloading
                } else {
                    Status::Queued
                },
                added_at,
                queue_group: group.clone(),
                start_all: mode == BatchMode::AllAtOnce,
                ..Default::default()
            })
            .collect();
        self.shared
            .tx
            .send(Command::AddBatch { items, mode })
            .map_err(|_| "Torrent manager is unavailable".to_owned())?;
        self.update_snapshot(|items| {
            let mut new_items = Vec::new();
            for item in optimistic {
                if let Some(existing) = items.iter_mut().find(|old| old.id == item.id) {
                    if matches!(existing.status, Status::Paused | Status::Failed) {
                        existing.status = Status::Queued;
                        existing.error.clear();
                        existing.speed = 0;
                        existing.up_speed = 0;
                    }
                } else {
                    new_items.push(item);
                }
            }
            items.splice(0..0, new_items);
        });
        Ok(count)
    }

    pub fn pause(&self, id: &str) -> Result<(), String> {
        self.shared
            .tx
            .send(Command::Pause(id.to_owned()))
            .map_err(|_| "Torrent manager is unavailable".to_owned())?;
        self.update_snapshot(|items| {
            if let Some(item) = items.iter_mut().find(|item| item.id == id) {
                item.status = Status::Paused;
                item.speed = 0;
                item.up_speed = 0;
                item.error.clear();
            }
        });
        Ok(())
    }

    pub fn resume(&self, id: &str) -> Result<(), String> {
        self.shared
            .tx
            .send(Command::Resume(id.to_owned()))
            .map_err(|_| "Torrent manager is unavailable".to_owned())?;
        self.update_snapshot(|items| {
            if let Some(item) = items.iter_mut().find(|item| item.id == id) {
                item.status = Status::Queued;
                item.error.clear();
                item.speed = 0;
                item.up_speed = 0;
            }
        });
        Ok(())
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        self.shared
            .tx
            .send(Command::Remove(id.to_owned()))
            .map_err(|_| "Torrent manager is unavailable".to_owned())?;
        self.update_snapshot(|items| items.retain(|item| item.id != id));
        self.persist_snapshot();
        Ok(())
    }

    pub fn set_max_downloads(&self, n: i32) {
        let _ = self.shared.tx.send(Command::SetMaxDownloads(n.max(0)));
    }

    pub fn set_seed_time(&self, minutes: i32) {
        let _ = self.shared.tx.send(Command::SetSeedTime(minutes.max(0)));
    }

    pub fn set_seeding_enabled(&self, enabled: bool) {
        let _ = self.shared.tx.send(Command::SetSeedingEnabled(enabled));
    }

    /// Resolve a magnet's metadata without adding it to the download queue.
    pub fn preview_files(
        &self,
        magnet: &str,
    ) -> async_channel::Receiver<Result<Vec<ItemFile>, String>> {
        let (tx, rx) = async_channel::bounded(1);
        let _ = self.shared.tx.send(Command::PreviewFiles {
            magnet: magnet.to_owned(),
            response: tx,
        });
        rx
    }

    pub fn set_trackers(&self, trackers: Vec<String>) {
        let _ = self.shared.tx.send(Command::SetTrackers(trackers));
    }

    pub fn set_seed_paused(&self, id: &str, paused: bool) {
        let _ = self.shared.tx.send(Command::SetSeedPaused {
            id: id.to_owned(),
            paused,
        });
    }

    pub fn set_all_seed_paused(&self, paused: bool) {
        let _ = self.shared.tx.send(Command::SetAllSeedPaused(paused));
    }

    /// Flush the queue and stop the session.
    pub fn close(&self) {
        let _ = self.shared.tx.send(Command::Shutdown);
        if let Some(handle) = self.shared.thread.lock().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for TorrentManager {
    fn drop(&mut self) {
        // Clones are handed out freely; only the last owner flushes the queue.
        if Arc::strong_count(&self.shared) > 1 {
            return;
        }
        self.close();
    }
}

struct Engine {
    shared: Arc<Shared>,
    session: Arc<Session>,
    preview_session: Arc<Session>,
    state: Mutex<State>,
    config: Mutex<Config>,
    dirty: AtomicBool,
    last_persist: Mutex<Instant>,
    /// Torrents waiting for metadata. Kept off the command loop so a slow
    /// magnet never delays a new item reaching the queue snapshot.
    start_tx: UnboundedSender<String>,
}

fn run(config: Config, shared: Arc<Shared>, rx: UnboundedReceiver<Command>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("nori-net")
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            *shared.startup_error.lock() =
                Some(format!("Could not start the network runtime: {err}"));
            return;
        }
    };

    runtime.block_on(async move {
        let session = match build_session(&config).await {
            Ok(session) => session,
            Err(err) => {
                *shared.startup_error.lock() = Some(format!("Could not start torrents: {err:#}"));
                return;
            }
        };
        let preview_session = match build_preview_session(&config).await {
            Ok(session) => session,
            Err(err) => {
                *shared.startup_error.lock() =
                    Some(format!("Could not start metadata previews: {err:#}"));
                return;
            }
        };
        let (start_tx, mut start_rx) = unbounded_channel::<String>();
        let engine = Engine {
            shared,
            session,
            preview_session,
            state: Mutex::new(State::default()),
            config: Mutex::new(config),
            dirty: AtomicBool::new(false),
            last_persist: Mutex::new(Instant::now()),
            start_tx,
        };
        engine.restore().await;
        let starts = async {
            while let Some(id) = start_rx.recv().await {
                engine.start(&id).await;
            }
        };
        tokio::select! {
            _ = engine.loop_forever(rx) => {}
            _ = starts => {}
        }
        engine.persist();
    });
}

async fn build_session(config: &Config) -> anyhow::Result<Arc<Session>> {
    std::fs::create_dir_all(config.download_path())?;
    std::fs::create_dir_all(config::data_dir())?;

    let mut opts = SessionOptions::default();
    // Many Linux environments have working IPv4 internet access but no
    // usable IPv6 route. Binding the peer and DHT sockets to IPv6 in that
    // situation leaves magnets stuck at zero peers while HTTP search still
    // works normally.
    opts.ipv4_only = true;
    opts.dht = Some(DhtSessionConfig {
        bootstrap_addrs: None,
        port: None,
        persistence: None,
    });
    opts.listen = Some(ListenerOptions {
        listen_addr: (std::net::Ipv4Addr::UNSPECIFIED, 0).into(),
        ..Default::default()
    });
    opts.disable_local_service_discovery = true;
    opts.client_name_and_version = Some("nori/0.1".into());
    opts.persistence = Some(SessionPersistenceConfig::Json {
        folder: Some(config::data_dir().join("session")),
    });

    let session = Session::new_with_opts(config.download_path(), opts)
        .await
        .context("creating the torrent session")?;
    tracing::info!(
        listen_addr = ?session.listen_addr(),
        ipv4_only = true,
        "torrent session started"
    );
    Ok(session)
}

/// Metadata previews must not wait behind a live download resolving its
/// magnet. This session has its own DHT/UDP resources and never owns files.
async fn build_preview_session(config: &Config) -> anyhow::Result<Arc<Session>> {
    let mut opts = SessionOptions::default();
    opts.ipv4_only = true;
    opts.dht = Some(DhtSessionConfig {
        bootstrap_addrs: None,
        port: None,
        persistence: None,
    });
    opts.listen = None;
    opts.disable_local_service_discovery = true;
    opts.client_name_and_version = Some("nori/0.1-preview".into());

    let session = Session::new_with_opts(config.download_path(), opts)
        .await
        .context("creating the metadata preview session")?;
    tracing::info!("metadata preview session started");
    Ok(session)
}

/// Put a new batch ahead of everything else without reversing its order.
fn insert_batch_items(state: &mut State, items: Vec<Item>) {
    for item in items.into_iter().rev() {
        let id = item.id.clone();
        state.order.insert(0, id.clone());
        state.entries.insert(id, Entry::new(item));
    }
}

/// Pick queued downloads that can start: start-all items ignore the cap, and
/// a queue batch never has more than one download running at a time.
fn plan_downloads(state: &mut State, max: i32) -> Vec<String> {
    let mut start = Vec::new();
    let mut active = 0i32;
    let mut groups: HashMap<String, i32> = HashMap::new();
    for entry in state.entries.values() {
        if entry.item.status == Status::Downloading {
            active += 1;
            if let Some(group) = &entry.item.queue_group {
                *groups.entry(group.clone()).or_insert(0) += 1;
            }
        }
    }

    for id in state.order.clone() {
        let Some(entry) = state.entries.get_mut(&id) else {
            continue;
        };
        if entry.item.status != Status::Queued || entry.starting {
            continue;
        }
        if !entry.item.start_all {
            if let Some(group) = entry.item.queue_group.clone() {
                if groups.get(&group).copied().unwrap_or(0) > 0 {
                    continue;
                }
                if max > 0 && active >= max {
                    continue;
                }
                *groups.entry(group).or_insert(0) += 1;
            } else if max > 0 && active >= max {
                continue;
            }
        }
        entry.starting = true;
        entry.item.status = Status::Downloading;
        entry.item.speed = 0;
        start.push(id);
        active += 1;
        if let Some(group) = &entry.item.queue_group {
            *groups.entry(group.clone()).or_insert(0) += 1;
        }
    }
    start
}

/// Reattach handles to finished torrents that want to keep seeding.
fn plan_reseeds(state: &mut State, seeding_enabled: bool) -> Vec<String> {
    let mut start = Vec::new();
    if !seeding_enabled {
        return start;
    }
    for id in state.order.clone() {
        let Some(entry) = state.entries.get_mut(&id) else {
            continue;
        };
        if entry.item.status != Status::Completed
            || entry.item.seed_paused
            || entry.handle.is_some()
            || entry.starting
        {
            continue;
        }
        entry.starting = true;
        start.push(id);
    }
    start
}

impl Engine {
    async fn loop_forever(&self, mut rx: UnboundedReceiver<Command>) {
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    self.tick().await;
                    self.flush(false);
                }
                cmd = rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    let shutdown = self.handle(cmd).await;
                    self.flush(true);
                    if shutdown {
                        break;
                    }
                }
            }
        }
        self.persist();
    }

    async fn handle(&self, command: Command) -> bool {
        match command {
            Command::Add {
                id,
                name,
                magnet,
                source,
                dir,
            } => {
                let existing = self.state.lock().entries.get(&id).map(|e| e.item.clone());
                if let Some(item) = existing {
                    if item.status == Status::Paused || item.status == Status::Failed {
                        self.resume(&id).await;
                    }
                    return false;
                }
                let dir = dir;
                let magnet = {
                    let trackers = self.config.lock().trackers.clone();
                    with_trackers(&magnet, &trackers)
                };
                let item = Item {
                    id: id.clone(),
                    name,
                    magnet,
                    source,
                    dir,
                    status: Status::Queued,
                    added_at: unix_now(),
                    ..Default::default()
                };
                {
                    let mut state = self.state.lock();
                    state.entries.insert(id.clone(), Entry::new(item));
                    state.order.insert(0, id);
                }
                self.dirty.store(true, Ordering::Relaxed);
                self.schedule().await;
                false
            }
            Command::AddBatch { items, mode } => {
                self.add_batch(items, mode).await;
                false
            }
            Command::Pause(id) => {
                self.pause(&id).await;
                false
            }
            Command::Resume(id) => {
                self.resume(&id).await;
                false
            }
            Command::Remove(id) => {
                self.remove(&id).await;
                false
            }
            Command::SetMaxDownloads(n) => {
                self.config.lock().max_downloads = n.max(0);
                self.schedule().await;
                false
            }
            Command::SetSeedTime(minutes) => {
                self.config.lock().seed_time_minutes = minutes.max(0);
                self.tick().await;
                false
            }
            Command::SetSeedingEnabled(enabled) => {
                self.set_seeding_enabled(enabled).await;
                false
            }
            Command::SetTrackers(trackers) => {
                self.config.lock().trackers = trackers;
                false
            }
            Command::SetSeedPaused { id, paused } => {
                self.set_seed_paused(&id, paused).await;
                false
            }
            Command::SetAllSeedPaused(paused) => {
                self.set_all_seed_paused(paused).await;
                false
            }
            Command::PreviewFiles { magnet, response } => {
                let session = self.preview_session.clone();
                let trackers = self.config.lock().trackers.clone();
                tokio::spawn(async move {
                    let magnet = with_trackers(&magnet, &trackers);
                    let result = match tokio::time::timeout(
                        Duration::from_secs(20),
                        session.add_torrent(
                            AddTorrent::from_url(magnet),
                            Some(AddTorrentOptions {
                                list_only: true,
                                ..Default::default()
                            }),
                        ),
                    )
                    .await
                    {
                        Ok(Ok(response)) => match response {
                            AddTorrentResponse::ListOnly(list) => Ok(list
                                .info
                                .iter_file_details()
                                .filter_map(|file| {
                                    if file.attrs().padding {
                                        return None;
                                    }
                                    Some(ItemFile {
                                        path: file
                                            .filename
                                            .to_pathbuf()
                                            .to_string_lossy()
                                            .replace('\\', "/"),
                                        size: file.len as i64,
                                        downloaded: 0,
                                    })
                                })
                                .collect()),
                            AddTorrentResponse::AlreadyManaged(_, handle)
                            | AddTorrentResponse::Added(_, handle) => {
                                let stats = handle.stats();
                                Ok(torrent_files(&handle, &stats.file_progress))
                            }
                        },
                        Ok(Err(error)) => Err(format!("{error:#}")),
                        Err(_) => Err("Timed out waiting for torrent metadata".to_owned()),
                    };
                    let _ = response.send(result).await;
                });
                false
            }
            Command::Restore(items) => {
                self.restore_items(items, false).await;
                self.schedule().await;
                false
            }
            Command::Shutdown => true,
        }
    }

    /// Read the queue file that the last run wrote.
    async fn restore(&self) {
        let path = config::queue_path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            self.publish();
            return;
        };
        let Ok(items) = serde_json::from_str::<Vec<Item>>(&raw) else {
            tracing::warn!("ignoring malformed queue at {}", path.display());
            self.publish();
            return;
        };
        self.restore_items(items, true).await;
        self.schedule().await;
    }

    async fn restore_items(&self, items: Vec<Item>, from_disk: bool) {
        let trackers = self.config.lock().trackers.clone();
        let mut state = self.state.lock();
        for mut item in items {
            if item.id.is_empty() || item.magnet.is_empty() {
                continue;
            }
            if state.entries.contains_key(&item.id) {
                continue;
            }
            if from_disk {
                item.status = restored_status(item.status);
            }
            item.magnet = with_trackers(&item.magnet, &trackers);
            item.speed = 0;
            item.up_speed = 0;
            item.peers = 0;
            state.order.push(item.id.clone());
            state.entries.insert(item.id.clone(), Entry::new(item));
        }
        drop(state);
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Add a selected group of torrents in one go. New rows go to the front of
    /// the queue in the order the user saw them, so a queue batch progresses
    /// top to bottom.
    async fn add_batch(&self, items: Vec<NewTorrent>, mode: BatchMode) {
        if items.is_empty() {
            return;
        }
        let trackers = self.config.lock().trackers.clone();
        // One timestamp for the whole batch keeps the chosen order stable when
        // the queue is sorted newest-first.
        let added_at = unix_now();
        let group = {
            let state = self.state.lock();
            items
                .iter()
                .find(|item| !state.entries.contains_key(&item.id))
                .map(|item| item.id.clone())
        };

        let mut added = Vec::new();
        {
            let mut state = self.state.lock();
            for torrent in items {
                if let Some(entry) = state.entries.get_mut(&torrent.id) {
                    if matches!(entry.item.status, Status::Paused | Status::Failed) {
                        entry.item.status = Status::Queued;
                        entry.item.error.clear();
                        entry.item.speed = 0;
                        entry.item.up_speed = 0;
                        entry.starting = false;
                        match mode {
                            BatchMode::AllAtOnce => {
                                entry.item.queue_group = None;
                                entry.item.start_all = true;
                            }
                            BatchMode::Queue => {
                                if let Some(group) = &group {
                                    entry.item.queue_group = Some(group.clone());
                                    entry.item.start_all = false;
                                }
                            }
                        }
                    }
                    continue;
                }
                let dir = torrent.dir;
                let magnet = with_trackers(&torrent.magnet, &trackers);
                added.push(Item {
                    id: torrent.id.clone(),
                    name: torrent.name,
                    magnet,
                    source: torrent.source,
                    dir,
                    status: Status::Queued,
                    added_at,
                    queue_group: if mode == BatchMode::Queue {
                        group.clone()
                    } else {
                        None
                    },
                    start_all: mode == BatchMode::AllAtOnce,
                    ..Default::default()
                });
            }
            insert_batch_items(&mut state, added);
        }
        self.dirty.store(true, Ordering::Relaxed);
        self.schedule().await;
    }

    async fn schedule(&self) {
        let start = {
            let mut state = self.state.lock();
            let (max, seeding_enabled) = {
                let config = self.config.lock();
                (config.max_downloads, config.seeding_enabled)
            };
            let mut start = plan_downloads(&mut state, max);
            // Finished torrents only need their handle back so they can seed;
            // they never pass through the download queue again.
            start.extend(plan_reseeds(&mut state, seeding_enabled));
            start
        };
        self.dirty.store(true, Ordering::Relaxed);
        // The views see the new state before any metadata is resolved.
        self.publish();
        for id in start {
            let _ = self.start_tx.send(id);
        }
    }

    async fn start(&self, id: &str) {
        // Read the settings once, before touching the state. Taking
        // `self.config.lock()` twice in one expression would keep the first
        // guard alive until the expression ends and deadlock on itself, which
        // wedges the whole engine the first time a download starts.
        let (trackers, seeding_enabled) = {
            let config = self.config.lock();
            (config.trackers.clone(), config.seeding_enabled)
        };
        let (magnet, dir, status, seed_paused, handle) = {
            let mut state = self.state.lock();
            let Some(entry) = state.entries.get_mut(id) else {
                return;
            };
            if !matches!(
                entry.item.status,
                Status::Queued | Status::Downloading | Status::Completed
            ) {
                // Paused or failed while waiting in the start queue.
                entry.starting = false;
                return;
            }
            (
                entry.item.magnet.clone(),
                entry.item.dir.clone(),
                entry.item.status,
                entry.item.seed_paused,
                entry.handle.clone(),
            )
        };

        // Resuming a paused or failed item that still has a handle only needs
        // the session unpaused; adding it a second time would duplicate work.
        if let Some(handle) = handle {
            let should_pause = status == Status::Paused
                || (status == Status::Completed && (!seeding_enabled || seed_paused));
            if should_pause {
                if !handle.is_paused() {
                    let _ = self.session.pause(&handle).await;
                }
            } else if handle.is_paused() {
                match self.session.unpause(&handle).await {
                    Ok(()) => {
                        // Watch a fresh resume for a moment: a torrent that was
                        // paused for a long time may not find its swarm again
                        // until it is restarted.
                        if status != Status::Completed {
                            let mut state = self.state.lock();
                            if let Some(entry) = state.entries.get_mut(id) {
                                entry.resume_probe = Some(ResumeProbe {
                                    started: Instant::now(),
                                    baseline: entry.item.downloaded,
                                });
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!("could not resume {id}: {err:#}");
                        let mut state = self.state.lock();
                        if let Some(entry) = state.entries.get_mut(id) {
                            entry.item.error = format!("Could not resume: {err:#}");
                        }
                    }
                }
            }
            {
                let mut state = self.state.lock();
                if let Some(entry) = state.entries.get_mut(id) {
                    entry.starting = false;
                }
            }
            self.publish();
            return;
        }

        let pause_after = status == Status::Completed && (!seeding_enabled || seed_paused);
        let mut options = AddTorrentOptions {
            overwrite: true,
            // An empty folder means "use the session default". librqbit then
            // keeps single-file torrents flat and gives multi-file torrents
            // their own folder, so nothing is scattered into the download root.
            output_folder: None,
            paused: pause_after,
            // Re-announce periodically so a swarm that went quiet is picked up
            // again without restarting the app.
            force_tracker_interval: Some(Duration::from_secs(300)),
            ..Default::default()
        };
        if !dir.trim().is_empty() {
            options.output_folder = Some(dir);
        }
        if !trackers.is_empty() {
            options.trackers = Some(trackers);
        }
        tracing::info!(id, "starting torrent metadata acquisition");
        let response = tokio::time::timeout(
            Duration::from_secs(60),
            self.session
                .add_torrent(AddTorrent::from_url(magnet), Some(options)),
        )
        .await
        .map_err(|_| anyhow::anyhow!("timed out waiting for torrent metadata"))
        .and_then(|result| result);
        match response {
            Ok(response) => {
                let Some(handle) = response.into_handle() else {
                    self.start_failed(id, "The torrent carries no downloadable files")
                        .await;
                    return;
                };
                // The real output folder is only known after metadata resolves:
                // multi-file torrents get a folder of their own.
                let folder = handle.output_folder().to_string_lossy().into_owned();
                let stats = handle.stats();
                let files = torrent_files(&handle, &stats.file_progress);
                let keep = {
                    let mut state = self.state.lock();
                    match state.entries.get_mut(id) {
                        Some(entry) => {
                            entry.handle = Some(handle.clone());
                            entry.starting = false;
                            entry.resume_probe = None;
                            if !folder.is_empty() {
                                entry.item.dir = folder;
                            }
                            if !files.is_empty() {
                                entry.item.files = files;
                            }
                            true
                        }
                        None => false,
                    }
                };
                if !keep {
                    // The user removed it while metadata was resolving.
                    let _ = self
                        .session
                        .delete(TorrentIdOrHash::Id(handle.id()), false)
                        .await;
                    return;
                }

                // The pause flag can change while metadata is resolving.
                let (status, seed_paused, seeding_enabled) = {
                    let state = self.state.lock();
                    let seeding_enabled = self.config.lock().seeding_enabled;
                    state
                        .entries
                        .get(id)
                        .map(|entry| (entry.item.status, entry.item.seed_paused, seeding_enabled))
                        .unwrap_or((Status::Failed, false, seeding_enabled))
                };
                let should_pause = status == Status::Paused
                    || (status == Status::Completed && (!seeding_enabled || seed_paused));
                if should_pause {
                    if !handle.is_paused() {
                        let _ = self.session.pause(&handle).await;
                    }
                } else if handle.is_paused() {
                    if let Err(err) = self.session.unpause(&handle).await {
                        tracing::warn!("could not start {id}: {err:#}");
                    }
                }
                self.publish();
            }
            Err(err) => {
                tracing::warn!(id, error = %err, "torrent start failed");
                self.start_failed(id, &format!("{err:#}")).await;
            }
        }
    }

    /// A start that failed before attaching a torrent. A finished torrent
    /// stays in Seeding and simply stops instead of moving to Downloads.
    async fn start_failed(&self, id: &str, message: &str) {
        let mut state = self.state.lock();
        if let Some(entry) = state.entries.get_mut(id) {
            entry.starting = false;
            entry.item.speed = 0;
            entry.item.up_speed = 0;
            if entry.item.status == Status::Completed {
                entry.item.seed_paused = true;
            } else if entry.item.status != Status::Paused {
                entry.item.status = Status::Failed;
            }
            entry.item.error = message.to_owned();
        }
        drop(state);
        self.dirty.store(true, Ordering::Relaxed);
        self.publish();
        // A failed item must not hold its queue batch hostage.
        self.schedule().await;
    }

    fn fail(&self, id: &str, message: &str) {
        let mut state = self.state.lock();
        if let Some(entry) = state.entries.get_mut(id) {
            entry.item.status = Status::Failed;
            entry.item.error = message.to_owned();
            entry.item.speed = 0;
            entry.item.up_speed = 0;
            entry.starting = false;
        }
        drop(state);
        self.dirty.store(true, Ordering::Relaxed);
        self.publish();
    }

    async fn pause(&self, id: &str) {
        let handle = {
            let mut state = self.state.lock();
            let Some(entry) = state.entries.get_mut(id) else {
                return;
            };
            if !matches!(entry.item.status, Status::Downloading | Status::Queued) {
                return;
            }
            entry.item.status = Status::Paused;
            entry.item.speed = 0;
            entry.item.up_speed = 0;
            entry.item.error.clear();
            entry.handle.clone()
        };
        self.dirty.store(true, Ordering::Relaxed);
        self.publish();
        if let Some(handle) = handle {
            let _ = self.session.pause(&handle).await;
        }
        tracing::debug!("pausing {id}");
        self.schedule().await;
    }

    async fn resume(&self, id: &str) {
        let handle = {
            let mut state = self.state.lock();
            let Some(entry) = state.entries.get_mut(id) else {
                return;
            };
            if !matches!(entry.item.status, Status::Paused | Status::Failed) {
                return;
            }
            // Let the scheduler decide when it can start again: the global cap
            // and the item's queue batch both still apply.
            entry.item.status = Status::Queued;
            entry.item.error.clear();
            entry.item.speed = 0;
            entry.item.up_speed = 0;
            entry.starting = false;
            entry.resume_probe = entry.handle.as_ref().map(|_| ResumeProbe {
                started: Instant::now(),
                baseline: entry.item.downloaded,
            });
            entry.handle.clone()
        };
        self.dirty.store(true, Ordering::Relaxed);
        self.publish();
        if let Some(handle) = handle
            && !handle.is_paused()
        {
            // Do not let a handle resume behind the scheduler's back while the
            // item waits for a slot or its batch turn.
            let _ = self.session.pause(&handle).await;
        }
        tracing::debug!("resuming {id}");
        self.dirty.store(true, Ordering::Relaxed);
        self.schedule().await;
    }

    async fn remove(&self, id: &str) {
        let handle = {
            let mut state = self.state.lock();
            let Some(entry) = state.entries.remove(id) else {
                return;
            };
            state.order.retain(|existing| existing != id);
            entry.handle
        };
        self.dirty.store(true, Ordering::Relaxed);
        self.publish();
        if let Some(handle) = handle {
            let _ = self
                .session
                .delete(TorrentIdOrHash::Id(handle.id()), false)
                .await;
        }
        self.schedule().await;
    }

    async fn set_seed_paused(&self, id: &str, paused: bool) {
        if !paused && !self.config.lock().seeding_enabled {
            // The global switch wins over the per-torrent play button.
            return;
        }
        let handle = {
            let mut state = self.state.lock();
            let Some(entry) = state.entries.get_mut(id) else {
                return;
            };
            if entry.item.status != Status::Completed || entry.item.seed_paused == paused {
                return;
            }
            entry.item.seed_paused = paused;
            if !paused {
                // A manual resume restarts the seed clock, so the automatic
                // limit does not undo it a second later.
                entry.item.completed_at = unix_now();
            }
            entry.handle.clone()
        };
        if let Some(handle) = handle {
            if paused {
                let _ = self.session.pause(&handle).await;
            } else {
                let _ = self.session.unpause(&handle).await;
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        self.schedule().await;
    }

    async fn set_all_seed_paused(&self, paused: bool) {
        if !paused && !self.config.lock().seeding_enabled {
            return;
        }
        let handles = {
            let mut state = self.state.lock();
            let mut handles = Vec::new();
            for id in state.order.clone() {
                let Some(entry) = state.entries.get_mut(&id) else {
                    continue;
                };
                if entry.item.status != Status::Completed || entry.item.seed_paused == paused {
                    continue;
                }
                entry.item.seed_paused = paused;
                if !paused {
                    entry.item.completed_at = unix_now();
                }
                if let Some(handle) = &entry.handle {
                    handles.push(handle.clone());
                }
            }
            handles
        };
        for handle in handles {
            if paused {
                let _ = self.session.pause(&handle).await;
            } else {
                let _ = self.session.unpause(&handle).await;
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        self.schedule().await;
    }

    /// Turn seeding on or off globally. Finished torrents stay in Seeding
    /// either way; this only starts or stops the upload side.
    async fn set_seeding_enabled(&self, enabled: bool) {
        self.config.lock().seeding_enabled = enabled;
        let mut actions: Vec<(Arc<ManagedTorrent>, bool)> = Vec::new();
        {
            let mut state = self.state.lock();
            for id in state.order.clone() {
                let Some(entry) = state.entries.get_mut(&id) else {
                    continue;
                };
                if entry.item.status != Status::Completed {
                    continue;
                }
                let Some(handle) = &entry.handle else {
                    continue;
                };
                if enabled {
                    if !entry.item.seed_paused && handle.is_paused() {
                        actions.push((handle.clone(), false));
                    }
                } else if !handle.is_paused() {
                    actions.push((handle.clone(), true));
                }
            }
        }
        for (handle, pause) in actions {
            if pause {
                let _ = self.session.pause(&handle).await;
            } else if let Err(err) = self.session.unpause(&handle).await {
                tracing::warn!("could not resume seeding: {err:#}");
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        self.publish();
        // With seeding back on, finished torrents without a handle are
        // re-attached here.
        self.schedule().await;
    }

    async fn tick(&self) {
        let now = unix_now();
        let (seed_minutes, seeding_enabled) = {
            let config = self.config.lock();
            (config.seed_time_minutes, config.seeding_enabled)
        };
        let mut finished: Vec<(String, String, Vec<String>)> = Vec::new();
        let mut stop_seeding: Vec<Arc<ManagedTorrent>> = Vec::new();
        let mut restarts: Vec<String> = Vec::new();
        let mut changed = false;
        let mut slot_freed = false;

        {
            let mut state = self.state.lock();
            let ids = state.order.clone();
            for id in ids {
                let Some(entry) = state.entries.get_mut(&id) else {
                    continue;
                };
                let Some(handle) = entry.handle.clone() else {
                    continue;
                };

                let stats = handle.stats();
                if matches!(stats.state, TorrentStatsState::Error) {
                    if entry.item.status != Status::Failed {
                        if entry.item.status == Status::Downloading {
                            slot_freed = true;
                        }
                        entry.item.status = Status::Failed;
                        entry.item.error = stats
                            .error
                            .clone()
                            .unwrap_or_else(|| "The torrent stopped with an error".into());
                        entry.item.speed = 0;
                        entry.item.up_speed = 0;
                        changed = true;
                    }
                    continue;
                }

                let (down, up, peers) = stats
                    .live
                    .as_ref()
                    .map(|live| {
                        (
                            live.download_speed.as_bytes() as i64,
                            live.upload_speed.as_bytes() as i64,
                            live.snapshot.peer_stats.live as i64,
                        )
                    })
                    .unwrap_or((0, 0, 0));

                entry.item.size_bytes = stats.total_bytes as i64;
                entry.item.downloaded = stats.progress_bytes as i64;
                entry.item.uploaded = stats.uploaded_bytes as i64;
                entry.item.peers = peers;
                entry.item.speed = down;
                entry.item.up_speed = up;
                if let Some(name) = handle.name().filter(|n| !n.is_empty()) {
                    entry.item.name = name;
                }

                // Keep the file list and its per-file progress current. The
                // list itself only changes once, when metadata arrives.
                let files = torrent_files(&handle, &stats.file_progress);
                if !files.is_empty() {
                    let same = entry.item.files.len() == files.len()
                        && entry
                            .item
                            .files
                            .iter()
                            .zip(&files)
                            .all(|(old, new)| old.path == new.path && old.size == new.size);
                    if same {
                        for (slot, fresh) in entry.item.files.iter_mut().zip(files) {
                            slot.downloaded = fresh.downloaded;
                        }
                    } else {
                        entry.item.files = files;
                        changed = true;
                    }
                }

                let total = entry.item.size_bytes;
                let downloaded = entry.item.downloaded;
                if total > 0 {
                    entry.item.progress = (downloaded as f64 / total as f64).clamp(0.0, 1.0);
                }

                let done = stats.finished || (total > 0 && downloaded >= total);
                if entry.item.status == Status::Downloading && done {
                    entry.item.status = Status::Completed;
                    entry.item.progress = 1.0;
                    entry.item.speed = 0;
                    entry.item.completed_at = now;
                    entry.promoted = false;
                    for file in &mut entry.item.files {
                        file.downloaded = file.size;
                    }
                    if !seeding_enabled {
                        stop_seeding.push(handle.clone());
                    }
                    changed = true;
                    slot_freed = true;
                    let paths = file_paths(&handle);
                    finished.push((id.clone(), entry.item.dir.clone(), paths));
                }

                // A resume that never finds its swarm is restarted from
                // scratch, which forces a fresh tracker and DHT announce.
                if entry.item.status == Status::Downloading {
                    let clear = entry
                        .resume_probe
                        .as_ref()
                        .is_some_and(|probe| peers > 0 || downloaded > probe.baseline || down > 0);
                    if clear {
                        entry.resume_probe = None;
                    } else if entry.resume_probe.as_ref().is_some_and(|probe| {
                        resume_probe_should_restart(probe, peers, downloaded, down)
                    }) {
                        restarts.push(id.clone());
                    }
                }

                if entry.item.status == Status::Completed {
                    entry.item.speed = 0;
                    if entry.item.completed_at == 0 {
                        entry.item.completed_at = now;
                    }
                    if entry.item.seed_paused || !seeding_enabled {
                        entry.item.up_speed = 0;
                        if !seeding_enabled && !handle.is_paused() {
                            stop_seeding.push(handle.clone());
                        }
                    } else if seed_minutes > 0 && entry.item.completed_at > 0 {
                        let age = now - entry.item.completed_at;
                        if age >= (seed_minutes as i64) * 60 {
                            entry.item.seed_paused = true;
                            entry.item.up_speed = 0;
                            if let Some(handle) = &entry.handle {
                                stop_seeding.push(handle.clone());
                            }
                            changed = true;
                        }
                    }
                }

                if entry.item.status == Status::Paused {
                    entry.item.speed = 0;
                    entry.item.up_speed = 0;
                }
            }
        }

        for handle in stop_seeding {
            let _ = self.session.pause(&handle).await;
        }
        if !restarts.is_empty() {
            for id in restarts {
                self.restart(&id).await;
            }
            slot_freed = true;
        }
        for (id, dir, paths) in finished {
            self.promote(id, dir, paths);
        }
        if changed {
            self.dirty.store(true, Ordering::Relaxed);
        }
        if slot_freed {
            // A finished or failed download frees a slot for the next queued
            // item, including the next item in a sequential batch.
            self.schedule().await;
        } else {
            self.publish();
        }
    }

    /// Drop a stalled handle (keeping its files) so the scheduler can add the
    /// torrent again with a fresh set of tracker and DHT announces.
    async fn restart(&self, id: &str) {
        let handle = {
            let mut state = self.state.lock();
            let Some(entry) = state.entries.get_mut(id) else {
                return;
            };
            if entry.item.status != Status::Downloading {
                return;
            }
            entry.resume_probe = None;
            entry.starting = false;
            entry.item.status = Status::Queued;
            entry.item.speed = 0;
            entry.item.up_speed = 0;
            entry.handle.take()
        };
        if let Some(handle) = handle {
            tracing::info!("{id} found no peers after resuming; restarting the torrent");
            let _ = self
                .session
                .delete(TorrentIdOrHash::Id(handle.id()), false)
                .await;
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// The completion hook: files stay ordinary read/write files, and folders
    /// holding an episode run get a playlist beside them.
    fn promote(&self, id: String, dir: String, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(750)).await;
            let chmod_paths = paths.clone();
            let chmod_dir = dir.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let deadline = Instant::now() + Duration::from_secs(15);
                while perms::make_writable(Path::new(&chmod_dir), &chmod_paths) {
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            })
            .await;
            let playlist_dir = dir.clone();
            let playlist_paths = paths.clone();
            let _ = tokio::task::spawn_blocking(move || {
                playlist::write_playlists(Path::new(&playlist_dir), &playlist_paths);
            })
            .await;
            tracing::debug!("finished arranging files for {id}");
        });
    }

    fn publish(&self) {
        let mut items: Vec<Item> = {
            let state = self.state.lock();
            state
                .order
                .iter()
                .filter_map(|id| state.entries.get(id).map(|e| e.item.clone()))
                .collect()
        };
        items.sort_by(|a, b| b.added_at.cmp(&a.added_at));
        *self.shared.items.write() = items;
        let _ = self.shared.changes.try_send(());
    }

    fn flush(&self, force: bool) {
        let dirty = self.dirty.swap(false, Ordering::Relaxed);
        if !dirty && !force {
            return;
        }
        {
            let mut last = self.last_persist.lock();
            if !force && last.elapsed() < PERSIST_EVERY {
                self.dirty.store(true, Ordering::Relaxed);
                return;
            }
            *last = Instant::now();
        }
        self.persist();
    }

    fn persist(&self) {
        let items: Vec<Item> = {
            let state = self.state.lock();
            state
                .order
                .iter()
                .filter_map(|id| state.entries.get(id).map(|e| e.item.clone()))
                .map(|mut item| {
                    item.speed = 0;
                    item.up_speed = 0;
                    item.peers = 0;
                    item
                })
                .collect()
        };
        persist_items(&items);
    }
}

fn persist_items(items: &[Item]) {
    let Some(parent) = config::queue_path().parent().map(PathBuf::from) else {
        return;
    };
    if std::fs::create_dir_all(&parent).is_err() {
        return;
    }
    let Ok(raw) = serde_json::to_vec_pretty(items) else {
        return;
    };
    let tmp = config::queue_path().with_extension("json.tmp");
    if std::fs::write(&tmp, raw).is_ok() {
        let _ = std::fs::rename(&tmp, config::queue_path());
    }
}

/// Validate a magnet link or bare info hash and normalise it for the queue.
fn parse_download_input(input: &str) -> Result<(String, String, String), String> {
    let input = input.trim();
    match parse_magnet(input) {
        Some(parsed) => Ok((parsed.hash, parsed.name, parsed.magnet)),
        None => {
            if !is_info_hash(input) {
                return Err("Not a magnet link or info hash".into());
            }
            let hash = normalize_hash(input);
            let magnet = build_magnet(&hash, &hash, &[]);
            Ok((hash.clone(), hash, magnet))
        }
    }
}

fn source_label(source: &str) -> String {
    if source.trim().is_empty() {
        "magnet".into()
    } else {
        source.to_owned()
    }
}

/// Append the user's extra trackers, skipping ones already on the magnet.
fn with_trackers(magnet: &str, extra: &[String]) -> String {
    if extra.is_empty() {
        return magnet.to_owned();
    }
    let mut seen: HashSet<String> = trackers_of(magnet).into_iter().collect();
    let mut out = magnet.trim().to_owned();
    for tracker in extra {
        let tracker = tracker.trim();
        if tracker.is_empty() || !seen.insert(tracker.to_owned()) {
            continue;
        }
        out.push_str("&tr=");
        out.push_str(&query_escape(tracker));
    }
    out
}

/// Relative paths of every real file in a torrent, with forward slashes.
fn file_paths(handle: &Arc<ManagedTorrent>) -> Vec<String> {
    handle
        .with_metadata(|metadata| {
            metadata
                .file_infos
                .iter()
                .filter(|file| !file.attrs.padding)
                .map(|file| file.relative_filename.to_string_lossy().replace('\\', "/"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// Every real file with its size and downloaded bytes, for the downloads view.
fn torrent_files(handle: &Arc<ManagedTorrent>, progress: &[u64]) -> Vec<ItemFile> {
    handle
        .with_metadata(|metadata| {
            metadata
                .file_infos
                .iter()
                .enumerate()
                .filter(|(_, file)| !file.attrs.padding)
                .map(|(index, file)| ItemFile {
                    path: file.relative_filename.to_string_lossy().replace('\\', "/"),
                    size: file.len as i64,
                    downloaded: progress.get(index).copied().unwrap_or(0) as i64,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnet_gets_extra_trackers_once() {
        let magnet = "magnet:?xt=urn:btih:abc&dn=Test&tr=udp%3A%2F%2Fa%3A1%2Fannounce";
        let got = with_trackers(
            magnet,
            &["udp://a:1/announce".into(), "udp://b:2/announce".into()],
        );
        assert!(got.ends_with("&tr=udp%3A%2F%2Fb%3A2%2Fannounce"));
        assert_eq!(got.matches("udp%3A%2F%2Fa%3A1").count(), 1);
    }

    #[test]
    fn timestamps_accept_rfc3339_dates() {
        #[derive(Deserialize)]
        struct Row {
            #[serde(default, deserialize_with = "timestamp")]
            at: i64,
        }
        let row: Row = serde_json::from_str(r#"{"at":"2024-05-01T12:00:00Z"}"#).unwrap();
        assert_eq!(row.at, 1_714_564_800);
        let row: Row = serde_json::from_str(r#"{"at":42}"#).unwrap();
        assert_eq!(row.at, 42);
    }

    #[test]
    fn completed_entries_stay_in_seeding_after_restore() {
        assert_eq!(restored_status(Status::Completed), Status::Completed);
        assert_eq!(restored_status(Status::Downloading), Status::Queued);
        assert_eq!(restored_status(Status::Queued), Status::Queued);
        assert_eq!(restored_status(Status::Paused), Status::Paused);
        assert_eq!(restored_status(Status::Failed), Status::Failed);
    }

    #[test]
    fn fractions_are_clamped() {
        let mut item = Item::default();
        item.progress = 3.0;
        assert_eq!(item.fraction(), 1.0);
        item.progress = -1.0;
        assert_eq!(item.fraction(), 0.0);
    }

    fn test_item(id: &str, status: Status) -> Item {
        Item {
            id: id.to_owned(),
            name: id.to_owned(),
            magnet: format!("magnet:?xt=urn:btih:{id}"),
            status,
            ..Default::default()
        }
    }

    fn test_state(items: Vec<Item>) -> State {
        let mut state = State::default();
        for item in items {
            state.order.push(item.id.clone());
            state.entries.insert(item.id.clone(), Entry::new(item));
        }
        state
    }

    fn test_manager(items: Vec<Item>) -> (TorrentManager, UnboundedReceiver<Command>) {
        let (tx, rx) = unbounded_channel();
        let (changes, change_rx) = async_channel::bounded(8);
        (
            TorrentManager {
                shared: Arc::new(Shared {
                    tx,
                    items: RwLock::new(items),
                    changes,
                    change_rx: Mutex::new(Some(change_rx)),
                    startup_error: Mutex::new(None),
                    thread: Mutex::new(None),
                }),
            },
            rx,
        )
    }

    #[test]
    fn queue_commands_update_the_visible_snapshot_immediately() {
        let mut item = test_item(
            "0123456789abcdef0123456789abcdef01234567",
            Status::Downloading,
        );
        item.speed = 10;
        let id = item.id.clone();
        let (manager, _commands) = test_manager(vec![item]);

        manager.pause(&id).unwrap();
        assert_eq!(manager.snapshot()[0].status, Status::Paused);
        assert_eq!(manager.snapshot()[0].speed, 0);

        manager.resume(&id).unwrap();
        assert_eq!(manager.snapshot()[0].status, Status::Queued);

        manager.remove(&id).unwrap();
        assert!(manager.snapshot().is_empty());
    }

    #[test]
    fn add_is_visible_before_the_engine_starts_it() {
        let (manager, _commands) = test_manager(Vec::new());
        let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Cyberpunk";

        manager.add(magnet, "tpb").unwrap();

        let item = manager.snapshot().pop().unwrap();
        assert_eq!(item.status, Status::Downloading);
        assert_eq!(item.name, "Cyberpunk");
    }

    #[test]
    fn queued_batches_run_one_at_a_time() {
        let mut first = test_item("first", Status::Queued);
        first.queue_group = Some("batch".into());
        let mut second = test_item("second", Status::Queued);
        second.queue_group = Some("batch".into());
        let mut third = test_item("third", Status::Queued);
        third.queue_group = Some("batch".into());
        let mut state = test_state(vec![first, second, third]);

        assert_eq!(plan_downloads(&mut state, 0), vec!["first"]);
        state.entries.get_mut("first").unwrap().item.status = Status::Completed;
        assert_eq!(plan_downloads(&mut state, 0), vec!["second"]);
        state.entries.get_mut("second").unwrap().item.status = Status::Failed;
        assert_eq!(plan_downloads(&mut state, 0), vec!["third"]);
    }

    #[test]
    fn start_all_batches_bypass_the_download_cap() {
        let mut first = test_item("first", Status::Queued);
        first.start_all = true;
        let normal = test_item("normal", Status::Queued);
        let mut last = test_item("last", Status::Queued);
        last.start_all = true;
        let mut state = test_state(vec![first, normal, last]);

        assert_eq!(plan_downloads(&mut state, 1), vec!["first", "last"]);
    }

    #[test]
    fn normal_downloads_still_respect_the_cap() {
        let mut state = test_state(vec![
            test_item("first", Status::Queued),
            test_item("second", Status::Queued),
        ]);
        assert_eq!(plan_downloads(&mut state, 1), vec!["first"]);
        assert_eq!(
            state.entries.get("second").unwrap().item.status,
            Status::Queued
        );
    }

    #[test]
    fn a_busy_batch_does_not_block_other_downloads() {
        let mut active = test_item("active", Status::Downloading);
        active.queue_group = Some("batch".into());
        let mut waiting = test_item("waiting", Status::Queued);
        waiting.queue_group = Some("batch".into());
        let other = test_item("other", Status::Queued);
        let mut state = test_state(vec![active, waiting, other]);

        assert_eq!(plan_downloads(&mut state, 0), vec!["other"]);
        assert_eq!(
            state.entries.get("waiting").unwrap().item.status,
            Status::Queued
        );
    }

    #[test]
    fn a_batch_keeps_the_order_the_rows_were_chosen_in() {
        let mut state = test_state(Vec::new());
        insert_batch_items(
            &mut state,
            vec![
                test_item("first", Status::Queued),
                test_item("second", Status::Queued),
                test_item("third", Status::Queued),
            ],
        );
        assert_eq!(state.order, vec!["first", "second", "third"]);
    }

    #[test]
    fn batch_fields_survive_a_queue_round_trip() {
        let mut item = test_item("first", Status::Queued);
        item.queue_group = Some("batch".into());
        item.start_all = true;
        let raw = serde_json::to_string(&item).unwrap();
        let back: Item = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.queue_group.as_deref(), Some("batch"));
        assert!(back.start_all);
    }

    #[test]
    fn completed_items_are_not_reseeded_when_disabled() {
        let mut state = test_state(vec![test_item("done", Status::Completed)]);
        assert!(plan_reseeds(&mut state, false).is_empty());
        assert_eq!(plan_reseeds(&mut state, true), vec!["done"]);
    }

    #[test]
    fn file_metadata_round_trips_with_the_queue() {
        let mut item = test_item("files", Status::Completed);
        item.files.push(ItemFile {
            path: "Season 1/episode.mkv".into(),
            size: 123,
            downloaded: 123,
        });
        let back: Item = serde_json::from_str(&serde_json::to_string(&item).unwrap()).unwrap();
        assert_eq!(back.files, item.files);
    }

    #[test]
    fn stalled_resume_probe_requires_no_progress() {
        let probe = ResumeProbe {
            started: Instant::now() - RESUME_GRACE - Duration::from_secs(1),
            baseline: 100,
        };
        assert!(resume_probe_should_restart(&probe, 0, 100, 0));
        assert!(!resume_probe_should_restart(&probe, 1, 100, 0));
        assert!(!resume_probe_should_restart(&probe, 0, 101, 0));
        assert!(!resume_probe_should_restart(&probe, 0, 100, 1));
    }
}
