//! The window's state, key map and shell.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    ClipboardItem, Context, Div, Entity, FocusHandle, Focusable, FontWeight, Hsla, ListAlignment,
    ListState, Pixels, Point, Render, ScrollHandle, SharedString, Task, UniformListScrollHandle,
    Window, WindowAppearance, actions, div, hsla, prelude::*, px,
};
use nori_core::config::{self, Config, parse_trackers};
use nori_core::search::model::source_tag;
use nori_core::search::{
    self, CancellationToken, Group, SearchEvent, SearchResult, Searcher, SourceMeta, build_magnet,
    filter_by_text, is_info_hash, normalize_hash, parse_magnet, sources, sources_for,
};
use nori_core::torrent::{BatchMode, Item, ItemFile, Status, TorrentManager};

use crate::icons;
use crate::input::TextInput;
use crate::theme::{Theme, source_tint};
use crate::views;
use crate::widgets::{self, MONO_FONT, UI_FONT};

actions!(
    nori,
    [
        FocusSearch,
        PasteMagnet,
        SearchAgain,
        OpenSettings,
        Escape,
        DownloadPicked,
        CopyPicked,
        CycleSort,
        ToggleFilter,
        ToggleHideDead,
        PauseDownload,
        RemoveDownload,
        OpenFolder,
        PauseSeed,
        RemoveSeed,
        SelectNext,
        SelectPrev,
        SelectAllResults,
        OpenPicked,
        Submit,
        Quit,
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Browse,
    Downloads,
    Seeding,
    Settings,
}

impl Section {
    pub fn id(self) -> &'static str {
        match self {
            Section::Browse => "browse",
            Section::Downloads => "downloads",
            Section::Seeding => "seeding",
            Section::Settings => "settings",
        }
    }

    pub fn context(self) -> &'static str {
        match self {
            Section::Browse => "Results",
            Section::Downloads => "Downloads",
            Section::Seeding => "Seeding",
            Section::Settings => "Settings",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceState {
    pub loading: bool,
    pub error: Option<String>,
    pub count: usize,
}

/// Width and height of the sort menu, in pixels. Used to place the overlay.
pub const SORT_MENU_WIDTH: f32 = 186.;
pub const SORT_MENU_HEIGHT: f32 = 336.;

/// The field the result list is ordered by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    Default,
    Size,
    Seeders,
    Source,
}

/// The full sort choice: the field, its direction, and the pinned source.
#[derive(Clone, Copy, Debug)]
pub struct Sort {
    pub by: SortBy,
    /// Largest / most first when true.
    pub desc: bool,
    /// Source tag to put first when sorting by source.
    pub source: Option<&'static str>,
}

impl Default for Sort {
    fn default() -> Self {
        Sort {
            by: SortBy::Default,
            desc: true,
            source: None,
        }
    }
}

impl Sort {
    /// Short description shown on the toolbar button.
    pub fn label(&self) -> String {
        match self.by {
            SortBy::Default => "Sort".to_string(),
            SortBy::Size => format!("Sort: {}", if self.desc { "Largest" } else { "Smallest" }),
            SortBy::Seeders => format!("Sort: {}", if self.desc { "Highest" } else { "Lowest" }),
            SortBy::Source => match self.source {
                Some(tag) => format!("Sort: {tag}"),
                None => "Sort: Source".to_string(),
            },
        }
    }

    pub fn is_active(&self) -> bool {
        self.by != SortBy::Default
    }
}

/// The source tags the source sort can pin, in a stable order.
pub static SORT_SOURCES: [&str; 8] = ["FG", "YTS", "EZTV", "NYAA", "SUB", "TPB", "1337", "BT"];

/// Source tags that should be offered for a category, keeping only tags with
/// at least one enabled source in that group.
pub fn sort_tags_for(group: Option<Group>, enabled: impl Fn(&str) -> bool) -> Vec<&'static str> {
    SORT_SOURCES
        .into_iter()
        .filter(|tag| {
            sources_for(group)
                .iter()
                .any(|source| source_tag(source.key) == *tag && enabled(source.key))
        })
        .collect()
}

pub struct Toast {
    pub message: String,
    pub at: Instant,
}

/// The choice sheet shown when several results are downloaded together.
#[derive(Clone, Debug)]
pub struct BatchPrompt {
    pub rows: Vec<SearchResult>,
}

pub struct App {
    pub cfg: Config,
    pub manager: TorrentManager,
    pub searcher: Arc<Searcher>,
    pub runtime: tokio::runtime::Handle,

    pub section: Section,
    pub category: usize,

    pub search_input: Entity<TextInput>,
    pub filter_input: Entity<TextInput>,
    pub trackers_input: Entity<TextInput>,
    pub filter_text: String,
    pub trackers_text: String,

    pub results: Vec<SearchResult>,
    pub shown: Vec<SearchResult>,
    pub states: HashMap<String, SourceState>,
    pub loading: usize,
    pub search_id: u64,
    pub cancel: Option<CancellationToken>,

    pub sort: Sort,
    pub sort_open: bool,
    pub sort_submenu: Option<SortBy>,
    pub sort_menu_at: Option<Point<Pixels>>,
    pub hide_dead: bool,
    pub chosen: HashSet<String>,
    /// Chosen results in the order they were ticked. Kept separately so a
    /// filter or a new search does not lose the selection's details.
    pub chosen_results: Vec<SearchResult>,
    pub selected: Option<usize>,
    pub show_filter: bool,
    pub detail_open: bool,
    pub detail_touched: bool,
    pub detail_expanded: Option<String>,
    pub detail_list: ListState,
    pub detail_width: f32,
    pub detail_resizing: bool,
    pub preview_hash: Option<String>,
    pub preview_files: Vec<ItemFile>,
    pub preview_loading: bool,
    pub preview_error: Option<String>,

    pub downloads: Vec<Item>,
    pub dl_selected: Option<usize>,
    pub done_selected: Option<usize>,
    pub seed_selected: Option<usize>,
    pub dl_hover: Option<usize>,
    pub seed_hover: Option<usize>,

    pub help_open: bool,
    pub toast: Option<Toast>,
    pub batch_prompt: Option<BatchPrompt>,

    pub list_focus: FocusHandle,
    pub result_scroll: UniformListScrollHandle,
    pub dl_scroll: UniformListScrollHandle,
    pub done_scroll: UniformListScrollHandle,
    pub seed_scroll: UniformListScrollHandle,
    pub settings_scroll: ScrollHandle,
}

impl App {
    pub fn new(
        cfg: Config,
        manager: TorrentManager,
        runtime: tokio::runtime::Handle,
        searcher: Arc<Searcher>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = cx.new(|cx| {
            TextInput::new(
                "Search torrents, or paste a magnet link",
                "TextInput SearchInput",
                13.,
                cx,
            )
        });
        let filter_input = cx.new(|cx| {
            TextInput::new(
                "Filter the results by name",
                "TextInput FilterInput",
                12.5,
                cx,
            )
        });
        let trackers_input = cx.new(|cx| {
            TextInput::new(
                "udp://tracker.example:1337/announce, https://…",
                "TextInput TrackersInput",
                12.,
                cx,
            )
        });

        let trackers_text = cfg.trackers.join("\n");
        trackers_input.update(cx, |input, cx| input.set_text(trackers_text.clone(), cx));

        cx.observe(&search_input, |_this, _input, cx| cx.notify())
            .detach();
        cx.observe(&filter_input, |this, input, cx| {
            let text = input.read(cx).text().to_owned();
            if text == this.filter_text {
                return;
            }
            this.filter_text = text;
            this.refresh();
            cx.notify();
        })
        .detach();
        cx.observe(&trackers_input, |this, input, cx| {
            let text = input.read(cx).text().to_owned();
            if text == this.trackers_text {
                return;
            }
            this.trackers_text = text;
            this.cfg.trackers = parse_trackers(&this.trackers_text);
            this.manager.set_trackers(this.cfg.trackers.clone());
            this.save_config();
            cx.notify();
        })
        .detach();

        let changes = manager.changes();
        cx.spawn(async move |this, cx| {
            while let Ok(()) = changes.recv().await {
                let alive = this
                    .update(cx, |app, cx| {
                        app.downloads = app.manager.snapshot();
                        app.clamp_selections();
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();

        let downloads = manager.snapshot();
        let hide_dead = cfg.hide_dead;

        let mut app = Self {
            cfg,
            manager,
            searcher,
            runtime,
            section: Section::Browse,
            category: 0,
            search_input,
            filter_input,
            trackers_input,
            filter_text: String::new(),
            trackers_text,
            results: Vec::new(),
            shown: Vec::new(),
            states: HashMap::new(),
            loading: 0,
            search_id: 0,
            cancel: None,
            sort: Sort::default(),
            sort_open: false,
            sort_submenu: None,
            sort_menu_at: None,
            hide_dead,
            chosen: HashSet::new(),
            chosen_results: Vec::new(),
            selected: None,
            show_filter: false,
            detail_open: false,
            detail_touched: false,
            detail_expanded: None,
            detail_list: ListState::new(0, ListAlignment::Top, px(300.)),
            detail_width: 340.,
            detail_resizing: false,
            preview_hash: None,
            preview_files: Vec::new(),
            preview_loading: false,
            preview_error: None,
            downloads,
            dl_selected: None,
            done_selected: None,
            seed_selected: None,
            dl_hover: None,
            seed_hover: None,
            help_open: false,
            toast: None,
            batch_prompt: None,
            list_focus: cx.focus_handle(),
            result_scroll: UniformListScrollHandle::new(),
            dl_scroll: UniformListScrollHandle::new(),
            done_scroll: UniformListScrollHandle::new(),
            seed_scroll: UniformListScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
        };
        app.start_search(cx);
        app
    }

    pub fn theme(&self, window: &Window) -> Theme {
        Theme::resolve(self.cfg.theme_preference(), window.appearance())
    }

    /// Move the keyboard out of a text field and onto the list, so the section
    /// shortcuts can fire. Clicking a row and finishing a search both land
    /// here.
    pub fn focus_list(&self, window: &mut Window, cx: &mut gpui::App) {
        self.list_focus.focus(window, cx);
    }

    pub fn focus_search(&self, window: &mut Window, cx: &mut gpui::App) {
        let handle = self.search_input.read(cx).focus_handle(cx);
        handle.focus(window, cx);
    }

    pub fn focus_filter(&self, window: &mut Window, cx: &mut gpui::App) {
        let handle = self.filter_input.read(cx).focus_handle(cx);
        handle.focus(window, cx);
    }

    pub fn save_config(&self) {
        let _ = self.cfg.save();
    }

    pub fn set_toast(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        let at = Instant::now();
        self.toast = Some(Toast {
            message: message.into().to_string(),
            at,
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            let _ = this.update(cx, |app, cx| {
                if app.toast.as_ref().is_some_and(|toast| toast.at == at) {
                    app.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn navigate(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        self.detail_open = false;
        self.batch_prompt = None;
        self.close_sort_menu();
        cx.notify();
    }

    pub fn active_downloads(&self) -> Vec<Item> {
        self.downloads
            .iter()
            .filter(|item| item.status != Status::Completed)
            .cloned()
            .collect()
    }

    pub fn completed_downloads(&self) -> Vec<Item> {
        self.downloads
            .iter()
            .filter(|item| item.status == Status::Completed)
            .cloned()
            .collect()
    }

    pub fn clamp_selections(&mut self) {
        let active = self
            .downloads
            .iter()
            .filter(|i| i.status != Status::Completed)
            .count();
        let done = self.downloads.len() - active;
        self.dl_selected = self.dl_selected.filter(|i| *i < active);
        self.done_selected = self.done_selected.filter(|i| *i < done);
        self.seed_selected = self.seed_selected.filter(|i| *i < done);
        if let Some(selected) = self.selected
            && selected >= self.shown.len()
        {
            self.selected = self.shown.len().checked_sub(1);
        }
    }

    pub fn category_group(&self) -> Option<Group> {
        match self.category {
            1 => Some(Group::Movies),
            2 => Some(Group::TV),
            3 => Some(Group::Anime),
            4 => Some(Group::Games),
            _ => None,
        }
    }

    pub fn enabled_sources(&self) -> Vec<&'static SourceMeta> {
        sources_for(self.category_group())
            .into_iter()
            .filter(|source| self.cfg.enabled(source.key))
            .collect()
    }

    /// Source tags that should appear in the sort submenu: only sources the
    /// user has left enabled in this category, in the stable tag order.
    pub fn enabled_sort_tags(&self) -> Vec<&'static str> {
        sort_tags_for(self.category_group(), |key| self.cfg.enabled(key))
    }

    /// Reconcile the browse state after a source is switched on or off.
    pub fn sources_changed(&mut self, cx: &mut Context<Self>) {
        let enabled: HashSet<String> = self
            .enabled_sources()
            .iter()
            .map(|source| source.key.to_owned())
            .collect();
        self.states.retain(|key, _| enabled.contains(key));
        self.results
            .retain(|result| self.cfg.enabled(&result.source));

        // A result from a source the user just disabled should not linger in
        // the details bar either.
        self.chosen_results
            .retain(|result| self.cfg.enabled(&result.source));
        let keep: HashSet<String> = self
            .chosen_results
            .iter()
            .map(|result| result.info_hash.clone())
            .collect();
        self.chosen.retain(|hash| keep.contains(hash));

        if let Some(pinned) = self.sort.source
            && !self.enabled_sort_tags().contains(&pinned)
        {
            self.sort = Sort::default();
        }
        self.refresh();
        cx.notify();
    }

    pub fn reports_health(&self, source_id: &str) -> bool {
        sources()
            .iter()
            .find(|source| source.key == source_id)
            .map(SourceMeta::reports_health)
            .unwrap_or(true)
    }

    pub fn refresh(&mut self) {
        let mut out: Vec<SearchResult> = self
            .results
            .iter()
            .filter(|result| {
                self.cfg.enabled(&result.source)
                    && !(self.hide_dead
                        && result.seeders == 0
                        && self.reports_health(&result.source))
            })
            .cloned()
            .collect();
        if !self.filter_text.trim().is_empty() {
            out = filter_by_text(&out, &self.filter_text);
        }
        if let Some(pinned) = self.sort.source
            && !out
                .iter()
                .any(|result| source_tag(&result.source) == pinned)
        {
            self.sort = Sort::default();
        }
        if self.sort.by == SortBy::Source && self.sort.source.is_none() {
            self.sort = Sort::default();
        }
        sort_results(&mut out, &self.sort);
        self.shown = out;
        if let Some(selected) = self.selected
            && selected >= self.shown.len()
        {
            self.selected = self.shown.len().checked_sub(1);
        }
        // Selections survive filtering and new searches. Keep the stored
        // copies fresh whenever the same torrent shows up in the new results.
        if !self.chosen_results.is_empty() {
            let fresh: HashMap<&str, &SearchResult> = self
                .results
                .iter()
                .map(|result| (result.info_hash.as_str(), result))
                .collect();
            for result in &mut self.chosen_results {
                if let Some(updated) = fresh.get(result.info_hash.as_str()) {
                    *result = (*updated).clone();
                }
            }
        }
    }

    pub fn cancel_search(&mut self) {
        if let Some(token) = self.cancel.take() {
            token.cancel();
        }
    }

    pub fn start_search(&mut self, cx: &mut Context<Self>) {
        self.cancel_search();
        self.batch_prompt = None;
        self.search_id = self.search_id.wrapping_add(1);
        let id = self.search_id;
        let query = self.search_input.read(cx).text().trim().to_owned();

        if let Some(parsed) = parse_magnet(&query) {
            self.add_magnet(&parsed.magnet, &parsed.name, cx);
            return;
        }
        if is_info_hash(&query) {
            let hash = normalize_hash(&query);
            let magnet = build_magnet(&hash, &hash, &[]);
            self.add_magnet(&magnet, &hash, cx);
            return;
        }

        let enabled = self.enabled_sources();
        self.states = enabled
            .iter()
            .map(|source| {
                (
                    source.key.to_owned(),
                    SourceState {
                        loading: true,
                        error: None,
                        count: 0,
                    },
                )
            })
            .collect();
        self.results.clear();
        self.selected = None;
        self.reset_detail_expansion();
        self.loading = enabled.len();
        self.refresh();

        if enabled.is_empty() {
            self.loading = 0;
            cx.notify();
            return;
        }

        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        let (tx, rx) = async_channel::unbounded::<SearchEvent>();
        self.searcher.run(&self.runtime, enabled, query, cancel, tx);
        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                let alive = this
                    .update(cx, |app, cx| {
                        if app.search_id != id {
                            return false;
                        }
                        app.apply_event(event);
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub fn apply_event(&mut self, event: SearchEvent) {
        // A source may have been switched off while its search was running.
        if !self.cfg.enabled(&event.source) {
            self.loading = self.loading.saturating_sub(1);
            self.refresh();
            return;
        }
        let state = self
            .states
            .entry(event.source.clone())
            .or_insert(SourceState {
                loading: false,
                error: None,
                count: 0,
            });
        state.loading = false;
        state.count = event.results.len();
        state.error = event.error;
        self.results = search::dedupe(
            self.results
                .iter()
                .cloned()
                .chain(event.results)
                .collect::<Vec<_>>(),
        );
        self.loading = self.loading.saturating_sub(1);
        self.refresh();
    }

    pub fn add_magnet(&mut self, magnet: &str, name: &str, cx: &mut Context<Self>) {
        match self.manager.add(magnet, "magnet") {
            Ok(()) => {
                self.search_input.update(cx, |input, cx| input.clear(cx));
                self.search_id = self.search_id.wrapping_add(1);
                self.loading = 0;
                self.set_toast(format!("Added {name}"), cx);
            }
            Err(err) => self.set_toast(err, cx),
        }
    }

    fn magnet_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.set_toast("The clipboard holds no magnet link", cx);
            return;
        };
        let text = text.trim().to_owned();
        if let Some(parsed) = parse_magnet(&text) {
            self.add_magnet(&parsed.magnet, &parsed.name, cx);
            return;
        }
        if is_info_hash(&text) {
            let hash = normalize_hash(&text);
            let magnet = build_magnet(&hash, &hash, &[]);
            self.add_magnet(&magnet, &hash, cx);
            return;
        }
        self.set_toast("The clipboard holds no magnet link", cx);
    }

    /// Return the detail-pane accordion to its default: only the first
    /// selected torrent open, and the next click starts from there.
    pub fn reset_detail_expansion(&mut self) {
        self.detail_touched = false;
        self.detail_expanded = None;
        self.detail_list.reset(self.pick().len());
    }

    /// Open or close one selected torrent in the multi-selection accordion.
    /// Only one torrent is open at a time so the narrow pane stays readable.
    pub fn toggle_detail_result(&mut self, hash: &str, is_open: bool, cx: &mut Context<Self>) {
        let rows = self.pick();
        let previous_open = if self.detail_touched {
            self.detail_expanded.clone()
        } else {
            rows.first().map(|result| result.info_hash.clone())
        };

        self.detail_touched = true;
        self.detail_expanded = if is_open { None } else { Some(hash.to_owned()) };

        // A card's height changes when it opens or closes. Re-measure the
        // affected rows so the virtual list keeps its scroll extent honest.
        for target in [previous_open.as_deref(), self.detail_expanded.as_deref()] {
            if let Some(target) = target
                && let Some(index) = rows.iter().position(|row| row.info_hash == target)
            {
                self.detail_list.splice(index..index + 1, 1);
            }
        }
        if !is_open && let Some(result) = rows.iter().find(|row| row.info_hash == hash).cloned() {
            self.request_preview(&result, cx);
        }
        cx.notify();
    }

    pub fn pick(&self) -> Vec<SearchResult> {
        if !self.chosen.is_empty() {
            return self.chosen_results.clone();
        }
        self.selected
            .and_then(|index| self.shown.get(index).cloned())
            .into_iter()
            .collect()
    }

    /// Resolve and cache the file listing for a selected search result.
    pub fn request_preview(&mut self, result: &SearchResult, cx: &mut Context<Self>) {
        if self.preview_hash.as_deref() == Some(result.info_hash.as_str())
            && (self.preview_loading
                || !self.preview_files.is_empty()
                || self.preview_error.is_some())
        {
            return;
        }

        let hash = result.info_hash.clone();
        self.preview_hash = Some(hash.clone());
        self.preview_files.clear();
        self.preview_error = None;
        self.preview_loading = true;

        let response = self.manager.preview_files(&result.magnet);
        let timeout_hash = hash.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(20))
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.preview_hash.as_deref() == Some(timeout_hash.as_str()) && app.preview_loading
                {
                    app.preview_loading = false;
                    app.preview_error = Some(
                        "Metadata did not arrive within 20 seconds; try selecting it again"
                            .to_owned(),
                    );
                    cx.notify();
                }
            });
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = response
                .recv()
                .await
                .unwrap_or_else(|_| Err("File preview was cancelled".to_owned()));
            let _ = this.update(cx, |app, cx| {
                if app.preview_hash.as_deref() != Some(hash.as_str()) {
                    return;
                }
                app.preview_loading = false;
                match result {
                    Ok(files) => app.preview_files = files,
                    Err(error) => app.preview_error = Some(error),
                }
                let picked = app.pick();
                if picked.len() > 1
                    && let Some(index) = picked.iter().position(|row| row.info_hash == hash)
                {
                    app.detail_list.splice(index..index + 1, 1);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn request_preview_for_pick(&mut self, cx: &mut Context<Self>) {
        let rows = self.pick();
        let selected = if rows.len() == 1 {
            rows.first()
        } else if self.detail_touched {
            self.detail_expanded
                .as_deref()
                .and_then(|hash| rows.iter().find(|row| row.info_hash == hash))
        } else {
            rows.first()
        };
        if let Some(selected) = selected.cloned() {
            self.request_preview(&selected, cx);
        }
    }

    /// Drop every chosen result, including any the current filter hides.
    pub fn clear_chosen(&mut self) {
        self.chosen.clear();
        self.chosen_results.clear();
    }

    /// Tick or untick one result, keeping the ordered copies in step.
    pub fn toggle_chosen(&mut self, result: &SearchResult) {
        if self.chosen.remove(&result.info_hash) {
            self.chosen_results
                .retain(|picked| picked.info_hash != result.info_hash);
        } else {
            self.chosen.insert(result.info_hash.clone());
            self.chosen_results.push(result.clone());
        }
    }

    /// Add every visible result to the selection, or remove only the visible
    /// ones. Selections hidden by the current filter are left alone.
    pub fn set_all_shown_chosen(&mut self, on: bool) {
        let visible: HashSet<String> = self
            .shown
            .iter()
            .map(|result| result.info_hash.clone())
            .collect();
        if on {
            for result in &self.shown {
                if self.chosen.insert(result.info_hash.clone()) {
                    self.chosen_results.push(result.clone());
                }
            }
        } else {
            self.chosen.retain(|hash| !visible.contains(hash));
            self.chosen_results
                .retain(|picked| !visible.contains(&picked.info_hash));
        }
    }

    pub fn download_picked(&mut self, cx: &mut Context<Self>) {
        if self.batch_prompt.is_some() {
            return;
        }
        let rows = self.pick();
        if rows.is_empty() {
            self.set_toast("Choose a result first", cx);
            return;
        }
        if rows.len() > 1 {
            self.close_sort_menu();
            self.help_open = false;
            self.batch_prompt = Some(BatchPrompt { rows });
            cx.notify();
            return;
        }
        let row = &rows[0];
        match self.manager.add(&row.magnet, &row.source) {
            Ok(()) => {
                self.clear_chosen();
                // Stay on the results, with the added torrent still selected, so
                // its details and file list stay on screen.
                self.selected = self
                    .shown
                    .iter()
                    .position(|result| result.info_hash == row.info_hash)
                    .or(self.selected);
                self.reset_detail_expansion();
                self.set_toast(format!("Added {}", row.name), cx);
            }
            Err(err) => self.set_toast(err, cx),
        }
    }

    /// Confirm the open batch sheet and hand the whole group to the queue.
    pub fn confirm_batch(&mut self, mode: BatchMode, cx: &mut Context<Self>) {
        let Some(prompt) = self.batch_prompt.take() else {
            return;
        };
        let rows: Vec<(String, String)> = prompt
            .rows
            .iter()
            .map(|row| (row.magnet.clone(), row.source.clone()))
            .collect();
        match self.manager.add_batch(&rows, mode) {
            Ok(count) => {
                self.clear_chosen();
                if let Some(first) = prompt.rows.first() {
                    // Keep the batch visible in the details pane instead of
                    // jumping to Downloads.
                    self.selected = self
                        .shown
                        .iter()
                        .position(|result| result.info_hash == first.info_hash)
                        .or(self.selected);
                }
                self.reset_detail_expansion();
                let verb = if mode == BatchMode::Queue {
                    "Queued"
                } else {
                    "Added"
                };
                let plural = if count == 1 { "" } else { "s" };
                self.set_toast(format!("{verb} {count} download{plural}"), cx);
            }
            Err(err) => self.set_toast(err, cx),
        }
        cx.notify();
    }

    pub fn cancel_batch(&mut self, cx: &mut Context<Self>) {
        if self.batch_prompt.take().is_some() {
            cx.notify();
        }
    }

    pub fn copy_picked(&mut self, cx: &mut Context<Self>) {
        let rows = self.pick();
        if rows.is_empty() {
            self.set_toast("Choose a result first", cx);
            return;
        }
        if rows.len() == 1 {
            cx.write_to_clipboard(ClipboardItem::new_string(rows[0].magnet.clone()));
            self.set_toast("Magnet link copied", cx);
            return;
        }
        let magnets = rows
            .iter()
            .map(|row| row.magnet.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(magnets));
        self.set_toast(format!("Copied {} magnet links", rows.len()), cx);
    }

    pub fn source_details(&self) -> String {
        let mut lines = Vec::new();
        for source in sources() {
            let Some(state) = self.states.get(source.key) else {
                continue;
            };
            let mut line = format!("{} {}", source.title, source.groups_label());
            if let Some(error) = &state.error {
                line.push_str(" — ");
                line.push_str(error);
            } else if state.loading {
                line.push_str(" — searching");
            } else {
                let plural = if state.count == 1 { "" } else { "s" };
                line.push_str(&format!(" — {} result{plural}", state.count));
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    pub fn close_detail(&mut self, cx: &mut Context<Self>) {
        self.clear_chosen();
        self.selected = None;
        self.detail_open = false;
        self.reset_detail_expansion();
        self.refresh();
        cx.notify();
    }

    pub fn select_offset(&mut self, delta: i32, cx: &mut Context<Self>) {
        match self.section {
            Section::Browse => {
                if self.shown.is_empty() {
                    return;
                }
                let next = match (self.selected, delta) {
                    (None, 1) => 0,
                    (None, _) => 0,
                    (Some(index), 1) => (index + 1).min(self.shown.len() - 1),
                    (Some(index), -1) => index.saturating_sub(1),
                    (Some(index), _) => index,
                };
                self.selected = Some(next);
                self.detail_open = false;
                if let Some(result) = self.shown.get(next).cloned() {
                    self.request_preview(&result, cx);
                }
                self.result_scroll
                    .scroll_to_item(next, gpui::ScrollStrategy::Center);
            }
            Section::Downloads => {
                let count = self
                    .downloads
                    .iter()
                    .filter(|item| item.status != Status::Completed)
                    .count();
                if count == 0 {
                    return;
                }
                let next = match (self.dl_selected, delta) {
                    (None, _) => 0,
                    (Some(index), 1) => (index + 1).min(count - 1),
                    (Some(index), -1) => index.saturating_sub(1),
                    (Some(index), _) => index,
                };
                self.dl_selected = Some(next);
                self.dl_scroll
                    .scroll_to_item(next, gpui::ScrollStrategy::Center);
            }
            Section::Seeding => {
                let count = self
                    .downloads
                    .iter()
                    .filter(|item| item.status == Status::Completed)
                    .count();
                if count == 0 {
                    return;
                }
                let next = match (self.seed_selected, delta) {
                    (None, _) => 0,
                    (Some(index), 1) => (index + 1).min(count - 1),
                    (Some(index), -1) => index.saturating_sub(1),
                    (Some(index), _) => index,
                };
                self.seed_selected = Some(next);
                self.seed_scroll
                    .scroll_to_item(next, gpui::ScrollStrategy::Center);
            }
            Section::Settings => {}
        }
        cx.notify();
    }

    pub fn open_folder(&self, item: &Item, _cx: &mut Context<Self>) {
        if item.dir.trim().is_empty() {
            // Metadata has not named the folder yet; the configured download
            // folder is the best answer.
            reveal_path(std::path::Path::new(&self.cfg.download_dir));
        } else {
            reveal_item(item);
        }
    }

    pub fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let start = self.cfg.download_dir.clone();
        let (tx, rx) = async_channel::bounded::<Option<PathBuf>>(1);
        std::thread::spawn(move || {
            let output = std::process::Command::new("zenity")
                .arg("--file-selection")
                .arg("--directory")
                .arg("--title=Download folder")
                .arg(format!("--filename={start}/"))
                .output();
            let path = output
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
                .filter(|path| !path.is_empty())
                .map(PathBuf::from);
            let _ = tx.send_blocking(path);
        });
        cx.spawn(async move |this, cx| {
            let Ok(Some(path)) = rx.recv().await else {
                return;
            };
            let _ = this.update(cx, |app, cx| {
                app.cfg.set_download_dir(&path);
                app.save_config();
                app.set_toast(
                    format!(
                        "Download folder set: {} (applies after restart)",
                        path.display()
                    ),
                    cx,
                );
            });
        })
        .detach();
    }

    // ---- actions -------------------------------------------------------

    fn action_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.batch_prompt.is_some() {
            self.batch_prompt = None;
        } else if self.sort_open {
            self.close_sort_menu();
        } else if self.help_open {
            self.help_open = false;
        } else if self.detail_open {
            self.detail_open = false;
            self.refresh();
        } else if self.show_filter {
            self.show_filter = false;
            self.filter_text.clear();
            self.filter_input.update(cx, |input, cx| input.clear(cx));
            self.refresh();
        } else if self.section == Section::Browse
            && (!self.chosen.is_empty() || self.selected.is_some())
        {
            self.clear_chosen();
            self.selected = None;
            self.reset_detail_expansion();
            self.refresh();
        } else if self.section == Section::Downloads && self.dl_selected.is_some() {
            self.dl_selected = None;
        } else if self.section == Section::Seeding && self.seed_selected.is_some() {
            self.seed_selected = None;
        }
        self.focus_list(window, cx);
        cx.notify();
    }

    fn action_focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_search(window, cx);
    }

    fn action_paste_magnet(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.magnet_from_clipboard(cx);
    }

    fn action_search_again(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_search(cx);
        self.focus_list(window, cx);
    }

    /// Enter starts a search from the search field and otherwise behaves like
    /// the list's primary action.
    fn action_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search_focused = self
            .search_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        if search_focused {
            self.start_search(cx);
            self.focus_list(window, cx);
            return;
        }
        if self.section == Section::Browse {
            self.action_open_picked(window, cx);
        }
    }

    fn action_open_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Section::Settings, cx);
    }

    fn action_download_picked(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.download_picked(cx);
    }

    fn action_copy_picked(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_picked(cx);
    }

    fn action_cycle_sort(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.sort = match (self.sort.by, self.sort.desc) {
            (SortBy::Default, _) => Sort {
                by: SortBy::Size,
                desc: true,
                source: None,
            },
            (SortBy::Size, true) => Sort {
                by: SortBy::Size,
                desc: false,
                source: None,
            },
            (SortBy::Size, false) => Sort {
                by: SortBy::Seeders,
                desc: true,
                source: None,
            },
            (SortBy::Seeders, true) => Sort {
                by: SortBy::Seeders,
                desc: false,
                source: None,
            },
            _ => Sort::default(),
        };
        self.close_sort_menu();
        self.refresh();
        cx.notify();
    }

    fn action_toggle_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_filter = true;
        self.focus_filter(window, cx);
        cx.notify();
    }

    fn action_toggle_hide_dead(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.hide_dead = !self.hide_dead;
        self.cfg.hide_dead = self.hide_dead;
        self.save_config();
        self.refresh();
        cx.notify();
    }

    fn action_pause_download(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let active = self.active_downloads();
        let Some(item) = self.dl_selected.and_then(|index| active.get(index)) else {
            return;
        };
        let result = if matches!(item.status, Status::Paused | Status::Failed) {
            self.manager.resume(&item.id)
        } else {
            self.manager.pause(&item.id)
        };
        if let Err(error) = result {
            self.set_toast(error, cx);
        } else {
            cx.notify();
        }
    }

    fn action_remove_download(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let active = self.active_downloads();
        let Some(item) = self.dl_selected.and_then(|index| active.get(index)) else {
            return;
        };
        if let Err(error) = self.manager.remove(&item.id) {
            self.set_toast(error, cx);
        } else {
            self.set_toast("Deleted", cx);
        }
    }

    fn action_open_folder(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let item = match self.section {
            Section::Seeding => {
                let done = self.completed_downloads();
                self.seed_selected.and_then(|i| done.get(i)).cloned()
            }
            _ => {
                let active = self.active_downloads();
                self.dl_selected.and_then(|i| active.get(i)).cloned()
            }
        };
        if let Some(item) = item {
            self.open_folder(&item, cx);
        }
    }

    fn action_pause_seed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let done = self.completed_downloads();
        let Some(item) = self.seed_selected.and_then(|index| done.get(index)) else {
            return;
        };
        self.manager.set_seed_paused(&item.id, !item.seed_paused);
        cx.notify();
    }

    fn action_remove_seed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let done = self.completed_downloads();
        let Some(item) = self.seed_selected.and_then(|index| done.get(index)) else {
            return;
        };
        if let Err(error) = self.manager.remove(&item.id) {
            self.set_toast(error, cx);
        } else {
            cx.notify();
        }
    }

    fn action_select_next(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.select_offset(1, cx);
    }

    fn action_select_prev(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.select_offset(-1, cx);
    }

    /// Tick every visible result, or clear them all, from the toolbar box.
    pub fn toggle_all_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.section != Section::Browse || self.shown.is_empty() {
            return;
        }
        let all = self
            .shown
            .iter()
            .all(|result| self.chosen.contains(&result.info_hash));
        self.set_all_shown_chosen(!all);
        self.selected = None;
        self.reset_detail_expansion();
        self.focus_list(window, cx);
        cx.notify();
    }

    fn action_select_all_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.section != Section::Browse || self.shown.is_empty() {
            return;
        }
        self.set_all_shown_chosen(true);
        self.selected = None;
        self.reset_detail_expansion();
        self.focus_list(window, cx);
        cx.notify();
    }

    fn action_open_picked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let width = window.bounds().size.width.as_f32();
        if self.section == Section::Browse && width < 900. {
            if !self.pick().is_empty() {
                self.detail_open = true;
                cx.notify();
            }
            return;
        }
        self.download_picked(cx);
    }

    // ---- shell ---------------------------------------------------------

    fn header(
        &self,
        theme: &Theme,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        if width < 820. {
            div()
                .flex()
                .flex_col()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(12.))
                        .pt(px(8.))
                        .pb(px(6.))
                        .child(self.search_bar(theme, window, cx))
                        .child(self.header_actions(theme, cx)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(12.))
                        .pb(px(8.))
                        .child(self.category_bar(theme, cx)),
                )
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(16.))
                .py(px(8.))
                .border_b_1()
                .border_color(theme.border)
                .child(self.search_bar(theme, window, cx))
                .child(self.category_bar(theme, cx))
                .child(self.header_actions(theme, cx))
        }
    }

    fn search_bar(
        &self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let focused = self
            .search_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let empty = self.search_input.read(cx).is_empty();
        div()
            .flex()
            .items_center()
            .gap(px(7.))
            .h(px(32.))
            .flex_grow(1.0)
            .min_w(px(120.))
            .px(px(9.))
            .bg(theme.inset)
            .border_1()
            .border_color(if focused { theme.accent } else { theme.border })
            .id("search-bar")
            .on_click(cx.listener(|this, _, window, cx| {
                this.focus_search(window, cx);
            }))
            .child(icons::icon(
                icons::SEARCH,
                14.,
                if focused {
                    theme.text_secondary
                } else {
                    theme.text_tertiary
                },
            ))
            .child(
                div()
                    .flex_grow(1.0)
                    .min_w(px(0.))
                    .h(px(20.))
                    .text_color(theme.text)
                    .child(self.search_input.clone()),
            )
            .when(!empty, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(18.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.overlay_strong))
                        .id("search-clear")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.search_input.update(cx, |input, cx| input.clear(cx));
                            this.focus_search(window, cx);
                        }))
                        .child(icons::icon(icons::X, 10., theme.text_tertiary)),
                )
            })
    }

    fn category_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let labels = ["All", "Movies", "TV", "Anime", "Games"];
        let mut bar = div()
            .flex()
            .items_center()
            .gap(px(1.))
            .p(px(2.))
            .bg(theme.inset)
            .border_1()
            .border_color(theme.border);
        for (index, label) in labels.iter().enumerate() {
            let active = self.category == index;
            let mut item = div()
                .flex()
                .items_center()
                .justify_center()
                .h(px(24.))
                .px(px(9.))
                .text_size(px(11.5))
                .font_family(UI_FONT)
                .cursor_pointer()
                .id(("category", index))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.category = index;
                    this.section = Section::Browse;
                    this.detail_open = false;
                    this.start_search(cx);
                    this.focus_list(window, cx);
                }));
            if active {
                item = item
                    .bg(theme.raised)
                    .text_color(theme.text)
                    .font_weight(FontWeight::MEDIUM);
            } else {
                item = item
                    .text_color(theme.text_tertiary)
                    .hover(move |s| s.text_color(theme.text_secondary));
            }
            bar = bar.child(item.child(*label));
        }
        bar
    }

    fn header_actions(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        div().flex().items_center().gap(px(4.)).child(
            widgets::icon_button(theme, icons::SETTINGS, 14., theme.text_secondary)
                .id("header-settings")
                .on_click(cx.listener(|this, _, _, cx| this.navigate(Section::Settings, cx))),
        )
    }

    fn sidebar(&self, theme: &Theme, width: f32, cx: &mut Context<Self>) -> Div {
        let active = self
            .downloads
            .iter()
            .filter(|item| matches!(item.status, Status::Downloading | Status::Queued))
            .count();
        let seeding = if self.cfg.seeding_enabled {
            self.downloads
                .iter()
                .filter(|item| item.status == Status::Completed && !item.seed_paused)
                .count()
        } else {
            // Nothing is seeding while the global switch is off.
            0
        };
        if width < 1020. {
            return self.compact_sidebar(theme, active, seeding, cx);
        }

        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .w(px(196.))
            .flex_shrink_0()
            .p(px(8.))
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border);

        column = column
            .child(self.sidebar_item(theme, Section::Browse, icons::SEARCH, "Browse", 0, cx))
            .child(self.sidebar_item(
                theme,
                Section::Downloads,
                icons::DOWNLOAD,
                "Downloads",
                active,
                cx,
            ))
            .child(self.sidebar_item(theme, Section::Seeding, icons::SEED, "Seeding", seeding, cx));
        column
    }

    fn sidebar_item(
        &self,
        theme: &Theme,
        section: Section,
        glyph: &'static [u8],
        label: &'static str,
        badge: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.section == section;
        let mut item = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(30.))
            .px(px(8.))
            .cursor_pointer()
            .text_size(px(12.5))
            .font_family(UI_FONT)
            .border_1()
            .border_color(if selected {
                theme.border
            } else {
                gpui::transparent_black()
            })
            .id(label)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.navigate(section, cx);
                this.focus_list(window, cx);
            }));
        item = if selected {
            item.bg(theme.raised).text_color(theme.text)
        } else {
            item.text_color(theme.text_secondary)
                .hover(move |s| s.bg(theme.overlay_strong).text_color(theme.text))
        };
        item.child(icons::icon(
            glyph,
            14.,
            if selected {
                theme.text
            } else {
                theme.text_tertiary
            },
        ))
        .child(div().flex_grow(1.0).child(label))
        .when(badge > 0, |el| {
            el.child(widgets::badge(theme, badge.to_string()))
        })
    }

    fn compact_sidebar(
        &self,
        theme: &Theme,
        active: usize,
        seeding: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut column = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .w(px(54.))
            .flex_shrink_0()
            .py(px(7.))
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border);
        column = column
            .child(self.rail_item(theme, Section::Browse, icons::SEARCH, 0, cx))
            .child(self.rail_item(theme, Section::Downloads, icons::DOWNLOAD, active, cx))
            .child(self.rail_item(theme, Section::Seeding, icons::SEED, seeding, cx));
        column
    }

    fn rail_item(
        &self,
        theme: &Theme,
        section: Section,
        glyph: &'static [u8],
        badge: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.section == section;
        let mut item = div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .w(px(40.))
            .h(px(36.))
            .cursor_pointer()
            .border_1()
            .border_color(if selected {
                theme.border
            } else {
                gpui::transparent_black()
            })
            .id(section.id())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.navigate(section, cx);
                this.focus_list(window, cx);
            }));
        if selected {
            item = item.bg(theme.raised);
        } else {
            item = item.hover(move |s| s.bg(theme.overlay_strong));
        }
        item.child(icons::icon(
            glyph,
            17.,
            if selected {
                theme.accent
            } else {
                theme.text_tertiary
            },
        ))
        .when(badge > 0, |el| {
            el.child(widgets::badge(theme, badge.to_string()))
        })
    }

    fn body(&self, theme: &Theme, width: f32, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let content = match self.section {
            Section::Browse => views::browse::view(self, theme, width, window, cx),
            Section::Downloads => views::downloads::downloads_view(self, theme, width, cx),
            Section::Seeding => views::downloads::seeding_view(self, theme, width, cx),
            Section::Settings => views::settings::view(self, theme, cx),
        };
        div()
            .flex()
            .flex_grow(1.0)
            .min_h(px(0.))
            .child(self.sidebar(theme, width, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_grow(1.0)
                    .min_w(px(0.))
                    .id("section-content")
                    .key_context(self.section.context())
                    .track_focus(&self.list_focus)
                    .child(content),
            )
    }

    /// Whether any result in the visible list came from `tag`.
    pub fn source_has_results(&self, tag: &str) -> bool {
        self.shown
            .iter()
            .any(|result| source_tag(&result.source) == tag)
    }

    /// Move the sort to `next`, close the menu and reorder the list.
    fn apply_sort(&mut self, next: Sort, window: &mut Window, cx: &mut Context<Self>) {
        self.sort = next;
        self.close_sort_menu();
        self.refresh();
        self.focus_list(window, cx);
        cx.notify();
    }

    pub fn close_sort_menu(&mut self) {
        self.sort_open = false;
        self.sort_submenu = None;
    }

    fn sort_menu(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .flex()
            .flex_col()
            .w(px(SORT_MENU_WIDTH))
            .p(px(4.))
            .bg(theme.raised)
            .border_1()
            .border_color(theme.border_strong)
            .shadow_lg()
            .id("sort-menu")
            .occlude()
            .on_click(cx.listener(|_, _, _, _| {}));

        // Default
        panel = panel.child(self.sort_row(
            theme,
            "Default",
            SortBy::Default,
            self.sort.by == SortBy::Default,
            false,
            cx,
        ));

        // Size
        panel = panel.child(self.sort_row(
            theme,
            "Size",
            SortBy::Size,
            self.sort.by == SortBy::Size,
            true,
            cx,
        ));
        if self.sort_submenu == Some(SortBy::Size) {
            for (slot, (label, desc)) in [("Largest", true), ("Smallest", false)]
                .into_iter()
                .enumerate()
            {
                let chosen = self.sort.by == SortBy::Size && self.sort.desc == desc;
                let next = Sort {
                    by: SortBy::Size,
                    desc,
                    source: None,
                };
                panel = panel.child(self.sort_option(theme, label, slot, chosen, true, next, cx));
            }
        }

        // Seeders
        panel = panel.child(self.sort_row(
            theme,
            "Seeders",
            SortBy::Seeders,
            self.sort.by == SortBy::Seeders,
            true,
            cx,
        ));
        if self.sort_submenu == Some(SortBy::Seeders) {
            for (slot, (label, desc)) in [("Highest", true), ("Lowest", false)]
                .into_iter()
                .enumerate()
            {
                let chosen = self.sort.by == SortBy::Seeders && self.sort.desc == desc;
                let next = Sort {
                    by: SortBy::Seeders,
                    desc,
                    source: None,
                };
                panel =
                    panel.child(self.sort_option(theme, label, slot + 2, chosen, true, next, cx));
            }
        }

        // Source, with only the sources the user has left enabled.
        let tags = self.enabled_sort_tags();
        if !tags.is_empty() {
            panel = panel.child(self.sort_row(
                theme,
                "Source",
                SortBy::Source,
                self.sort.by == SortBy::Source,
                true,
                cx,
            ));
            if self.sort_submenu == Some(SortBy::Source) {
                for (slot, tag) in tags.into_iter().enumerate() {
                    let chosen = self.sort.by == SortBy::Source && self.sort.source == Some(tag);
                    let available = self.source_has_results(tag);
                    let next = Sort {
                        by: SortBy::Source,
                        desc: self.sort.desc,
                        source: Some(tag),
                    };
                    panel = panel.child(self.sort_option(
                        theme,
                        tag,
                        slot + 4,
                        chosen,
                        available,
                        next,
                        cx,
                    ));
                }
            }
        }

        panel
    }

    /// One top-level field row: the label selects it, the chevron opens the
    /// options that belong to it.
    fn sort_row(
        &self,
        theme: &Theme,
        label: &'static str,
        by: SortBy,
        active: bool,
        expandable: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let expanded = self.sort_submenu == Some(by);
        let next = Sort {
            by,
            desc: true,
            source: None,
        };

        let mut row = div()
            .flex()
            .items_center()
            .h(px(28.))
            .px(px(8.))
            .font_family(UI_FONT)
            .text_size(px(12.))
            .id(("sort-row", by as usize));
        row = if active {
            row.bg(theme.inset)
        } else {
            row.hover(move |s| s.bg(theme.overlay_strong))
        };

        let mut title = div()
            .flex()
            .items_center()
            .flex_grow(1.0)
            .h_full()
            .cursor_pointer()
            .id(("sort-choice", by as usize))
            .on_click(cx.listener(move |this, _, window, cx| {
                if expandable {
                    // Opening a field only reveals its options; the sort
                    // itself changes once an option is picked.
                    this.sort_submenu = if this.sort_submenu == Some(by) {
                        None
                    } else {
                        Some(by)
                    };
                    cx.notify();
                } else {
                    this.apply_sort(next, window, cx);
                }
            }))
            .child(label);
        title = if active {
            title
                .text_color(theme.accent)
                .font_weight(FontWeight::SEMIBOLD)
        } else {
            title.text_color(theme.text_secondary)
        };
        row = row.child(title);

        if active && !expandable {
            row = row.child(
                div()
                    .w(px(14.))
                    .text_size(px(11.))
                    .text_color(theme.accent)
                    .child("✓"),
            );
        } else if expandable {
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(18.))
                    .h_full()
                    .cursor_pointer()
                    .text_size(px(15.))
                    .text_color(if expanded {
                        theme.text
                    } else {
                        theme.text_tertiary
                    })
                    .id(("sort-dir", by as usize))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.sort_submenu = if this.sort_submenu == Some(by) {
                            None
                        } else {
                            Some(by)
                        };
                        cx.notify();
                    }))
                    .child("▾"),
            );
        }

        row
    }

    /// One nested option under a field row.
    fn sort_option(
        &self,
        theme: &Theme,
        label: &'static str,
        slot: usize,
        chosen: bool,
        available: bool,
        next: Sort,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut option = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(26.))
            .pl(px(22.))
            .pr(px(8.))
            .font_family(UI_FONT)
            .text_size(px(11.5))
            .cursor_pointer()
            .id(("sort-sub", slot));
        option = if available {
            option
                .text_color(if chosen {
                    theme.accent
                } else {
                    theme.text_secondary
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.apply_sort(next, window, cx);
                }))
                .child(label)
        } else {
            option
                .text_color(theme.text_ghost)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_toast(format!("No {label} results right now"), cx);
                }))
                .child(label)
                .child(
                    div()
                        .flex_none()
                        .text_size(px(10.))
                        .text_color(theme.text_tertiary)
                        .child("N/A"),
                )
        };
        if chosen && available {
            option = option.child("✓");
        }
        option
    }

    fn toast_layer(&self, theme: &Theme) -> Div {
        let Some(toast) = &self.toast else {
            return div();
        };
        div()
            .absolute()
            .bottom(px(18.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .px(px(12.))
                    .py(px(7.))
                    .bg(theme.raised)
                    .border_1()
                    .border_color(theme.border_strong)
                    .text_size(px(12.))
                    .text_color(theme.text)
                    .font_family(UI_FONT)
                    .shadow_lg()
                    .child(toast.message.clone()),
            )
    }
}

fn sort_results(list: &mut [SearchResult], sort: &Sort) {
    match sort.by {
        SortBy::Default => search::sort_default(list),
        SortBy::Size => {
            if sort.desc {
                list.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
            } else {
                list.sort_by(|a, b| a.size_bytes.cmp(&b.size_bytes));
            }
        }
        SortBy::Seeders => {
            if sort.desc {
                list.sort_by(|a, b| b.seeders.cmp(&a.seeders));
            } else {
                list.sort_by(|a, b| a.seeders.cmp(&b.seeders));
            }
        }
        SortBy::Source => {
            // The pinned source moves to the top; everything else keeps the
            // order it had, so the list never looks alphabetically sorted.
            if let Some(pinned) = sort.source {
                list.sort_by_key(|result| source_tag(&result.source) != pinned);
            }
        }
    }
}

pub fn width_of(window: &Window) -> f32 {
    window.bounds().size.width.as_f32()
}

/// The path to reveal: the item itself when it exists on disk, otherwise the
/// folder that holds it.
pub fn item_target(item: &Item) -> PathBuf {
    if item.dir.trim().is_empty() {
        return PathBuf::new();
    }
    let path = Path::new(&item.dir).join(&item.name);
    if path.exists() {
        path
    } else {
        PathBuf::from(&item.dir)
    }
}

pub fn reveal_item(item: &Item) {
    reveal_path(&item_target(item));
}

/// Open the file manager with the file (or folder) selected, rather than
/// launching it. Falls back to a plain folder open when no file manager
/// supports the freedesktop interface.
pub fn reveal_path(target: &Path) {
    let uri = file_uri(target);
    let shown = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("array:string:{uri}"),
            "string:",
        ])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if shown {
        return;
    }
    if std::process::Command::new("nautilus")
        .arg("--select")
        .arg(target)
        .spawn()
        .is_ok()
    {
        return;
    }
    let folder = target.parent().unwrap_or(target);
    let _ = std::process::Command::new("xdg-open").arg(folder).spawn();
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                uri.push(byte as char);
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

pub fn theme_of(window: &Window, app: &App) -> Theme {
    app.theme(window)
}

pub fn appearance_is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme(window);
        let width = width_of(window);

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.canvas)
            .text_color(theme.text)
            .font_family(UI_FONT)
            .key_context("Nori")
            .on_action(cx.listener(|this, _: &Escape, window, cx| this.action_escape(window, cx)))
            .on_action(
                cx.listener(|this, _: &FocusSearch, window, cx| {
                    this.action_focus_search(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &PasteMagnet, window, cx| {
                    this.action_paste_magnet(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &SearchAgain, window, cx| {
                    this.action_search_again(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.action_open_settings(window, cx)
            }))
            .on_action(cx.listener(|this, _: &DownloadPicked, window, cx| {
                this.action_download_picked(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &CopyPicked, window, cx| this.action_copy_picked(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CycleSort, window, cx| this.action_cycle_sort(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleFilter, window, cx| {
                this.action_toggle_filter(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleHideDead, window, cx| {
                this.action_toggle_hide_dead(window, cx)
            }))
            .on_action(cx.listener(|this, _: &PauseDownload, window, cx| {
                this.action_pause_download(window, cx)
            }))
            .on_action(cx.listener(|this, _: &RemoveDownload, window, cx| {
                this.action_remove_download(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &OpenFolder, window, cx| this.action_open_folder(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PauseSeed, window, cx| this.action_pause_seed(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &RemoveSeed, window, cx| this.action_remove_seed(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectNext, window, cx| this.action_select_next(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectPrev, window, cx| this.action_select_prev(window, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectAllResults, window, cx| {
                this.action_select_all_results(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &OpenPicked, window, cx| this.action_open_picked(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Submit, window, cx| this.action_submit(window, cx)))
            .on_action(cx.listener(|this, _: &Quit, _, cx| {
                let _ = this;
                cx.quit();
            }))
            .child(self.header(&theme, width, window, cx))
            .child(self.body(&theme, width, window, cx));

        if self.help_open {
            root = root.child(views::help::modal(self, &theme, cx));
        }
        if self.batch_prompt.is_some() {
            root = root.child(views::batch::modal(self, &theme, cx));
        }
        if self.toast.is_some() {
            root = root.child(self.toast_layer(&theme));
        }
        if self.sort_open {
            if let Some(at) = self.sort_menu_at {
                // The menu lives in a plain overlay rather than an anchored
                // layer: anchored children painted above the list still let the
                // click through to the row underneath.
                let height = window.bounds().size.height.as_f32();
                let left = (at.x.as_f32() - SORT_MENU_WIDTH - 2.).max(8.);
                let top = (at.y.as_f32() + 13.).min((height - SORT_MENU_HEIGHT - 8.).max(8.));
                root = root.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .id("sort-layer")
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .id("sort-dismiss")
                                .occlude()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.close_sort_menu();
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(left))
                                .top(px(top))
                                .id("sort-wrap")
                                .occlude()
                                .on_click(cx.listener(|_, _, _, _| {}))
                                .child(self.sort_menu(&theme, cx)),
                        ),
                );
            }
        }
        root
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.manager.close();
    }
}

// Keep the search result title style and the monospace hash together with the
// rest of the type ramp so views stay free of magic numbers.
pub fn result_title(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .font_family(UI_FONT)
        .text_size(px(14.))
        .text_color(theme.text)
        .line_height(px(19.))
        .truncate()
        .child(text.into())
}

pub fn result_meta(parts: &[(SharedString, Hsla)], theme: &Theme) -> Div {
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .font_family(UI_FONT)
        .text_size(px(11.5))
        .line_height(px(16.))
        .whitespace_nowrap()
        .overflow_hidden();
    for (ix, (text, color)) in parts.iter().enumerate() {
        if ix > 0 {
            row = row.child(div().text_color(theme.text_ghost).child("·"));
        }
        row = row.child(div().text_color(*color).child(text.clone()));
    }
    row
}

pub fn hash_text(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .font_family(MONO_FONT)
        .text_size(px(10.5))
        .text_color(theme.text_tertiary)
        .line_height(px(15.))
        .truncate()
        .child(text.into())
}

pub fn row_background(theme: &Theme, selected: bool) -> Hsla {
    if selected {
        theme.row_selected()
    } else {
        gpui::transparent_black()
    }
}

pub fn tint(id: &str, theme: &Theme) -> Hsla {
    source_tint(id, theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_sort_tags_follow_group_and_enabled_sources() {
        let tags = sort_tags_for(Some(Group::Movies), |key| {
            key != "yts" && key != "tpb-movies"
        });
        assert!(!tags.contains(&"YTS"));
        assert!(!tags.contains(&"TPB"));
        assert!(tags.contains(&"1337"));
    }
}

pub fn _keep_imports_used() {
    let _ = config::APP_DIR;
    let _ = hsla;
    let _ = Task::ready(());
}
