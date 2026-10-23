//! The settings window's content, after boards R1 to R4: a home with one
//! large search field and the pages as tiles in four families, and pages
//! with the families on the side. Everything is built again when what it
//! shows changes; a change of setting comes out as a [`Change`] for the
//! configuration file, and shows once the compositor has applied it.

use std::{collections::HashMap, time::Duration};

use hocus_core::Action;
use hocus_shell::{Lang, icons};
use hocus_ui::{
    Accent, AccessNode, Align, Button, ButtonKind, Canvas, Color, Direction, Draw, Event, Icon,
    Input, Insets, Justify, Key, Kind, Length, Look, NodeId, Paint, PixelRect, Point, Property,
    Rect, Size, Style, Surface, TextField, Theme, ThemeName, Tree,
};

use crate::{
    arrange,
    catalogue::{self, Context, Family, Hit, Page, Setting},
    edit::Change,
    words::{self, ShortcutGroup},
};

mod agents;
mod hardware;
mod map;
mod personalise;
mod search;
mod system;
mod work;

use hardware::HardwarePage;
pub use hardware::{BluetoothAction, Listed, Pairing, Secret, WifiAction};
use personalise::WallpaperPage;
use search::SearchPage;
use system::SystemPage;
use work::{Field, Fields};

/// What the window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Page(Page),
}

/// What the user asked for.
#[derive(Debug, Clone)]
pub enum Outcome {
    Change(Box<Change>),
    Close,
    /// Each screen shows its name.
    Identify,
    /// The system's updates are installed, through polkit.
    InstallUpdates,
    /// A calendar account's password, for the keyring.
    CalendarPassword {
        account: String,
        password: String,
    },
    /// The compositor holds its own shortcuts back while keys are
    /// recorded (keyboard-shortcuts-inhibit), or no longer.
    Inhibit(bool),
    /// The system's time zone is set through timedated (polkit asks).
    SetTimeZone(String),
    /// The sound, a radio or the power profile changes, through the
    /// compositor, as quick settings change it.
    Hardware(hocus_api::HardwareChange),
    /// Do not disturb on or off, through the compositor.
    DoNotDisturb(bool),
    /// This application opens this kind of file from now on (the user's
    /// `mimeapps.list`).
    SetDefaultApp {
        kind: String,
        app: String,
    },
    /// This autostart entry starts with the session, or not (the user's
    /// copy of it).
    SetAutostart {
        id: String,
        on: bool,
    },
    /// The account's full name, through AccountsService.
    SetRealName(String),
    /// The account's picture, from this file, through AccountsService.
    SetPicture(String),
    /// An AI provider's key, for the compositor's keyring; empty, it is
    /// forgotten.
    AiKey {
        provider: String,
        key: String,
    },
    /// Every MCP agent stopped, or allowed again, through the compositor.
    StopAgents(bool),
    /// A network is joined, left or forgotten, through NetworkManager.
    Wifi(WifiAction),
    /// A device is paired, connected, disconnected or forgotten, through
    /// BlueZ.
    Bluetooth(BluetoothAction),
    /// The passkey the pairing asked about is the device's, or not.
    PairingAnswer(bool),
    /// The document search's folders looked at again now, through the
    /// compositor.
    SearchAgain,
}

/// What a node does when clicked.
#[derive(Debug, Clone)]
enum Target {
    Home,
    Page(Page),
    Hit(usize),
    Close,
    Set(Change),
    /// The calendar form's account is added.
    AddCalendar,
    /// Several keys at once (the primary screen: on for one, off for the
    /// others).
    SetAll(Vec<Change>),
    Group(Option<ShortcutGroup>),
    ChooseScreen(String),
    List(ModeList),
    /// The layout of these screens is renamed.
    Rename(Vec<String>),
    Identify,
    /// Records new keys for `action`.
    Edit {
        action: Action,
        old: Option<hocus_core::shortcuts::Combo>,
        place: EditPlace,
    },
    ConfirmEdit,
    CancelEdit,
    /// Shows or hides the layouts offered.
    AddLayout,
    /// Shows or hides the new shortcut's form.
    NewShortcut,
    /// The new shortcut's keys are recorded.
    ListenNew,
    /// Asks, or stops asking, to go back to the original shortcuts.
    Reset,
    InstallUpdates,
    /// The wallpaper of this screen shows, or every screen's.
    WallpaperScreen(Option<String>),
    /// What a field of the Work pages holds is written.
    Commit(Field),
    /// The system's time zone is set to this one.
    SetZone(String),
    /// What the system's services hold changes.
    Hardware(hocus_api::HardwareChange),
    DoNotDisturb(bool),
    /// The choices of this kind of file show, or no longer.
    OpenKind(&'static str),
    DefaultApp {
        kind: &'static str,
        app: String,
    },
    Autostart {
        id: String,
        on: bool,
    },
    /// The questions go to another server speaking OpenAI's API.
    OpenAiLike,
    /// The key of this provider is forgotten.
    ForgetKey(String),
    StopAgents(bool),
    Wifi(WifiAction),
    /// The password of this network is asked.
    AskPassword(String),
    CancelPassword,
    /// The network is joined with the password typed.
    JoinTyped,
    Bluetooth(BluetoothAction),
    PairingAnswer(bool),
    /// The world map: the city nearest the pointer gives the zone.
    Map,
    SearchAgain,
}

/// Which of a screen's choices are open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModeList {
    Resolution,
    Rate,
}

/// The Screens page between builds.
#[derive(Debug, Default)]
struct ScreensPage {
    /// The screen whose settings show.
    chosen: Option<String>,
    /// The arrangement's node, and how screens map onto it.
    area: Option<(NodeId, arrange::Mapping)>,
    /// Each screen's tile in the arrangement.
    tiles: Vec<(String, NodeId)>,
    drag: Option<Drag>,
    list: Option<ModeList>,
    /// The layout being renamed, by its screens, and its field.
    renaming: Option<(Vec<String>, Option<NodeId>)>,
}

/// A screen being dragged in the arrangement.
#[derive(Debug, Clone)]
struct Drag {
    name: String,
    /// Where the pointer took it, from its corner.
    grab: Point,
    /// Its corner in the area.
    at: Point,
    /// The arrangement as it was when the drag began.
    mapping: arrange::Mapping,
    moved: bool,
}

/// The calendar form between builds: name, address, user, password, and
/// their fields.
#[derive(Debug, Default)]
struct AccountsPage {
    values: [String; 4],
    fields: [Option<NodeId>; 4],
    /// The address was refused.
    bad_url: bool,
}

impl AccountsPage {
    fn index(&self, id: Option<NodeId>) -> Option<usize> {
        let id = id?;
        self.fields.iter().position(|f| *f == Some(id))
    }
}

/// The keyboard page between builds.
#[derive(Debug, Default)]
struct KeyboardPage {
    /// A shortcut being changed or made.
    editing: Option<Editing>,
    /// The layouts offered to add show.
    adding_layout: bool,
    /// Going back to the original shortcuts is asked.
    resetting: bool,
    /// The new shortcut's form shows, with what it runs.
    new_open: bool,
    new_text: String,
    new_field: Option<NodeId>,
}

/// Where a shortcut being changed shows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EditPlace {
    /// Its row, by the keys it has.
    Row(hocus_core::shortcuts::Combo),
    /// Beside the layouts: the switch combination.
    Layouts,
    /// The new shortcut's form.
    New,
}

#[derive(Debug, Clone)]
struct Editing {
    /// What the keys will run.
    action: Action,
    /// The keys it has now, when it has some.
    old: Option<hocus_core::shortcuts::Combo>,
    place: EditPlace,
    state: Recording,
}

/// A shortcut being changed: nothing is written until it is confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Recording {
    /// Waiting for keys; `refused` when the last had no Super, Ctrl or Alt.
    Listening { refused: bool },
    /// Keys pressed, free or already bound to `owner`.
    Got {
        combo: hocus_core::shortcuts::Combo,
        owner: Option<Action>,
    },
}

const LEFT: &str = "M15 6l-6 6 6 6";
const PLUS: &str = "M12 5v14M5 12h14";

/// `[keyboard] layouts` as `layouts`.
fn layouts_change(layouts: &[String]) -> Change {
    let list: toml_edit::Array = layouts.iter().map(String::as_str).collect();
    Change::new("keyboard", "layouts", list)
}

/// The arrangement's height, logical pixels.
const AREA_HEIGHT: f32 = 300.0;

/// A pencil, for renaming.
const PENCIL: &str = "M4 20h4L19 9l-4-4L4 16zM14 6l4 4";

/// A switch, drawn: a pill with its knob, the accent when on.
struct Switch(bool);

impl Draw for Switch {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let c = &theme.colors;
        let (w, h) = (40.0, 24.0);
        let pill = Rect::new(rect.x + rect.w - w, rect.y + (rect.h - h) / 2.0, w, h);
        let (ground, knob_x) = if self.0 {
            (c.accent, pill.x + w - h + 3.0)
        } else {
            (c.border.over(c.surface), pill.x + 3.0)
        };
        canvas.fill_rect(pill, h / 2.0, ground);
        let knob = if self.0 { c.on_accent } else { c.text };
        canvas.fill_rect(
            Rect::new(knob_x, pill.y + 3.0, h - 6.0, h - 6.0),
            (h - 6.0) / 2.0,
            knob,
        );
    }

    fn size(&self) -> Size {
        Size::new(40.0, 24.0)
    }
}

/// A theme's miniature: its background, a window, the column, the accent.
struct Miniature(Theme);

impl Draw for Miniature {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, _theme: &Theme, _now: Duration) {
        let c = &self.0.colors;
        canvas.fill_rect(rect, 12.0, c.background);
        let window = Rect::new(rect.x + 14.0, rect.y + 16.0, rect.w * 0.55, rect.h * 0.56);
        canvas.shadow(window, 8.0, 12.0, c.shadow);
        canvas.fill_rect(window, 8.0, c.surface);
        canvas.fill_rect(
            Rect::new(window.x + 8.0, window.y + 10.0, window.w * 0.55, 6.0),
            3.0,
            c.accent,
        );
        canvas.fill_rect(
            Rect::new(rect.x + rect.w - 36.0, rect.y + 8.0, 28.0, rect.h - 16.0),
            8.0,
            c.surface,
        );
    }

    fn size(&self) -> Size {
        Size::new(0.0, 110.0)
    }
}

/// An accent's swatch, ringed when chosen.
struct Swatch {
    color: Color,
    chosen: bool,
}

impl Draw for Swatch {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let d = rect.w.min(rect.h);
        let r = Rect::new(
            rect.x + (rect.w - d) / 2.0,
            rect.y + (rect.h - d) / 2.0,
            d,
            d,
        );
        if self.chosen {
            canvas.fill_rect(r, d / 2.0, theme.colors.text);
        }
        let inner = Rect::new(r.x + 3.0, r.y + 3.0, d - 6.0, d - 6.0);
        canvas.fill_rect(inner, inner.w / 2.0, theme.colors.surface);
        let dot = Rect::new(r.x + 6.0, r.y + 6.0, d - 12.0, d - 12.0);
        canvas.fill_rect(dot, dot.w / 2.0, self.color);
    }

    fn size(&self) -> Size {
        Size::new(48.0, 48.0)
    }
}

pub struct SettingsView {
    lang: Lang,
    /// The session's language, for an interface that follows it.
    session_lang: Lang,
    tree: Tree,
    surface: Surface,
    ctx: Context,
    screen: Screen,
    query: String,
    hits: Vec<Hit>,
    /// The result Enter opens.
    chosen: usize,
    /// The setting a search led to, on its page.
    highlight: Option<Setting>,
    group: Option<ShortcutGroup>,
    targets: HashMap<NodeId, Target>,
    /// The search field, and the field that had the keyboard.
    field: Option<NodeId>,
    typing: bool,
    /// The scrolled box and its content, and how far down it is.
    scroller: Option<(NodeId, NodeId)>,
    scroll: f32,
    /// The text size slider, and the size it last asked for.
    text_size: Option<(NodeId, u32)>,
    pointer: Option<Point>,
    screens: ScreensPage,
    keys: KeyboardPage,
    accounts: AccountsPage,
    wallpaper: WallpaperPage,
    /// The Work pages' fields.
    fields: Fields,
    hardware: HardwarePage,
    system: SystemPage,
    search: SearchPage,
    /// Why the system did not take the time zone chosen last.
    zone_refused: Option<String>,
    /// The time zones' places, for the map; none without the database.
    places: std::sync::Arc<Vec<hocus_platform::timedate::Place>>,
    /// The map, where it is.
    map: Option<NodeId>,
    dirty: bool,
    now: Duration,
}

impl std::fmt::Debug for SettingsView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsView")
            .field("screen", &self.screen)
            .field("query", &self.query)
            .finish_non_exhaustive()
    }
}

fn stroked(path: &str, size: f32) -> Icon {
    Icon::stroked(path, size, 2.1).unwrap_or_else(|| unreachable!("icon {path}"))
}

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

fn row(gap: f32) -> Style {
    Style {
        direction: Direction::Row,
        gap,
        align: Align::Center,
        ..Style::default()
    }
}

fn column(gap: f32) -> Style {
    Style {
        gap,
        ..Style::default()
    }
}

fn fill_width(style: Style) -> Style {
    Style {
        width: Length::Fill,
        ..style
    }
}

/// The interface's language: as set, or the session's.
fn lang_of(config: &hocus_core::Config, session: Lang) -> Lang {
    use hocus_core::config::InterfaceLanguage;
    match config.language.interface {
        InterfaceLanguage::Auto => session,
        InterfaceLanguage::En => Lang::En,
        InterfaceLanguage::Fr => Lang::Fr,
    }
}

impl SettingsView {
    pub fn new(theme: Theme, lang: Lang, size: Size, scale: f32, ctx: Context) -> Self {
        Self {
            lang: lang_of(&ctx.config, lang),
            session_lang: lang,
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            ctx,
            screen: Screen::Home,
            query: String::new(),
            hits: Vec::new(),
            chosen: 0,
            highlight: None,
            group: None,
            targets: HashMap::new(),
            field: None,
            typing: true,
            scroller: None,
            scroll: 0.0,
            text_size: None,
            pointer: None,
            screens: ScreensPage::default(),
            keys: KeyboardPage::default(),
            accounts: AccountsPage::default(),
            wallpaper: WallpaperPage::default(),
            fields: Fields::default(),
            hardware: HardwarePage::default(),
            system: SystemPage::default(),
            search: SearchPage::default(),
            zone_refused: None,
            places: Default::default(),
            map: None,
            dirty: true,
            now: Duration::ZERO,
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn set_theme(&mut self, theme: Theme) {
        if *self.tree.theme() != theme {
            self.tree.set_theme(theme);
            self.dirty = true;
        }
    }

    /// What is in effect changed (the file, the screens).
    pub fn set_context(&mut self, mut ctx: Context) {
        // The compositor does not say the system's zone: timedated does.
        if ctx.zone == catalogue::SystemZone::Unknown {
            ctx.zone = self.ctx.zone.clone();
        }
        // What was typed in the wallpaper's fields is in the file now.
        if ctx.config.wallpaper != self.ctx.config.wallpaper {
            self.wallpaper.leave();
        }
        if ctx.config != self.ctx.config {
            self.fields.leave();
            self.system.bad_address = false;
            self.search.leave();
        }
        // The settings read these themselves, not the compositor.
        ctx.apps = std::mem::take(&mut self.ctx.apps);
        ctx.account = std::mem::take(&mut self.ctx.account);
        // OpenAI's API on a server whose address is now written.
        if ctx.config.ai.provider != "openai" || !ctx.config.ai.url.is_empty() {
            self.system.openai_like = false;
        }
        self.ctx = ctx;
        // A language chosen here is spoken at once.
        self.lang = lang_of(&self.ctx.config, self.session_lang);
        if !self.query.is_empty() {
            self.hits = catalogue::search(&self.query, &self.ctx, self.lang);
        }
        self.dirty = true;
    }

    /// What timedated says the system's zone is.
    pub fn set_zone(&mut self, zone: catalogue::SystemZone) {
        if zone != self.ctx.zone {
            // Changed: a refusal before no longer holds.
            self.zone_refused = None;
            self.ctx.zone = zone;
            if !self.query.is_empty() {
                self.hits = catalogue::search(&self.query, &self.ctx, self.lang);
            }
            self.dirty = true;
        }
    }

    /// The system took the zone asked for, or refused it, and why.
    pub fn zone_set(&mut self, result: Result<String, String>) {
        match result {
            Ok(zone) => {
                self.zone_refused = None;
                self.set_zone(catalogue::SystemZone::Known(zone));
            }
            Err(why) => self.zone_refused = Some(why),
        }
        self.dirty = true;
    }

    /// The time zones' places, as the database says, for the map.
    pub fn set_places(&mut self, places: Vec<hocus_platform::timedate::Place>) {
        if places != *self.places {
            self.places = std::sync::Arc::new(places);
            self.dirty = true;
        }
    }

    pub fn context(&self) -> &Context {
        &self.ctx
    }

    /// The language it speaks.
    pub fn lang(&self) -> Lang {
        self.lang
    }

    pub fn resize(&mut self, size: Size, scale: f32) {
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
            self.dirty = true;
        }
    }

    pub fn size(&self) -> Size {
        self.tree.size()
    }

    /// Shows a page, or the home; a deep link.
    pub fn open(&mut self, page: Option<Page>) {
        self.query.clear();
        self.hits.clear();
        self.highlight = None;
        self.go(page.map_or(Screen::Home, Screen::Page));
    }

    fn go(&mut self, screen: Screen) {
        if screen != self.screen {
            self.scroll = 0.0;
            self.group = None;
            self.wallpaper.leave();
            self.fields.leave();
            self.system.open = None;
            self.system.openai_like = false;
            self.system.bad_address = false;
            self.hardware.leave();
            self.search.leave();
        }
        self.screen = screen;
        self.typing = screen == Screen::Home;
        self.dirty = true;
    }

    fn open_hit(&mut self, i: usize) {
        let Some(hit) = self.hits.get(i).cloned() else {
            return;
        };
        if !hit.page.available() {
            return;
        }
        self.query.clear();
        self.hits.clear();
        self.highlight = hit.setting;
        if let Some(Setting::Shortcut(_)) = &self.highlight {
            self.group = None;
        }
        self.go(Screen::Page(hit.page));
        self.typing = false;
    }

    pub fn pointer_move(&mut self, at: Point) {
        self.pointer = Some(at);
        if let Some(drag) = &mut self.screens.drag {
            let area = self
                .screens
                .area
                .and_then(|(id, _)| self.tree.rect(id))
                .unwrap_or_default();
            let to = Point::new(at.x - area.x - drag.grab.x, at.y - area.y - drag.grab.y);
            if (to.x - drag.at.x).abs() + (to.y - drag.at.y).abs() > 3.0 || drag.moved {
                drag.moved = true;
                drag.at = to;
                self.dirty = true;
            }
            return;
        }
        self.tree.input(Input::PointerMove(at));
    }

    pub fn pointer_leave(&mut self) {
        self.pointer = None;
        self.tree.input(Input::PointerLeave);
    }

    pub fn pointer_button(&mut self, pressed: bool) -> Vec<Outcome> {
        // Screens are dragged in the arrangement, or chosen by a click.
        if self.screen == Screen::Page(Page::Screens) {
            if pressed && let Some(drag) = self.drag_start() {
                self.screens.drag = Some(drag);
                return Vec::new();
            }
            if !pressed && let Some(drag) = self.screens.drag.take() {
                self.dirty = true;
                if drag.moved {
                    return self.drop_screen(&drag).into_iter().collect();
                }
                self.screens.chosen = Some(drag.name);
                self.screens.list = None;
                return Vec::new();
            }
        }
        let events = self.tree.input(Input::PointerButton { pressed });
        self.typing = self.tree.focused() == self.field && self.field.is_some();
        self.handle(events)
    }

    /// The wheel, logical pixels, down positive.
    pub fn scroll(&mut self, dy: f32) {
        let Some((viewport, content)) = self.scroller else {
            return;
        };
        let (Some(view), Some(inner)) = (self.tree.rect(viewport), self.tree.rect(content)) else {
            return;
        };
        let room = (inner.h - view.h).max(0.0);
        let scroll = (self.scroll + dy).clamp(0.0, room);
        if scroll != self.scroll {
            self.scroll = scroll;
            self.tree.jump(content, Property::OffsetY, -scroll);
        }
    }

    pub fn key(&mut self, key: Key) -> Vec<Outcome> {
        self.catch_up();
        // A network's password: Enter joins, Escape forgets it.
        if self.hardware.password_field(self.tree.focused()) {
            match key {
                Key::Enter => return self.join_typed().into_iter().collect(),
                Key::Escape => {
                    self.hardware.password = None;
                    self.tree.set_focus(None);
                    self.dirty = true;
                    return Vec::new();
                }
                _ => {}
            }
        }
        // A path or colour typed: Enter writes it, Escape forgets it.
        if let Some(entry) = self.wallpaper.entry_of(self.tree.focused()) {
            match key {
                Key::Enter => return self.commit(entry),
                Key::Escape => {
                    self.wallpaper.forget(entry);
                    self.tree.set_focus(None);
                    self.dirty = true;
                    return Vec::new();
                }
                _ => {}
            }
        }
        // A Work page's field: Enter writes it, Escape forgets it.
        if let Some(field) = self.fields.field_of(self.tree.focused()) {
            match key {
                Key::Enter => return self.commit_field(field),
                Key::Escape => {
                    self.fields.forget(&field);
                    self.tree.set_focus(None);
                    self.dirty = true;
                    return Vec::new();
                }
                _ => {}
            }
        }
        // Enter in the calendar form: the next field, or Add from the last.
        if key == Key::Enter
            && let Some(i) = self.accounts.index(self.tree.focused())
        {
            if i + 1 < self.accounts.fields.len() {
                self.tree.set_focus(self.accounts.fields[i + 1]);
                return Vec::new();
            }
            return self.add_calendar();
        }
        if key == Key::Escape && self.keys.editing.is_some() {
            return self.cancel_edit();
        }
        if key == Key::Escape
            && (self.keys.adding_layout || self.keys.resetting || self.keys.new_open)
        {
            self.keys.adding_layout = false;
            self.keys.resetting = false;
            self.keys.new_open = false;
            self.dirty = true;
            return Vec::new();
        }
        if key == Key::Enter
            && self.keys.new_field.is_some()
            && self.tree.focused() == self.keys.new_field
        {
            return self.listen_new();
        }
        let results = self.screen == Screen::Home && !self.hits.is_empty();
        if let Some((outputs, Some(field))) = self.screens.renaming.clone() {
            match key {
                Key::Escape => {
                    self.screens.renaming = None;
                    self.dirty = true;
                    return Vec::new();
                }
                Key::Enter => {
                    self.screens.renaming = None;
                    self.dirty = true;
                    let name = self
                        .tree
                        .field_value(field)
                        .unwrap_or_default()
                        .trim()
                        .to_owned();
                    if name.is_empty() {
                        return Vec::new();
                    }
                    return vec![Outcome::Change(Box::new(Change::layout(
                        &outputs,
                        "name",
                        name.as_str(),
                    )))];
                }
                _ => {}
            }
        }
        match key {
            Key::Escape if !self.query.is_empty() => {
                self.query.clear();
                self.hits.clear();
                self.dirty = true;
                Vec::new()
            }
            Key::Escape if self.screen != Screen::Home => {
                self.highlight = None;
                self.go(Screen::Home);
                Vec::new()
            }
            Key::Escape => vec![Outcome::Close],
            Key::Down if results => {
                self.chosen = (self.chosen + 1).min(self.hits.len() - 1);
                self.dirty = true;
                Vec::new()
            }
            Key::Up if results => {
                self.chosen = self.chosen.saturating_sub(1);
                self.dirty = true;
                Vec::new()
            }
            Key::Enter if results && self.tree.focused() == self.field => {
                self.open_hit(self.chosen);
                Vec::new()
            }
            key => {
                let events = self.tree.input(Input::Key(key));
                self.handle(events)
            }
        }
    }

    /// Built again now if a click changed what shows: keys typed at once
    /// after it go to the field it brought (a network's password).
    fn catch_up(&mut self) {
        if self.dirty {
            self.build();
            self.dirty = false;
        }
    }

    /// Typed text goes to the search field.
    pub fn text(&mut self, text: &str) -> Vec<Outcome> {
        self.catch_up();
        // The calendar form's or the wallpaper's field that has the
        // keyboard keeps it.
        if self.accounts.index(self.tree.focused()).is_some()
            || self.wallpaper.entry_of(self.tree.focused()).is_some()
            || self.fields.field_of(self.tree.focused()).is_some()
            || self.hardware.password_field(self.tree.focused())
        {
            let events = self.tree.text_input(text);
            return self.handle(events);
        }
        let renaming = self.screens.renaming.as_ref().and_then(|(_, f)| *f);
        if let Some(field) = self.keys.new_field
            && self.keys.editing.is_none()
        {
            if self.tree.focused() != Some(field) {
                self.tree.set_focus(Some(field));
            }
            let events = self.tree.text_input(text);
            self.keys.new_text = self.tree.field_value(field).unwrap_or_default().to_owned();
            return self.handle(events);
        }
        if renaming.is_some() && self.tree.focused() != renaming {
            self.tree.set_focus(renaming);
        } else if renaming.is_none() && self.tree.focused().is_none_or(|f| Some(f) != self.field) {
            self.tree.set_focus(self.field);
        }
        let events = self.tree.text_input(text);
        self.handle(events)
    }

    fn handle(&mut self, events: Vec<Event>) -> Vec<Outcome> {
        let mut out = Vec::new();
        for event in events {
            match event {
                Event::Changed(id) if Some(id) == self.field => {
                    let text = self.tree.field_value(id).unwrap_or_default().to_owned();
                    if text != self.query {
                        self.query = text;
                        self.hits = catalogue::search(&self.query, &self.ctx, self.lang);
                        self.chosen = 0;
                        self.typing = true;
                        self.screen = Screen::Home;
                        self.dirty = true;
                    }
                }
                Event::Changed(id) if self.text_size.is_some_and(|(s, _)| s == id) => {
                    let value = self.tree.slider_value(id).unwrap_or(0.0);
                    let percent = (80.0 + value * 70.0) as u32 / 5 * 5;
                    if let Some((_, last)) = &mut self.text_size
                        && *last != percent
                    {
                        *last = percent;
                        out.push(Outcome::Change(Box::new(Change::new(
                            "appearance",
                            "text_size",
                            i64::from(percent),
                        ))));
                    }
                }
                Event::Changed(id) if self.hardware.volume.is_some_and(|(s, _)| s == id) => {
                    let value = self.tree.slider_value(id).unwrap_or(0.0);
                    out.extend(self.volume_moved(value));
                }
                Event::Changed(id) if self.hardware.password_field(Some(id)) => {
                    let typed = self.tree.field_value(id).unwrap_or_default().to_owned();
                    // The Join button comes alive with the first letter.
                    let was_empty = self
                        .hardware
                        .password
                        .as_ref()
                        .is_some_and(|(_, _, t)| t.is_empty());
                    if let Some((_, _, t)) = &mut self.hardware.password {
                        *t = typed;
                    }
                    if was_empty {
                        self.dirty = true;
                    }
                }
                Event::Changed(id) if self.accounts.index(Some(id)).is_some() => {
                    if let Some(i) = self.accounts.index(Some(id)) {
                        self.accounts.values[i] =
                            self.tree.field_value(id).unwrap_or_default().to_owned();
                    }
                }
                Event::Changed(id) if self.wallpaper.entry_of(Some(id)).is_some() => {
                    if let Some(entry) = self.wallpaper.entry_of(Some(id)) {
                        let typed = self.tree.field_value(id).unwrap_or_default().to_owned();
                        self.wallpaper.type_into(entry, typed);
                    }
                }
                Event::Changed(id) if self.fields.field_of(Some(id)).is_some() => {
                    if let Some(field) = self.fields.field_of(Some(id)) {
                        let typed = self.tree.field_value(id).unwrap_or_default().to_owned();
                        self.fields.type_into(field, typed);
                    }
                }
                // A click in the form's fields: Enter is the key's to see.
                Event::Activated(id)
                    if self.accounts.index(Some(id)).is_some()
                        || self.wallpaper.entry_of(Some(id)).is_some()
                        || self.fields.field_of(Some(id)).is_some()
                        || self.hardware.password_field(Some(id)) => {}
                Event::Activated(id) => match self.targets.get(&id).cloned() {
                    Some(Target::Home) => {
                        self.highlight = None;
                        self.go(Screen::Home);
                    }
                    Some(Target::Page(page)) => {
                        self.highlight = None;
                        self.go(Screen::Page(page));
                    }
                    Some(Target::Hit(i)) => self.open_hit(i),
                    Some(Target::Close) => out.push(Outcome::Close),
                    Some(Target::Set(change)) => {
                        self.keys.adding_layout = false;
                        self.keys.resetting = false;
                        out.push(Outcome::Change(Box::new(change)));
                    }
                    Some(Target::Edit { action, old, place }) => {
                        out.extend(self.cancel_edit());
                        self.keys.editing = Some(Editing {
                            action,
                            old,
                            place,
                            state: Recording::Listening { refused: false },
                        });
                        self.dirty = true;
                        out.push(Outcome::Inhibit(true));
                    }
                    Some(Target::ConfirmEdit) => out.extend(self.confirm_edit()),
                    Some(Target::CancelEdit) => out.extend(self.cancel_edit()),
                    Some(Target::AddLayout) => {
                        self.keys.adding_layout = !self.keys.adding_layout;
                        self.dirty = true;
                    }
                    Some(Target::NewShortcut) => {
                        if self
                            .keys
                            .editing
                            .as_ref()
                            .is_some_and(|e| e.place == EditPlace::New)
                        {
                            out.extend(self.cancel_edit());
                        }
                        self.keys.new_open = !self.keys.new_open;
                        self.keys.new_text.clear();
                        self.dirty = true;
                    }
                    Some(Target::ListenNew) => out.extend(self.listen_new()),
                    Some(Target::InstallUpdates) => out.push(Outcome::InstallUpdates),
                    Some(Target::AddCalendar) => out.extend(self.add_calendar()),
                    Some(Target::Reset) => {
                        self.keys.resetting = !self.keys.resetting;
                        self.dirty = true;
                    }
                    Some(Target::SetAll(changes)) => {
                        out.extend(changes.into_iter().map(|c| Outcome::Change(Box::new(c))));
                    }
                    Some(Target::ChooseScreen(name)) => {
                        self.screens.chosen = Some(name);
                        self.screens.list = None;
                        self.dirty = true;
                    }
                    Some(Target::List(list)) => {
                        self.screens.list = (self.screens.list != Some(list)).then_some(list);
                        self.dirty = true;
                    }
                    Some(Target::Rename(outputs)) => {
                        self.screens.renaming = Some((outputs, None));
                        self.typing = false;
                        self.dirty = true;
                    }
                    Some(Target::Identify) => out.push(Outcome::Identify),
                    Some(Target::Commit(field)) => out.extend(self.commit_field(field)),
                    Some(Target::Hardware(change)) => out.push(Outcome::Hardware(change)),
                    Some(Target::DoNotDisturb(on)) => out.push(Outcome::DoNotDisturb(on)),
                    Some(Target::OpenKind(kind)) => {
                        self.system.open = (self.system.open != Some(kind)).then_some(kind);
                        self.dirty = true;
                    }
                    Some(Target::DefaultApp { kind, app }) => {
                        self.system.open = None;
                        self.dirty = true;
                        out.push(Outcome::SetDefaultApp {
                            kind: kind.to_owned(),
                            app,
                        });
                    }
                    Some(Target::Autostart { id, on }) => {
                        out.push(Outcome::SetAutostart { id, on });
                    }
                    Some(Target::OpenAiLike) => out.extend(self.choose_openai_like()),
                    Some(Target::ForgetKey(provider)) => out.push(Outcome::AiKey {
                        provider,
                        key: String::new(),
                    }),
                    Some(Target::StopAgents(stop)) => out.push(Outcome::StopAgents(stop)),
                    Some(Target::Wifi(action)) => out.push(self.ask_network(action)),
                    Some(Target::AskPassword(ssid)) => {
                        self.hardware.password = Some((ssid, None, String::new()));
                        self.hardware.network_failed = None;
                        self.dirty = true;
                    }
                    Some(Target::CancelPassword) => {
                        self.hardware.password = None;
                        self.dirty = true;
                    }
                    Some(Target::JoinTyped) => out.extend(self.join_typed()),
                    Some(Target::Bluetooth(action)) => out.push(self.ask_device(action)),
                    Some(Target::PairingAnswer(yes)) => {
                        self.hardware.pairing = None;
                        self.dirty = true;
                        out.push(Outcome::PairingAnswer(yes));
                    }
                    Some(Target::Map) => out.extend(self.map_clicked()),
                    Some(Target::SearchAgain) => out.push(Outcome::SearchAgain),
                    Some(Target::SetZone(zone)) => {
                        self.zone_refused = None;
                        self.dirty = true;
                        out.push(Outcome::SetTimeZone(zone));
                    }
                    Some(Target::WallpaperScreen(screen)) => {
                        self.wallpaper.show(screen);
                        self.dirty = true;
                    }
                    Some(Target::Group(group)) => {
                        self.group = group;
                        self.dirty = true;
                    }
                    None if Some(id) == self.field && !self.hits.is_empty() => {
                        self.open_hit(self.chosen);
                    }
                    None => {}
                },
                Event::Changed(_) => {}
            }
        }
        out
    }

    /// Keys are being recorded: the window gives them as combinations.
    pub fn recording(&self) -> bool {
        matches!(
            self.keys.editing,
            Some(Editing {
                state: Recording::Listening { .. },
                ..
            })
        )
    }

    /// Keys pressed while recording: free, or bound to another action;
    /// nothing is written until confirmed.
    pub fn record(&mut self, combo: hocus_core::shortcuts::Combo) -> Vec<Outcome> {
        use hocus_core::shortcuts::Key as K;
        let shortcuts = self.ctx.config.shortcuts();
        let Some(editing) = &mut self.keys.editing else {
            return Vec::new();
        };
        if !matches!(editing.state, Recording::Listening { .. }) {
            return Vec::new();
        }
        self.dirty = true;
        let m = combo.modifiers;
        // Alone, only keys no one types with: F1…F24, Print, media keys.
        let alone = matches!(&combo.key, K::Named(n)
            if n == "print"
                || n.starts_with("xf86")
                || (n.len() > 1 && n.starts_with('f') && n[1..].parse::<u8>().is_ok()));
        if !(m.logo || m.ctrl || m.alt || alone) {
            editing.state = Recording::Listening { refused: true };
            return Vec::new();
        }
        if editing.old.as_ref() == Some(&combo) {
            self.keys.editing = None;
            return vec![Outcome::Inhibit(false)];
        }
        let owner = shortcuts
            .action(&combo)
            .filter(|a| **a != editing.action)
            .cloned();
        editing.state = Recording::Got { combo, owner };
        vec![Outcome::Inhibit(false)]
    }

    fn cancel_edit(&mut self) -> Vec<Outcome> {
        let was = self.recording();
        if self.keys.editing.take().is_some() {
            self.dirty = true;
        }
        if was {
            vec![Outcome::Inhibit(false)]
        } else {
            Vec::new()
        }
    }

    /// The new keys written: bound to the action, the old ones freed.
    fn confirm_edit(&mut self) -> Vec<Outcome> {
        let Some(editing) = self.keys.editing.take() else {
            return Vec::new();
        };
        let Recording::Got { combo, .. } = &editing.state else {
            self.keys.editing = Some(editing);
            return Vec::new();
        };
        self.dirty = true;
        let mut out = vec![Outcome::Change(Box::new(Change::new(
            "shortcuts",
            combo.to_string(),
            editing.action.to_string(),
        )))];
        if let Some(old) = &editing.old
            && old != combo
        {
            out.push(Outcome::Change(Box::new(Change::new(
                "shortcuts",
                old.to_string(),
                "none",
            ))));
        }
        if editing.place == EditPlace::New {
            self.keys.new_open = false;
            self.keys.new_text.clear();
        }
        out
    }

    /// The new shortcut runs what its field says: an action by its name,
    /// or else a command.
    fn listen_new(&mut self) -> Vec<Outcome> {
        let text = self.keys.new_text.trim().to_owned();
        if text.is_empty() {
            return Vec::new();
        }
        let action = text
            .parse::<Action>()
            .unwrap_or_else(|_| Action::Spawn(text.clone()));
        let mut out = self.cancel_edit();
        self.keys.editing = Some(Editing {
            action,
            old: None,
            place: EditPlace::New,
            state: Recording::Listening { refused: false },
        });
        self.dirty = true;
        out.push(Outcome::Inhibit(true));
        out
    }

    /// Samples animations, builds again what changed, and draws: the
    /// regions drawn, in physical pixels.
    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        self.now = now;
        if self.dirty {
            self.build();
            self.dirty = false;
        }
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    pub fn is_animating(&self) -> bool {
        self.dirty || self.tree.is_animating(self.now)
    }

    /// Premultiplied RGBA, row by row.
    pub fn pixels(&self) -> &[u8] {
        self.surface.pixels()
    }

    pub fn pixel_size(&self) -> (u32, u32) {
        (self.surface.width(), self.surface.height())
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }

    // Building.

    fn build(&mut self) {
        let theme = self.tree.theme().clone();
        let typing_in_form = self.accounts.index(self.tree.focused());
        self.accounts.fields = [None; 4];
        let typing_entry = self.wallpaper.entry_of(self.tree.focused());
        self.wallpaper.fields.clear();
        let typing_field = self.fields.field_of(self.tree.focused());
        self.fields.nodes.clear();
        let root = self.tree.root();
        for child in self.tree.children(root) {
            self.tree.remove(child);
        }
        self.targets.clear();
        self.field = None;
        self.scroller = None;
        self.text_size = None;
        self.hardware.volume = None;
        self.map = None;
        if let Some((_, field, _)) = &mut self.hardware.password {
            *field = None;
        }
        self.tree.set_look(
            root,
            Look {
                background: Some(Paint::Surface),
                ..Look::default()
            },
        );
        match self.screen {
            Screen::Home => {
                self.tree.set_style(
                    root,
                    Style {
                        padding: Insets::symmetric(30.0, 40.0),
                        gap: 26.0,
                        ..Style::default()
                    },
                );
                self.home(&theme, root);
            }
            Screen::Page(page) => {
                self.tree.set_style(
                    root,
                    Style {
                        direction: Direction::Row,
                        ..Style::default()
                    },
                );
                self.nav(&theme, root, page);
                self.page(&theme, root, page);
            }
        }
        // What had the keyboard keeps it; what the pointer was over is
        // hovered again.
        if let Some((_, Some(field))) = &self.screens.renaming {
            self.tree.set_focus(Some(*field));
        } else if let Some((_, Some(field), _)) = &self.hardware.password {
            self.tree.set_focus(Some(*field));
        } else if let Some(i) = typing_in_form.filter(|_| self.accounts.fields[0].is_some()) {
            self.tree.set_focus(self.accounts.fields[i]);
        } else if let Some(field) = typing_entry.and_then(|e| self.wallpaper.node(e)) {
            self.tree.set_focus(Some(field));
        } else if let Some(node) = typing_field.and_then(|f| self.fields.node(&f)) {
            self.tree.set_focus(Some(node));
        } else if self.typing {
            self.tree.set_focus(self.field);
        }
        if let Some(at) = self.pointer {
            self.tree.layout();
            self.tree.input(Input::PointerMove(at));
        }
    }

    fn add(&mut self, parent: NodeId, style: Style, kind: Kind) -> NodeId {
        self.tree.add(parent, style, kind).unwrap_or_else(MISSING)
    }

    fn line(&mut self, parent: NodeId, text: &str, style: hocus_ui::TextStyle) -> NodeId {
        self.add(parent, Style::default(), Kind::text_line(text, style))
    }

    fn close_button(&mut self, theme: &Theme, parent: NodeId) {
        let close = self.add(
            parent,
            Style {
                width: Length::Px(44.0),
                height: Length::Px(44.0),
                ..Style::default()
            },
            Kind::icon_button(
                stroked(icons::CLOSE, 18.0),
                words::CLOSE.get(self.lang),
                ButtonKind::Plain,
                theme,
            ),
        );
        self.targets.insert(close, Target::Close);
    }

    fn search_field(&mut self, theme: &Theme, parent: NodeId, large: bool) {
        let lang = self.lang;
        let height = if large { 52.0 } else { 44.0 };
        let border = if large { Some(Paint::Accent) } else { None };
        let box_ = self.add(
            parent,
            Style {
                width: Length::Fill,
                height: Length::Px(height),
                padding: Insets::symmetric(0.0, 16.0),
                ..row(12.0)
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 14.0,
                border,
                ..Look::default()
            }),
        );
        self.add(
            box_,
            Style::default(),
            Kind::Icon {
                icon: stroked(icons::SEARCH, if large { 20.0 } else { 17.0 }),
                color: Paint::Muted,
                label: None,
            },
        );
        let hint = if large {
            words::SEARCH_HINT
        } else {
            words::SEARCH
        };
        let style = if large {
            theme.text.title.with_weight(500)
        } else {
            theme.text.body
        };
        let field = self.add(
            box_,
            Style {
                width: Length::Fill,
                height: Length::Px(28.0),
                ..Style::default()
            },
            Kind::TextField(Box::new(TextField::new(
                words::SEARCH_NAME.get(lang),
                hint.get(lang),
                style,
            ))),
        );
        self.tree.set_field_value(field, &self.query);
        self.field = Some(field);
        if large {
            self.keycap(theme, box_, "Super+,");
        }
    }

    fn keycap(&mut self, theme: &Theme, parent: NodeId, label: &str) {
        let cap = self.add(
            parent,
            Style {
                padding: Insets::symmetric(4.0, 9.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::SurfacePressed),
                radius: 7.0,
                border: Some(Paint::Border),
                ..Look::default()
            }),
        );
        let mono = hocus_ui::TextStyle {
            typeface: hocus_ui::Typeface::Mono,
            ..theme.text.label.with_weight(500)
        };
        self.line(cap, label, mono);
    }

    fn caption(&mut self, theme: &Theme, parent: NodeId, words: words::Words) {
        let text = words.get(self.lang).to_uppercase();
        self.line(parent, &text, theme.text.caption);
    }

    fn home(&mut self, theme: &Theme, root: NodeId) {
        let lang = self.lang;
        let head = self.add(root, fill_width(row(20.0)), Kind::Box(Look::default()));
        self.line(
            head,
            words::SETTINGS.get(lang),
            theme.text.headline.with_weight(700),
        );
        self.search_field(theme, head, true);
        self.close_button(theme, head);
        let body = self.scrolled(root);
        if self.query.trim().is_empty() {
            self.families(theme, body);
        } else {
            self.results(theme, body);
        }
    }

    /// A box that scrolls with the wheel: what goes in it.
    fn scrolled(&mut self, parent: NodeId) -> NodeId {
        let viewport = self.add(
            parent,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                clip: true,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        let content = self.add(
            viewport,
            fill_width(column(0.0)),
            Kind::Box(Look::default()),
        );
        self.scroller = Some((viewport, content));
        self.tree.jump(content, Property::OffsetY, -self.scroll);
        content
    }

    fn families(&mut self, theme: &Theme, body: NodeId) {
        self.tree.set_style(body, fill_width(column(28.0)));
        // Two families side by side when there is room.
        let per_line = if self.tree.size().w >= 1180.0 { 2 } else { 1 };
        let mut line = None;
        for (i, family) in Family::ALL.into_iter().enumerate() {
            if i % per_line == 0 {
                line = Some(self.add(
                    body,
                    Style {
                        align: Align::Start,
                        ..fill_width(row(40.0))
                    },
                    Kind::Box(Look::default()),
                ));
            }
            let Some(line) = line else { continue };
            let section = self.add(line, fill_width(column(12.0)), Kind::Box(Look::default()));
            self.caption(theme, section, family.label());
            let pages: Vec<Page> = family.pages().collect();
            for three in pages.chunks(3) {
                let tiles = self.add(section, fill_width(row(10.0)), Kind::Box(Look::default()));
                for page in three {
                    self.tile(theme, tiles, *page);
                }
            }
        }
    }

    fn tile(&mut self, theme: &Theme, parent: NodeId, page: Page) {
        let lang = self.lang;
        let available = page.available();
        let card = self.add(
            parent,
            Style {
                width: Length::Fill,
                height: Length::Px(64.0),
                padding: Insets::symmetric(10.0, 14.0),
                ..row(14.0)
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                ..Look::default()
            }),
        );
        if available {
            self.tree.set_clickable(card, page.label().get(lang));
            self.targets.insert(card, Target::Page(page));
        }
        let tint = theme.accents.get(page.family().tint());
        let badge = self.add(
            card,
            Style {
                width: Length::Px(40.0),
                height: Length::Px(40.0),
                justify: Justify::Center,
                align: Align::Center,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Rgba(tint.with_alpha(if available {
                    40
                } else {
                    18
                }))),
                radius: 12.0,
                ..Look::default()
            }),
        );
        let ink = if available {
            tint
        } else {
            tint.with_alpha(110)
        };
        self.add(
            badge,
            Style::default(),
            Kind::Icon {
                icon: stroked(page.icon(), 20.0),
                color: Paint::Rgba(ink),
                label: None,
            },
        );
        let words_box = self.add(card, fill_width(column(1.0)), Kind::Box(Look::default()));
        let name_ink = if available { Paint::Text } else { Paint::Muted };
        self.line(
            words_box,
            page.label().get(lang),
            theme.text.title.with_weight(700).with_color(name_ink),
        );
        let summary = page.summary(&self.ctx, lang);
        self.line(
            words_box,
            &summary,
            theme.text.caption.with_weight(500).with_color(Paint::Muted),
        );
    }

    fn results(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(6.0)));
        if self.hits.is_empty() {
            self.line(
                body,
                words::NO_RESULT.get(lang),
                theme.text.title.with_color(Paint::Muted),
            );
            return;
        }
        for (i, hit) in self.hits.clone().into_iter().enumerate() {
            let chosen = i == self.chosen;
            let line = self.add(
                body,
                Style {
                    height: Length::Px(56.0),
                    padding: Insets::symmetric(8.0, 16.0),
                    ..fill_width(row(14.0))
                },
                Kind::Box(Look {
                    background: Some(if chosen {
                        Paint::SurfacePressed
                    } else {
                        Paint::SurfaceHover
                    }),
                    radius: 14.0,
                    border: chosen.then_some(Paint::Accent),
                    ..Look::default()
                }),
            );
            let ink = if hit.page.available() {
                Paint::Text
            } else {
                Paint::Muted
            };
            if hit.page.available() {
                self.tree.set_clickable(line, &hit.label);
                self.targets.insert(line, Target::Hit(i));
            }
            self.add(
                line,
                Style::default(),
                Kind::Icon {
                    icon: stroked(hit.page.icon(), 20.0),
                    color: Paint::Rgba(theme.accents.get(hit.page.family().tint())),
                    label: None,
                },
            );
            let words_box = self.add(line, fill_width(column(1.0)), Kind::Box(Look::default()));
            self.line(
                words_box,
                &hit.label,
                theme.text.title.with_weight(700).with_color(ink),
            );
            self.line(
                words_box,
                &hit.detail,
                theme.text.caption.with_weight(500).with_color(Paint::Muted),
            );
        }
    }

    fn nav(&mut self, theme: &Theme, root: NodeId, current: Page) {
        let lang = self.lang;
        let nav = self.add(
            root,
            Style {
                width: Length::Px(280.0),
                height: Length::Fill,
                padding: Insets::symmetric(22.0, 14.0),
                ..column(4.0)
            },
            Kind::Box(Look {
                background: Some(Paint::Background),
                ..Look::default()
            }),
        );
        self.search_field(theme, nav, false);
        self.add(
            nav,
            Style {
                height: Length::Px(8.0),
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        for family in Family::ALL {
            let head = self.add(
                nav,
                Style {
                    height: Length::Px(38.0),
                    padding: Insets::symmetric(0.0, 12.0),
                    justify: Justify::SpaceBetween,
                    ..fill_width(row(8.0))
                },
                Kind::Box(Look {
                    radius: 10.0,
                    ..Look::default()
                }),
            );
            self.tree.set_clickable(head, family.label().get(lang));
            self.targets.insert(head, Target::Home);
            let label = family.label().get(lang).to_uppercase();
            self.line(head, &label, theme.text.caption);
            if family != current.family() {
                self.line(head, "6", theme.text.caption.with_weight(600));
                continue;
            }
            for page in family.pages() {
                let here = page == current;
                let item = self.add(
                    nav,
                    Style {
                        height: Length::Px(42.0),
                        padding: Insets::symmetric(0.0, 12.0),
                        ..fill_width(row(12.0))
                    },
                    Kind::Box(Look {
                        background: here.then(|| Paint::Rgba(theme.colors.accent.with_alpha(36))),
                        radius: 12.0,
                        ..Look::default()
                    }),
                );
                let ink = match (here, page.available()) {
                    (true, _) => Paint::Accent,
                    (false, true) => Paint::Text,
                    (false, false) => Paint::Muted,
                };
                if page.available() {
                    self.tree.set_clickable(item, page.label().get(lang));
                    self.tree.set_selected_box(item, here);
                    self.targets.insert(item, Target::Page(page));
                }
                self.add(
                    item,
                    Style::default(),
                    Kind::Icon {
                        icon: stroked(page.icon(), 19.0),
                        color: ink,
                        label: None,
                    },
                );
                let weight = if here { 700 } else { 500 };
                self.line(
                    item,
                    page.label().get(lang),
                    theme.text.body.with_weight(weight).with_color(ink),
                );
            }
        }
    }

    fn page(&mut self, theme: &Theme, root: NodeId, page: Page) {
        let lang = self.lang;
        let main = self.add(
            root,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                padding: Insets::symmetric(28.0, 40.0),
                ..column(24.0)
            },
            Kind::Box(Look::default()),
        );
        let head = self.add(
            main,
            Style {
                justify: Justify::SpaceBetween,
                align: Align::Start,
                ..fill_width(row(12.0))
            },
            Kind::Box(Look::default()),
        );
        let titles = self.add(head, column(4.0), Kind::Box(Look::default()));
        self.line(
            titles,
            page.label().get(lang),
            theme.text.headline.with_weight(700),
        );
        let sub = match page {
            Page::Keyboard => words::SHORTCUTS_NOTE.get(lang).to_owned(),
            Page::Screens => {
                let names: Vec<&str> = self.ctx.screens.iter().map(|s| s.name.as_str()).collect();
                match self.ctx.config.layout_for(&names) {
                    Some(layout) => words::layout_recognised(&layout.name, lang),
                    None => words::SCREENS_NOTE.get(lang).to_owned(),
                }
            }
            _ => words::LIVE.get(lang).to_owned(),
        };
        self.line(titles, &sub, theme.text.body.with_color(Paint::Muted));
        self.close_button(theme, head);
        let body = self.scrolled(main);
        match page {
            Page::Appearance => self.appearance(theme, body),
            Page::Wallpaper => self.wallpaper_page(theme, body),
            Page::ColumnDock => self.column_dock(theme, body),
            Page::Pandas => self.pandas(theme, body),
            Page::Accessibility => self.accessibility(theme, body),
            Page::Desktops => self.desktops(theme, body),
            Page::Windows => self.windows(theme, body),
            Page::Mouse => self.mouse(theme, body),
            Page::Captures => self.captures(theme, body),
            Page::Clipboard => self.clipboard(theme, body),
            Page::Sound => self.sound(theme, body),
            Page::Network => self.network(theme, body),
            Page::Bluetooth => self.bluetooth(theme, body),
            Page::Power => self.power(theme, body),
            Page::Lock => self.lock(theme, body),
            Page::Language => self.language(theme, body),
            Page::Welcome => self.welcome(theme, body),
            Page::About => self.about(theme, body),
            Page::Keyboard => self.keyboard(theme, body),
            Page::Screens => self.screens(theme, body),
            Page::Account => self.account(theme, body),
            Page::Accounts => self.accounts(theme, body),
            Page::Notifications => self.notifications(theme, body),
            Page::Apps => self.apps(theme, body),
            Page::Agents => self.agents(theme, body),
            Page::Search => self.search_page(theme, body),
        }
    }

    /// The calendar form's account goes to the configuration, its password
    /// to the keyring; the form empties.
    fn add_calendar(&mut self) -> Vec<Outcome> {
        let [name, url, user, password] = self.accounts.values.clone().map(|v| v.trim().to_owned());
        if name.is_empty() || url.is_empty() {
            return Vec::new();
        }
        let account = hocus_core::config::CalendarAccount {
            name: name.clone(),
            kind: if user.is_empty() {
                "subscription"
            } else {
                "caldav"
            }
            .into(),
            url: url.clone(),
            user: user.clone(),
        };
        self.dirty = true;
        self.accounts.bad_url = !account.url_is_safe();
        if self.accounts.bad_url {
            return Vec::new();
        }
        let mut out = Vec::new();
        if !password.is_empty() {
            out.push(Outcome::CalendarPassword {
                account: account.id(),
                password,
            });
        }
        out.push(Outcome::Change(Box::new(Change::calendar(
            &url, &user, "name", name,
        ))));
        if account.kind == "subscription" {
            out.push(Outcome::Change(Box::new(Change::calendar(
                &url,
                &user,
                "kind",
                "subscription",
            ))));
        }
        self.accounts = AccountsPage::default();
        out
    }

    /// Board R1's "Accounts and calendar": the calendars with how they
    /// synced and Remove, and a form to add one.
    fn accounts(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let section = self.section(theme, body, words::CALENDARS);
        let accounts = self.ctx.config.calendars.clone();
        if accounts.is_empty() {
            self.line(
                section,
                words::NO_CALENDAR.get(lang),
                theme.text.body.with_color(Paint::Muted),
            );
        }
        for account in &accounts {
            let id = account.id();
            let status = self.ctx.calendars.accounts.iter().find(|s| s.id == id);
            let (said, paint) = match status {
                Some(s) if s.refused => {
                    (words::WRONG_PASSWORD.get(lang).to_owned(), Paint::Waiting)
                }
                Some(s) if s.error.is_some() => {
                    (words::UNREACHABLE.get(lang).to_owned(), Paint::Waiting)
                }
                Some(s) if s.synced => (words::synced(s.events, lang), Paint::Muted),
                _ => (words::SYNCING.get(lang).to_owned(), Paint::Muted),
            };
            let line = self.add(
                section,
                Style {
                    padding: Insets::symmetric(12.0, 18.0),
                    ..fill_width(row(12.0))
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 16.0,
                    ..Look::default()
                }),
            );
            let text = self.add(
                line,
                Style {
                    width: Length::Fill,
                    ..column(2.0)
                },
                Kind::Box(Look::default()),
            );
            self.line(text, &account.name, theme.text.body.with_weight(600));
            self.line(text, &said, theme.text.body.with_color(paint));
            let remove = self.add(
                line,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 16.0),
                    ..Style::default()
                },
                Kind::Button(Button::new(
                    words::REMOVE.get(lang),
                    ButtonKind::Plain,
                    theme,
                )),
            );
            self.targets.insert(
                remove,
                Target::Set(Change::forget_calendar(&account.url, &account.user)),
            );
        }

        let section = self.section(theme, body, words::ADD_CALENDAR);
        let form = self.add(
            section,
            fill_width(column(10.0)),
            Kind::Box(Look::default()),
        );
        let fields = [
            (words::CALENDAR_NAME, words::CALENDAR_NAME_HINT, false),
            (words::CALENDAR_URL, words::CALENDAR_URL_HINT, false),
            (words::CALENDAR_USER, words::CALENDAR_USER, false),
            (words::CALENDAR_PASSWORD, words::CALENDAR_PASSWORD, true),
        ];
        for (i, (name, hint, secret)) in fields.into_iter().enumerate() {
            let boxed = self.add(
                form,
                Style {
                    height: Length::Px(44.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    align: Align::Center,
                    ..fill_width(row(0.0))
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    border: Some(Paint::Border),
                    radius: 12.0,
                    ..Look::default()
                }),
            );
            let mut field = TextField::new(name.get(lang), hint.get(lang), theme.text.body);
            if secret {
                field = field.secret();
            }
            let field = self.add(
                boxed,
                Style {
                    width: Length::Fill,
                    height: Length::Px(28.0),
                    ..Style::default()
                },
                Kind::TextField(Box::new(field)),
            );
            let value = self.accounts.values[i].clone();
            self.tree.set_field_value(field, &value);
            self.accounts.fields[i] = Some(field);
        }
        let (note, paint) = if self.accounts.bad_url {
            (words::CALENDAR_BAD_URL, Paint::Waiting)
        } else {
            (words::CALENDAR_NOTE, Paint::Muted)
        };
        self.add(
            form,
            fill_width(Style::default()),
            Kind::text(note.get(lang), theme.text.body.with_color(paint)),
        );
        let buttons = self.add(form, row(10.0), Kind::Box(Look::default()));
        let add = self.add(
            buttons,
            Style {
                height: Length::Px(40.0),
                padding: Insets::symmetric(0.0, 18.0),
                ..Style::default()
            },
            Kind::Button(Button::new(
                words::ADD.get(lang),
                ButtonKind::Primary,
                theme,
            )),
        );
        self.targets.insert(add, Target::AddCalendar);
    }

    fn section(&mut self, theme: &Theme, parent: NodeId, title: words::Words) -> NodeId {
        let section = self.add(parent, fill_width(column(12.0)), Kind::Box(Look::default()));
        self.caption(theme, section, title);
        section
    }

    /// A box ringed with the accent when a search led to `setting`.
    fn highlighted(&mut self, id: NodeId, setting: &Setting, look: Look) {
        if self.highlight.as_ref() == Some(setting) {
            self.tree.set_look(
                id,
                Look {
                    border: Some(Paint::Accent),
                    background: Some(Paint::Rgba(self.tree.theme().colors.accent.with_alpha(28))),
                    ..look
                },
            );
            self.tree.set_focus(Some(id));
        }
    }

    /// Language and region: the interface's (#51), changed at once; the
    /// clock and its time zone.
    fn language(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let section = self.section(theme, body, words::INTERFACE_LANGUAGE);
        self.language_choices(theme, section);
        self.add(
            section,
            fill_width(Style::default()),
            Kind::text(
                words::INTERFACE_LANGUAGE_NOTE.get(lang),
                theme.text.label.with_weight(500).with_color(Paint::Muted),
            ),
        );
        self.highlighted(section, &Setting::InterfaceLanguage, Look::default());
        self.clock(theme, body);
    }

    /// Like the system, English, Français: the one in effect chosen.
    fn language_choices(&mut self, theme: &Theme, parent: NodeId) {
        use hocus_core::config::InterfaceLanguage;
        let lang = self.lang;
        let current = self.ctx.config.language.interface;
        let line = self.add(parent, row(8.0), Kind::Box(Look::default()));
        for (value, name, id) in [
            (InterfaceLanguage::Auto, words::LIKE_THE_SYSTEM, "auto"),
            (InterfaceLanguage::En, words::ENGLISH, "en"),
            (InterfaceLanguage::Fr, words::FRENCH, "fr"),
        ] {
            let target = Target::Set(Change::new("language", "interface", id));
            self.choice(theme, line, name.get(lang), value == current, target);
        }
    }

    /// A button among others, the one in effect chosen.
    fn choice(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        label: &str,
        chosen: bool,
        target: Target,
    ) -> NodeId {
        let mut button = Button::new(label, ButtonKind::Plain, theme);
        button.selected = chosen;
        let b = self.add(
            parent,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 16.0),
                ..Style::default()
            },
            Kind::Button(button),
        );
        self.targets.insert(b, target);
        b
    }

    /// The panda and two lines beside it: the welcome's and About's head.
    fn hero(
        &mut self,
        theme: &Theme,
        body: NodeId,
        panda: hocus_shell::panda::Panda,
        label: String,
        title: &str,
        line: &str,
    ) {
        let head = self.add(
            body,
            Style {
                align: Align::Center,
                ..row(28.0)
            },
            Kind::Box(Look::default()),
        );
        let frame = self.add(
            head,
            Style {
                width: Length::Px(150.0),
                height: Length::Px(150.0),
                padding: Insets::all(14.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 32.0,
                ..Look::default()
            }),
        );
        self.add(
            frame,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                ..Style::default()
            },
            Kind::Custom {
                draw: Box::new(panda),
                label: Some(label),
            },
        );
        let words_box = self.add(
            head,
            Style {
                width: Length::Fill,
                ..column(8.0)
            },
            Kind::Box(Look::default()),
        );
        let title_style = hocus_ui::TextStyle {
            size: 40.0,
            ..theme.text.display.with_weight(600)
        };
        self.add(
            words_box,
            fill_width(Style::default()),
            Kind::text(title, title_style),
        );
        self.add(
            words_box,
            fill_width(Style::default()),
            Kind::text(line, theme.text.body.with_color(Paint::Muted)),
        );
    }

    /// The page's sections, apart from one another.
    fn sections(&mut self, body: NodeId) -> NodeId {
        self.add(
            body,
            Style {
                padding: Insets {
                    top: 24.0,
                    ..Insets::default()
                },
                ..fill_width(column(22.0))
            },
            Kind::Box(Look::default()),
        )
    }

    /// The first session's (#82): a few choices, all to be changed later,
    /// and where to go next.
    fn welcome(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        // Still: this window draws on the wall clock, and the panda says
        // hello without waving its arm off.
        let mood = hocus_shell::panda::Mood::Wave;
        self.hero(
            theme,
            body,
            hocus_shell::panda::Panda::themed(
                hocus_shell::panda::Character::Apprentice,
                mood,
                theme,
            ),
            mood.label(lang).to_owned(),
            words::WELCOME_TITLE.get(lang),
            words::WELCOME_LINE.get(lang),
        );
        let body = self.sections(body);
        let section = self.section(theme, body, words::INTERFACE_LANGUAGE);
        self.language_choices(theme, section);
        // The keyboard: the layout chosen first, the others kept after it.
        let section = self.section(theme, body, words::KEYBOARD_LAYOUT);
        let line = self.add(section, row(8.0), Kind::Box(Look::default()));
        let layouts = self.ctx.config.keyboard.layouts.clone();
        let first = layouts.first().cloned().unwrap_or_else(|| "us".into());
        for code in ["us", "fr", "gb", "de", "es", "it"] {
            let mut list = vec![code.to_owned()];
            list.extend(layouts.iter().filter(|l| *l != code).cloned());
            let label = code.to_uppercase();
            self.choice(
                theme,
                line,
                &label,
                first == code,
                Target::Set(layouts_change(&list)),
            );
        }
        // The look.
        let a = self.ctx.config.appearance.clone();
        let section = self.section(theme, body, words::THEME);
        let line = self.add(section, row(8.0), Kind::Box(Look::default()));
        for (id, name, _) in words::THEMES {
            let target = Target::Set(Change::new("appearance", "theme", id));
            self.choice(theme, line, name.get(lang), a.theme == id, target);
        }
        let section = self.section(theme, body, words::ACCENT);
        let line = self.add(section, row(8.0), Kind::Box(Look::default()));
        for (id, name) in words::ACCENTS {
            let target = Target::Set(Change::new("appearance", "accent", id));
            let b = self.choice(theme, line, name.get(lang), a.accent == id, target);
            let label = format!("{} {}", words::ACCENT.get(lang), name.get(lang));
            self.tree.set_name(b, &label);
        }
        // Next.
        let section = self.section(theme, body, words::NEXT);
        let line = self.add(section, row(8.0), Kind::Box(Look::default()));
        for (name, target) in [
            (words::WIFI, Target::Page(Page::Network)),
            (Page::Appearance.label(), Target::Page(Page::Appearance)),
            (words::AI, Target::Page(Page::Agents)),
            (words::ALL_SETTINGS, Target::Home),
        ] {
            self.choice(theme, line, name.get(lang), false, target);
        }
        let mut start = Button::new(words::START.get(lang), ButtonKind::Primary, theme);
        start.selected = false;
        let b = self.add(
            body,
            Style {
                height: Length::Px(48.0),
                padding: Insets::symmetric(0.0, 28.0),
                ..Style::default()
            },
            Kind::Button(start),
        );
        self.targets.insert(b, Target::Close);
    }

    /// About: the version, the licences, the credits.
    fn about(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let version = format!("{} {}", words::VERSION.get(lang), env!("CARGO_PKG_VERSION"));
        self.hero(
            theme,
            body,
            hocus_shell::panda::Panda::logo(theme),
            lang.logo().to_owned(),
            "Hocuspanda",
            &version,
        );
        let body = self.sections(body);
        for (title, text) in [
            (words::LICENCES, words::LICENCES_TEXT),
            (words::CREDITS, words::CREDITS_TEXT),
        ] {
            let section = self.section(theme, body, title);
            self.add(
                section,
                fill_width(Style::default()),
                Kind::text(text.get(lang), theme.text.body),
            );
        }
    }

    fn appearance(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let a = self.ctx.config.appearance.clone();

        // Theme.
        let section = self.section(theme, body, words::THEME);
        let grid = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
        for (id, name, hint) in words::THEMES {
            let chosen = a.theme == id;
            let look = Look {
                background: Some(Paint::SurfaceHover),
                radius: 18.0,
                border: chosen.then_some(Paint::Accent),
                ..Look::default()
            };
            let card = self.add(
                grid,
                Style {
                    padding: Insets {
                        top: 8.0,
                        right: 8.0,
                        bottom: 12.0,
                        left: 8.0,
                    },
                    ..fill_width(column(10.0))
                },
                Kind::Box(look),
            );
            self.tree.set_clickable(card, name.get(lang));
            self.tree.set_selected_box(card, chosen);
            self.targets
                .insert(card, Target::Set(Change::new("appearance", "theme", id)));
            let mut preview = Theme::builtin(id.parse().unwrap_or(ThemeName::Dark));
            if let Ok(accent) = a.accent.parse::<Accent>() {
                preview = preview.with_accent(accent);
            }
            self.add(
                card,
                Style {
                    width: Length::Fill,
                    height: Length::Px(96.0),
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Miniature(preview)),
                    label: None,
                },
            );
            let words_box = self.add(
                card,
                Style {
                    padding: Insets::symmetric(0.0, 4.0),
                    ..column(1.0)
                },
                Kind::Box(Look::default()),
            );
            self.line(words_box, name.get(lang), theme.text.title.with_weight(700));
            self.line(
                words_box,
                hint.get(lang),
                theme.text.caption.with_weight(500).with_color(Paint::Muted),
            );
            if chosen {
                self.highlighted(card, &Setting::Theme, look);
            }
        }

        // Accent and text, side by side when there is room.
        let wide = self.tree.size().w >= 1180.0;
        let pair = self.add(
            body,
            Style {
                align: Align::Start,
                direction: if wide {
                    Direction::Row
                } else {
                    Direction::Column
                },
                gap: if wide { 40.0 } else { 26.0 },
                ..fill_width(Style::default())
            },
            Kind::Box(Look::default()),
        );
        let accent = self.section(theme, pair, words::ACCENT);
        let swatches = self.add(accent, row(12.0), Kind::Box(Look::default()));
        // The theme's own accent shows as its colour.
        let own = Theme::builtin(a.theme.parse().unwrap_or(ThemeName::Dark))
            .accents
            .default;
        for (id, name) in words::ACCENTS {
            let chosen = a.accent == id || (a.accent == "theme" && own.name() == id);
            let color = id
                .parse::<Accent>()
                .map_or(theme.colors.accent, |acc| theme.accents.get(acc));
            let swatch = self.add(
                swatches,
                Style {
                    width: Length::Px(48.0),
                    height: Length::Px(48.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    radius: 24.0,
                    ..Look::default()
                }),
            );
            let label = format!("{} {}", words::ACCENT.get(lang), name.get(lang));
            self.tree.set_clickable(swatch, &label);
            self.tree.set_selected_box(swatch, chosen);
            self.targets
                .insert(swatch, Target::Set(Change::new("appearance", "accent", id)));
            self.add(
                swatch,
                Style {
                    width: Length::Fill,
                    height: Length::Fill,
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Swatch { color, chosen }),
                    label: None,
                },
            );
        }
        let mut from_wallpaper =
            Button::new(words::FROM_WALLPAPER.get(lang), ButtonKind::Plain, theme);
        from_wallpaper.selected = a.accent == "wallpaper";
        let b = self.add(
            swatches,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 14.0),
                ..Style::default()
            },
            Kind::Button(from_wallpaper),
        );
        self.targets.insert(
            b,
            Target::Set(Change::new("appearance", "accent", "wallpaper")),
        );
        let note = self.add(
            accent,
            Style::default(),
            Kind::text(
                words::ACCENT_NOTE.get(lang),
                theme.text.label.with_weight(500).with_color(Paint::Muted),
            ),
        );
        let _ = note;
        let accent_look = Look::default();
        self.highlighted(accent, &Setting::Accent, accent_look);

        let text = self.section(theme, pair, words::TEXT);
        let fonts = self.add(text, fill_width(row(10.0)), Kind::Box(Look::default()));
        for w in [words::FONT_UI, words::FONT_MONO] {
            let mut b = Button::new(w.get(lang), ButtonKind::Plain, theme);
            // Choosing the faces comes with the font work of #103's sequel.
            b.enabled = false;
            self.add(
                fonts,
                Style {
                    width: Length::Fill,
                    height: Length::Px(44.0),
                    ..Style::default()
                },
                Kind::Button(b),
            );
        }
        let size_row = self.add(text, fill_width(row(12.0)), Kind::Box(Look::default()));
        self.add(
            size_row,
            Style {
                width: Length::Px(70.0),
                ..Style::default()
            },
            Kind::text_line(words::SIZE.get(lang), theme.text.body),
        );
        let slider = self.add(
            size_row,
            Style {
                width: Length::Fill,
                height: Length::Px(32.0),
                ..Style::default()
            },
            Kind::Slider {
                value: (a.text_size.clamp(80, 150) - 80) as f32 / 70.0,
                step: 5.0 / 70.0,
                name: words::TEXT_SIZE.get(lang).to_owned(),
            },
        );
        self.text_size = Some((slider, a.text_size));
        self.add(
            size_row,
            Style {
                width: Length::Px(56.0),
                ..Style::default()
            },
            Kind::text_line(
                words::percent(a.text_size, lang),
                theme.text.body.with_color(Paint::Muted),
            ),
        );
        self.highlighted(text, &Setting::TextSize, Look::default());

        // Theme per desktop, with #49.
        let per_desktop = self.add(body, fill_width(column(8.0)), Kind::Box(Look::default()));
        self.caption(theme, per_desktop, words::THEME_PER_DESKTOP);
        self.add(
            per_desktop,
            Style::default(),
            Kind::text(
                words::THEME_PER_DESKTOP_NOTE.get(lang),
                theme.text.body.with_color(Paint::Muted),
            ),
        );
        self.highlighted(per_desktop, &Setting::ThemePerDesktop, Look::default());

        // Switches, one under another when there is no room.
        let switches = self.add(
            body,
            Style {
                direction: if wide {
                    Direction::Row
                } else {
                    Direction::Column
                },
                ..fill_width(row(12.0))
            },
            Kind::Box(Look::default()),
        );
        for (setting, key, name, sub, on, works) in [
            (
                Setting::ReducedMotion,
                "reduced_motion",
                words::REDUCED_MOTION,
                words::REDUCED_MOTION_SUB,
                a.reduced_motion,
                true,
            ),
            (
                Setting::HighContrast,
                "high_contrast",
                words::HIGH_CONTRAST,
                words::HIGH_CONTRAST_SUB,
                a.high_contrast,
                true,
            ),
            (
                Setting::Transparency,
                "transparency",
                words::TRANSPARENCY,
                words::TRANSPARENCY_SUB,
                a.transparency,
                true,
            ),
        ] {
            let change = works.then(|| Change::new("appearance", key, !on));
            self.switch_card(theme, switches, (name, sub), on, change, Some(&setting));
        }
    }

    fn keyboard(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(22.0)));
        let config = self.ctx.config.clone();
        let shortcuts = config.shortcuts();
        let editing = self.keys.editing.clone();

        // Layouts, in order: the first is the one at start; each can go, or
        // come before the one ahead of it.
        let layouts = self.add(body, fill_width(row(10.0)), Kind::Box(Look::default()));
        self.caption(theme, layouts, words::LAYOUTS);
        let names: Vec<String> = if config.keyboard.layouts.is_empty() {
            vec!["us".into()]
        } else {
            config.keyboard.layouts.clone()
        };
        for (i, code) in names.iter().enumerate() {
            let name = words::layout_name(code, lang);
            let first = i == 0;
            let chip = self.add(
                layouts,
                Style {
                    height: Length::Px(44.0),
                    padding: Insets {
                        left: 14.0,
                        right: if names.len() > 1 { 6.0 } else { 14.0 },
                        ..Insets::default()
                    },
                    ..row(2.0)
                },
                Kind::Box(Look {
                    background: Some(if first {
                        Paint::Rgba(theme.colors.accent.with_alpha(0x29))
                    } else {
                        Paint::SurfaceHover
                    }),
                    radius: 12.0,
                    border: first.then_some(Paint::Accent),
                    ..Look::default()
                }),
            );
            let style = theme.text.label.with_weight(600);
            self.line(
                chip,
                &name,
                if first {
                    style.with_color(Paint::Accent)
                } else {
                    style
                },
            );
            if i > 0 {
                let mut order = names.clone();
                order.swap(i - 1, i);
                let before =
                    self.small_button(theme, chip, LEFT, &words::layout_before(&name, lang));
                self.targets
                    .insert(before, Target::Set(layouts_change(&order)));
            }
            if names.len() > 1 {
                let mut rest = names.clone();
                rest.remove(i);
                let remove = self.small_button(
                    theme,
                    chip,
                    icons::CLOSE,
                    &words::remove_layout(&name, lang),
                );
                self.targets
                    .insert(remove, Target::Set(layouts_change(&rest)));
            }
        }
        let mut add = Button::new(words::ADD.get(lang), ButtonKind::Chip, theme)
            .with_icon(stroked(PLUS, 16.0))
            .with_name(words::ADD_LAYOUT.get(lang));
        add.selected = self.keys.adding_layout;
        let add = self.add(
            layouts,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 14.0),
                ..Style::default()
            },
            Kind::Button(add),
        );
        self.targets.insert(add, Target::AddLayout);
        if let Some((combo, _)) = shortcuts
            .bindings
            .iter()
            .find(|(_, a)| **a == Action::SwitchLayout)
        {
            self.add(
                layouts,
                fill_width(Style::default()),
                Kind::Box(Look::default()),
            );
            self.line(
                layouts,
                words::SWITCH_LAYOUT.get(lang),
                theme.text.body.with_color(Paint::Muted),
            );
            let here = editing
                .as_ref()
                .is_some_and(|e| e.place == EditPlace::Layouts);
            let keys = self.add(layouts, row(6.0), Kind::Box(Look::default()));
            self.edit_caps(theme, keys, combo, here);
            let name =
                words::change_shortcut(words::SWITCH_LAYOUT.get(lang), &combo.to_string(), lang);
            self.tree.set_clickable(keys, &name);
            self.targets.insert(
                keys,
                Target::Edit {
                    action: Action::SwitchLayout,
                    old: Some(combo.clone()),
                    place: EditPlace::Layouts,
                },
            );
        }
        self.highlighted(layouts, &Setting::Layouts, Look::default());
        if self.keys.adding_layout {
            let offered: Vec<_> = words::LAYOUT_NAMES
                .iter()
                .filter(|(code, _)| !names.iter().any(|n| n == code))
                .collect();
            // Six to a line: the page does not wrap.
            for line in offered.chunks(6) {
                let chips = self.add(body, fill_width(row(6.0)), Kind::Box(Look::default()));
                for (code, name) in line {
                    let b = self.add(
                        chips,
                        Style {
                            height: Length::Px(36.0),
                            padding: Insets::symmetric(0.0, 12.0),
                            ..Style::default()
                        },
                        Kind::Button(Button::new(name.get(lang), ButtonKind::Chip, theme)),
                    );
                    let mut more = names.clone();
                    more.push((*code).to_owned());
                    self.targets.insert(b, Target::Set(layouts_change(&more)));
                }
            }
        }
        if editing
            .as_ref()
            .is_some_and(|e| e.place == EditPlace::Layouts)
        {
            let note = self.add(body, fill_width(row(10.0)), Kind::Box(Look::default()));
            self.edit_note(theme, note, true);
        }

        // Groups.
        let chips = self.add(body, fill_width(row(6.0)), Kind::Box(Look::default()));
        let groups = std::iter::once((None, words::ALL_GROUPS)).chain(
            ShortcutGroup::ALL.into_iter().map(|g| {
                (
                    Some(g),
                    words::Words::new(g.name(Lang::En), g.name(Lang::Fr)),
                )
            }),
        );
        for (group, label) in groups {
            let mut b = Button::new(label.get(lang), ButtonKind::Chip, theme);
            b.selected = self.group == group;
            let id = self.add(
                chips,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(b),
            );
            self.targets.insert(id, Target::Group(group));
        }

        // The table: what each shortcut does, on what, its keys; Change
        // records new ones.
        let table = self.add(
            body,
            Style {
                padding: Insets::symmetric(4.0, 0.0),
                ..fill_width(column(0.0))
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                ..Look::default()
            }),
        );
        let header = self.add(
            table,
            Style {
                height: Length::Px(38.0),
                padding: Insets::symmetric(0.0, 18.0),
                ..fill_width(row(16.0))
            },
            Kind::Box(Look::default()),
        );
        for (w, width) in [
            (words::ACTION, None),
            (words::ACTS_ON, Some(200.0)),
            (words::KEYS, Some(220.0)),
            (words::CHANGE_HEADER, Some(110.0)),
        ] {
            let cell = self.add(
                header,
                Style {
                    width: width.map_or(Length::Fill, Length::Px),
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            );
            self.caption(theme, cell, w);
        }
        let mut rows: Vec<(
            hocus_core::shortcuts::Combo,
            Action,
            String,
            &str,
            ShortcutGroup,
        )> = shortcuts
            .bindings
            .iter()
            .map(|(combo, action)| {
                let (label, group, on) = words::action(action, lang);
                (combo.clone(), action.clone(), label, on, group)
            })
            .filter(|r| self.group.is_none_or(|g| g == r.4))
            .collect();
        rows.sort_by(|a, b| (a.4 as u8, &a.2).cmp(&(b.4 as u8, &b.2)));
        for (combo, action, label, on, _) in rows {
            let here = editing
                .as_ref()
                .is_some_and(|e| e.place == EditPlace::Row(combo.clone()));
            let look = if here {
                Look {
                    background: Some(Paint::Rgba(theme.colors.accent.with_alpha(0x10))),
                    border: Some(Paint::Accent),
                    radius: 12.0,
                    ..Look::default()
                }
            } else {
                Look::default()
            };
            let line = self.add(
                table,
                Style {
                    padding: Insets::symmetric(if here { 10.0 } else { 0.0 }, 18.0),
                    height: if here { Length::Auto } else { Length::Px(50.0) },
                    ..fill_width(row(16.0))
                },
                Kind::Box(look),
            );
            let what = self.add(
                line,
                Style {
                    width: Length::Fill,
                    ..column(6.0)
                },
                Kind::Box(Look::default()),
            );
            self.add(
                what,
                Style::default(),
                Kind::text_line(label.clone(), theme.text.body.with_weight(600)),
            );
            if here {
                let note = self.add(what, row(10.0), Kind::Box(Look::default()));
                self.edit_note(theme, note, false);
            }
            let got = here
                && matches!(
                    self.keys.editing.as_ref().map(|e| &e.state),
                    Some(Recording::Got { .. })
                );
            let scope = self.add(
                line,
                Style {
                    width: Length::Px(200.0),
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            );
            self.line(
                scope,
                on,
                theme.text.label.with_weight(500).with_color(Paint::Muted),
            );
            let keys = self.add(
                line,
                Style {
                    width: Length::Px(220.0),
                    ..row(6.0)
                },
                Kind::Box(Look::default()),
            );
            self.edit_caps(theme, keys, &combo, here);
            // Changing: the confirmation over Cancel.
            let cell = self.add(
                line,
                Style {
                    width: Length::Px(110.0),
                    ..column(6.0)
                },
                Kind::Box(Look::default()),
            );
            if got {
                self.confirm_button(theme, cell);
            }
            let (text, target) = if here {
                (words::CANCEL, Target::CancelEdit)
            } else {
                (
                    words::CHANGE,
                    Target::Edit {
                        action: action.clone(),
                        old: Some(combo.clone()),
                        place: EditPlace::Row(combo.clone()),
                    },
                )
            };
            let button = Button::new(text.get(lang), ButtonKind::Plain, theme).with_name(if here {
                text.get(lang).to_owned()
            } else {
                words::change_shortcut(&label, &combo.to_string(), lang)
            });
            let b = self.add(
                cell,
                Style {
                    width: Length::Fill,
                    height: Length::Px(36.0),
                    ..Style::default()
                },
                Kind::Button(button),
            );
            self.targets.insert(b, target);
            let setting = Setting::Shortcut(combo.to_string());
            if self.highlight.as_ref() == Some(&setting) {
                self.tree.set_clickable(line, &setting_name(&setting));
            }
            self.highlighted(
                line,
                &setting,
                Look {
                    radius: 12.0,
                    ..look
                },
            );
        }

        // A new shortcut, and the original ones back.
        let foot = self.add(body, fill_width(row(10.0)), Kind::Box(Look::default()));
        let mut new = Button::new(words::NEW_SHORTCUT.get(lang), ButtonKind::Plain, theme)
            .with_icon(stroked(PLUS, 16.0));
        new.selected = self.keys.new_open;
        let new = self.add(
            foot,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 16.0),
                ..Style::default()
            },
            Kind::Button(new),
        );
        self.targets.insert(new, Target::NewShortcut);
        self.add(
            foot,
            fill_width(Style::default()),
            Kind::Box(Look::default()),
        );
        let mut reset = Button::new(words::RESET.get(lang), ButtonKind::Ghost, theme);
        reset.enabled = !config.shortcuts.is_empty();
        let reset = self.add(
            foot,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 16.0),
                ..Style::default()
            },
            Kind::Button(reset),
        );
        self.targets.insert(reset, Target::Reset);
        if self.keys.resetting {
            let ask = self.add(body, fill_width(row(10.0)), Kind::Box(Look::default()));
            self.add(
                ask,
                fill_width(Style::default()),
                Kind::Box(Look::default()),
            );
            self.line(
                ask,
                words::RESET_NOTE.get(lang),
                theme.text.label.with_color(Paint::Waiting),
            );
            let back = self.add(
                ask,
                Style {
                    height: Length::Px(36.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(Button::new(
                    words::GO_BACK.get(lang),
                    ButtonKind::Primary,
                    theme,
                )),
            );
            self.targets
                .insert(back, Target::Set(crate::edit::Change::forget("shortcuts")));
            let cancel = self.add(
                ask,
                Style {
                    height: Length::Px(36.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(Button::new(
                    words::CANCEL.get(lang),
                    ButtonKind::Plain,
                    theme,
                )),
            );
            self.targets.insert(cancel, Target::Reset);
        }
        if self.keys.new_open {
            let form = self.add(
                body,
                Style {
                    padding: Insets::all(14.0),
                    ..fill_width(row(12.0))
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 14.0,
                    border: editing
                        .as_ref()
                        .is_some_and(|e| e.place == EditPlace::New)
                        .then_some(Paint::Accent),
                    ..Look::default()
                }),
            );
            let field = self.add(
                form,
                Style {
                    width: Length::Fill,
                    height: Length::Px(28.0),
                    ..Style::default()
                },
                Kind::TextField(Box::new(TextField::new(
                    words::NEW_WHAT.get(lang),
                    words::NEW_WHAT_HINT.get(lang),
                    theme.text.body,
                ))),
            );
            self.tree.set_field_value(field, &self.keys.new_text);
            self.keys.new_field = Some(field);
            match editing.as_ref().filter(|e| e.place == EditPlace::New) {
                Some(e) => {
                    if let Recording::Got { combo, .. } = &e.state {
                        let keys = self.add(form, row(6.0), Kind::Box(Look::default()));
                        self.edit_caps(theme, keys, combo, true);
                    }
                    let note = self.add(form, row(10.0), Kind::Box(Look::default()));
                    self.edit_note(theme, note, true);
                }
                None => {
                    let choose = self.add(
                        form,
                        Style {
                            height: Length::Px(36.0),
                            padding: Insets::symmetric(0.0, 14.0),
                            ..Style::default()
                        },
                        Kind::Button(Button::new(
                            words::CHOOSE_KEYS.get(lang),
                            ButtonKind::Primary,
                            theme,
                        )),
                    );
                    self.targets.insert(choose, Target::ListenNew);
                }
            }
        } else {
            self.keys.new_field = None;
        }
    }

    /// A small icon button inside a chip.
    fn small_button(&mut self, theme: &Theme, parent: NodeId, icon: &str, name: &str) -> NodeId {
        self.add(
            parent,
            Style {
                width: Length::Px(30.0),
                height: Length::Px(30.0),
                ..Style::default()
            },
            Kind::icon_button(stroked(icon, 14.0), name, ButtonKind::Ghost, theme),
        )
    }

    /// The keys of a shortcut as caps; while it is changed, the new ones
    /// once pressed, in the accent.
    fn edit_caps(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        combo: &hocus_core::shortcuts::Combo,
        here: bool,
    ) {
        let got = match self.keys.editing.as_ref().map(|e| &e.state) {
            Some(Recording::Got { combo, .. }) if here => Some(combo.clone()),
            _ => None,
        };
        let shown = got.as_ref().unwrap_or(combo);
        for cap in hocus_shell::actions::caps(shown, self.lang) {
            if got.is_some() {
                self.keycap_new(theme, parent, &cap);
            } else {
                self.keycap(theme, parent, &cap);
            }
        }
    }

    /// Save, or Replace when the keys are taken.
    fn confirm_button(&mut self, theme: &Theme, parent: NodeId) {
        let taken = matches!(
            self.keys.editing.as_ref().map(|e| &e.state),
            Some(Recording::Got { owner: Some(_), .. })
        );
        let text = if taken { words::REPLACE } else { words::SAVE };
        let b = self.add(
            parent,
            Style {
                width: Length::Fill,
                height: Length::Px(36.0),
                padding: Insets::symmetric(0.0, 12.0),
                ..Style::default()
            },
            Kind::Button(Button::new(text.get(self.lang), ButtonKind::Primary, theme)),
        );
        self.targets.insert(b, Target::ConfirmEdit);
    }

    /// What the shortcut being changed waits for; with its buttons when
    /// `buttons` (a row has them in its last column).
    fn edit_note(&mut self, theme: &Theme, parent: NodeId, buttons: bool) {
        let lang = self.lang;
        let Some(editing) = self.keys.editing.clone() else {
            return;
        };
        let accent = theme.text.label.with_weight(500).with_color(Paint::Accent);
        let waiting = theme.text.label.with_weight(500).with_color(Paint::Waiting);
        let got = match &editing.state {
            Recording::Listening { refused: false } => {
                self.line(parent, words::PRESS_KEYS.get(lang), accent);
                false
            }
            Recording::Listening { refused: true } => {
                self.line(parent, words::NEEDS_MODIFIER.get(lang), waiting);
                false
            }
            Recording::Got { combo, owner: None } => {
                let keys = hocus_shell::actions::caps(combo, lang).join("+");
                self.line(parent, &words::new_keys(&keys, lang), accent);
                true
            }
            Recording::Got {
                owner: Some(owner), ..
            } => {
                let (name, _, _) = words::action(owner, lang);
                self.line(parent, &words::taken_by(&name, lang), waiting);
                true
            }
        };
        if buttons && got {
            let room = self.add(parent, row(0.0), Kind::Box(Look::default()));
            self.tree.set_style(
                room,
                Style {
                    width: Length::Px(130.0),
                    ..row(0.0)
                },
            );
            self.confirm_button(theme, room);
        }
        if buttons {
            let b = self.add(
                parent,
                Style {
                    height: Length::Px(32.0),
                    padding: Insets::symmetric(0.0, 12.0),
                    ..Style::default()
                },
                Kind::Button(Button::new(
                    words::CANCEL.get(lang),
                    ButtonKind::Plain,
                    theme,
                )),
            );
            self.targets.insert(b, Target::CancelEdit);
        }
    }

    /// A key cap of new keys: in the accent.
    fn keycap_new(&mut self, theme: &Theme, parent: NodeId, label: &str) {
        let cap = self.add(
            parent,
            Style {
                padding: Insets::symmetric(4.0, 9.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Rgba(theme.colors.accent.with_alpha(0x29))),
                radius: 7.0,
                border: Some(Paint::Accent),
                ..Look::default()
            }),
        );
        let mono = hocus_ui::TextStyle {
            typeface: hocus_ui::Typeface::Mono,
            ..theme.text.label.with_weight(500).with_color(Paint::Accent)
        };
        self.line(cap, label, mono);
    }

    /// A drag begins on the screen under the pointer, if any.
    fn drag_start(&self) -> Option<Drag> {
        let at = self.pointer?;
        let (area_id, mapping) = self.screens.area?;
        let area = self.tree.rect(area_id)?;
        let (name, rect) = self
            .screens
            .tiles
            .iter()
            .filter_map(|(n, id)| Some((n, self.tree.rect(*id)?)))
            .find(|(_, r)| r.contains(at))?;
        Some(Drag {
            name: name.clone(),
            grab: Point::new(at.x - rect.x, at.y - rect.y),
            at: Point::new(rect.x - area.x, rect.y - area.y),
            mapping,
            moved: false,
        })
    }

    /// Where a dragged screen lands, written as the layout of the screens
    /// connected.
    fn drop_screen(&self, drag: &Drag) -> Option<Outcome> {
        let screens = &self.ctx.screens;
        let moving = screens.iter().find(|s| s.name == drag.name)?;
        let (x, y) = drag.mapping.logical(drag.at);
        let others: Vec<arrange::Placed> = screens
            .iter()
            .filter(|s| s.name != drag.name)
            .map(placed)
            .collect();
        let landed = arrange::snap(
            arrange::Placed {
                x,
                y,
                ..placed(moving)
            },
            &others,
        );
        let mut positions: Vec<(String, (i32, i32))> = screens
            .iter()
            .map(|s| {
                let at = if s.name == drag.name {
                    landed
                } else {
                    (s.geometry.x, s.geometry.y)
                };
                (s.name.clone(), at)
            })
            .collect();
        arrange::normalized(&mut positions);
        let mut table = toml_edit::InlineTable::new();
        for (name, (x, y)) in &positions {
            table.insert(
                name,
                toml_edit::Value::from_iter([i64::from(*x), i64::from(*y)]),
            );
        }
        let names: Vec<String> = screens.iter().map(|s| s.name.clone()).collect();
        Some(Outcome::Change(Box::new(Change::layout(
            &names,
            "positions",
            table,
        ))))
    }

    fn screens(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(22.0)));
        let screens = self.ctx.screens.clone();
        if self
            .screens
            .chosen
            .as_ref()
            .is_none_or(|c| !screens.iter().any(|s| &s.name == c))
        {
            self.screens.chosen = screens
                .iter()
                .find(|s| s.primary)
                .or_else(|| screens.first())
                .map(|s| s.name.clone());
        }

        // The arrangement: each screen where it is, to drag into place.
        let area = self.add(
            body,
            Style {
                width: Length::Fill,
                height: Length::Px(AREA_HEIGHT),
                clip: true,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Background),
                radius: 18.0,
                border: Some(Paint::Border),
                ..Look::default()
            }),
        );
        self.tree.layout();
        let size = self
            .tree
            .rect(area)
            .map_or(Size::new(800.0, AREA_HEIGHT), |r| Size::new(r.w, r.h));
        let placed: Vec<arrange::Placed> = screens.iter().map(placed).collect();
        let mapping = match &self.screens.drag {
            Some(drag) => drag.mapping,
            None => arrange::Mapping::fit(&placed, size, 24.0, 56.0),
        };
        self.screens.area = Some((area, mapping));
        self.screens.tiles.clear();
        for (s, p) in screens.iter().zip(&placed) {
            let mut r = mapping.rect(*p);
            if let Some(drag) = self.screens.drag.as_ref().filter(|d| d.name == s.name) {
                r.x = drag.at.x;
                r.y = drag.at.y;
            }
            let chosen = self.screens.chosen.as_ref() == Some(&s.name);
            let tile = self.add(
                area,
                Style {
                    at: Some(Point::new(r.x, r.y)),
                    width: Length::Px(r.w.max(24.0)),
                    height: Length::Px(r.h.max(24.0)),
                    padding: Insets::symmetric(8.0, 10.0),
                    justify: Justify::SpaceBetween,
                    clip: true,
                    ..column(2.0)
                },
                Kind::Box(Look {
                    background: s.enabled.then_some(Paint::SurfaceHover),
                    radius: 10.0,
                    border: Some(if chosen { Paint::Accent } else { Paint::Border }),
                    ..Look::default()
                }),
            );
            let title = screen_title(s, lang);
            self.tree.set_clickable(tile, &title);
            self.tree.set_selected_box(tile, chosen);
            self.targets
                .insert(tile, Target::ChooseScreen(s.name.clone()));
            self.screens.tiles.push((s.name.clone(), tile));
            let ink = if s.enabled { Paint::Text } else { Paint::Muted };
            let name_row = self.add(tile, row(8.0), Kind::Box(Look::default()));
            self.line(
                name_row,
                &title,
                theme.text.label.with_weight(700).with_color(ink),
            );
            if s.primary {
                let pill = self.add(
                    name_row,
                    Style {
                        padding: Insets::symmetric(2.0, 8.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Rgba(theme.colors.accent.with_alpha(46))),
                        radius: 11.0,
                        ..Look::default()
                    }),
                );
                self.line(
                    pill,
                    words::PRIMARY.get(lang),
                    theme
                        .text
                        .caption
                        .with_weight(700)
                        .with_color(Paint::Accent),
                );
            }
            let detail = if s.enabled {
                format!(
                    "{} × {} · ×{} · {}",
                    pixels(s).0,
                    pixels(s).1,
                    trim(s.scale),
                    words::desktops(s.desktops, lang)
                )
            } else {
                words::OFF.get(lang).to_owned()
            };
            self.line(tile, &detail, theme.text.caption.with_color(Paint::Muted));
            self.highlighted(tile, &Setting::Screen(s.name.clone()), Look::default());
        }
        // Under the screens: how to place them, and what else to do.
        let bar = self.add(
            area,
            Style {
                at: Some(Point::new(0.0, size.h - 56.0)),
                width: Length::Px(size.w),
                height: Length::Px(56.0),
                padding: Insets::symmetric(8.0, 16.0),
                justify: Justify::SpaceBetween,
                ..row(8.0)
            },
            Kind::Box(Look::default()),
        );
        self.line(
            bar,
            words::DRAG_HINT.get(lang),
            theme.text.label.with_color(Paint::Muted),
        );
        let buttons = self.add(bar, row(8.0), Kind::Box(Look::default()));
        let identify = self.add(
            buttons,
            Style {
                height: Length::Px(40.0),
                ..Style::default()
            },
            Kind::Button(Button::new(
                words::IDENTIFY.get(lang),
                ButtonKind::Plain,
                theme,
            )),
        );
        self.targets.insert(identify, Target::Identify);
        // Mirroring (#125): every other lit screen shows the primary one.
        let primary = self
            .ctx
            .screens
            .iter()
            .find(|s| s.primary)
            .or_else(|| self.ctx.screens.iter().find(|s| s.enabled))
            .map(|s| s.name.clone());
        let others: Vec<String> = self
            .ctx
            .screens
            .iter()
            .filter(|s| s.enabled && Some(&s.name) != primary.as_ref())
            .map(|s| s.name.clone())
            .collect();
        let mirroring = !others.is_empty()
            && others.iter().all(|name| {
                self.ctx.config.output(name).and_then(|o| o.mirror.as_ref()) == primary.as_ref()
            });
        let label = if mirroring {
            words::STOP_MIRROR
        } else {
            words::MIRROR
        };
        let mut button = Button::new(label.get(lang), ButtonKind::Plain, theme);
        button.enabled = primary.is_some() && !others.is_empty();
        let mirror = self.add(
            buttons,
            Style {
                height: Length::Px(40.0),
                ..Style::default()
            },
            Kind::Button(button),
        );
        if let Some(primary) = primary {
            let changes = others
                .iter()
                .map(|name| {
                    if mirroring {
                        Change::output_unset(name, "mirror")
                    } else {
                        Change::output(name, "mirror", primary.as_str())
                    }
                })
                .collect();
            self.targets.insert(mirror, Target::SetAll(changes));
        }

        let wide = self.tree.size().w >= 1180.0;
        let pair = self.add(
            body,
            Style {
                align: Align::Start,
                direction: if wide {
                    Direction::Row
                } else {
                    Direction::Column
                },
                gap: if wide { 36.0 } else { 26.0 },
                ..fill_width(Style::default())
            },
            Kind::Box(Look::default()),
        );
        if let Some(s) = screens
            .iter()
            .find(|s| self.screens.chosen.as_ref() == Some(&s.name))
        {
            self.screen_settings(theme, pair, s, wide);
        }
        self.layouts(theme, pair, &screens);
    }

    /// The chosen screen's mode, scale, desktops and switches.
    fn screen_settings(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        s: &hocus_api::ScreenInfo,
        wide: bool,
    ) {
        let lang = self.lang;
        let section = self.add(
            parent,
            Style {
                grow: 1.5,
                ..fill_width(column(14.0))
            },
            Kind::Box(Look::default()),
        );
        let title = screen_title(s, lang).to_uppercase();
        self.line(section, &title, theme.text.caption);
        let current = s.current_mode.and_then(|i| s.modes.get(i)).copied();

        // Resolution and refresh rate, each opening its choices.
        let modes = self.add(section, fill_width(row(10.0)), Kind::Box(Look::default()));
        let (w, h) = pixels(s);
        let hz = current.map_or(0.0, |m| f64::from(m.refresh) / 1000.0);
        for (list, label) in [
            (
                ModeList::Resolution,
                labelled(words::RESOLUTION, &format!("{w} × {h}"), lang),
            ),
            (
                ModeList::Rate,
                labelled(words::REFRESH, &format!("{} Hz", trim(hz)), lang),
            ),
        ] {
            let mut b = Button::new(label, ButtonKind::Plain, theme);
            b.start = true;
            b.selected = self.screens.list == Some(list);
            b.enabled = s.enabled;
            let id = self.add(
                modes,
                Style {
                    width: Length::Fill,
                    height: Length::Px(48.0),
                    ..Style::default()
                },
                Kind::Button(b),
            );
            self.targets.insert(id, Target::List(list));
        }
        if let (Some(list), Some(current)) = (self.screens.list, current) {
            let choices: Vec<(String, String)> = match list {
                ModeList::Resolution => {
                    let mut sizes: Vec<(i32, i32)> =
                        s.modes.iter().map(|m| (m.width, m.height)).collect();
                    sizes.sort_by_key(|(w, h)| std::cmp::Reverse(i64::from(*w) * i64::from(*h)));
                    sizes.dedup();
                    sizes
                        .into_iter()
                        .map(|(w, h)| (format!("{w} × {h}"), format!("{w}x{h}")))
                        .collect()
                }
                ModeList::Rate => {
                    let mut rates: Vec<i32> = s
                        .modes
                        .iter()
                        .filter(|m| m.width == current.width && m.height == current.height)
                        .map(|m| m.refresh)
                        .collect();
                    rates.sort_unstable_by(|a, b| b.cmp(a));
                    rates.dedup();
                    rates
                        .into_iter()
                        .map(|r| {
                            let hz = f64::from(r) / 1000.0;
                            (
                                format!("{} Hz", trim(hz)),
                                format!("{}x{}@{hz}", current.width, current.height),
                            )
                        })
                        .collect()
                }
            };
            let grid = self.add(section, fill_width(column(6.0)), Kind::Box(Look::default()));
            for chunk in choices.chunks(4) {
                let line = self.add(grid, fill_width(row(6.0)), Kind::Box(Look::default()));
                for (label, mode) in chunk {
                    let mut b = Button::new(label.as_str(), ButtonKind::Plain, theme);
                    b.selected = match list {
                        ModeList::Resolution => *label == format!("{w} × {h}"),
                        ModeList::Rate => *label == format!("{} Hz", trim(hz)),
                    };
                    let id = self.add(
                        line,
                        Style {
                            width: Length::Fill,
                            height: Length::Px(40.0),
                            ..Style::default()
                        },
                        Kind::Button(b),
                    );
                    self.targets.insert(
                        id,
                        Target::Set(Change::output(&s.name, "mode", mode.as_str())),
                    );
                }
            }
        }

        // Scale, as segments.
        let scale_row = self.add(section, fill_width(row(14.0)), Kind::Box(Look::default()));
        self.add(
            scale_row,
            Style {
                width: Length::Px(90.0),
                ..Style::default()
            },
            Kind::text_line(words::SCALE.get(lang), theme.text.body),
        );
        let segments = self.add(
            scale_row,
            Style {
                padding: Insets::all(4.0),
                ..fill_width(row(4.0))
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 14.0,
                ..Look::default()
            }),
        );
        for percent in [100u32, 125, 150, 175, 200] {
            let value = f64::from(percent) / 100.0;
            let on = (s.scale - value).abs() < 0.01;
            let mut b = Button::new(
                words::percent(percent, lang),
                if on {
                    ButtonKind::Primary
                } else {
                    ButtonKind::Plain
                },
                theme,
            );
            b.selected = on;
            b.enabled = s.enabled;
            let id = self.add(
                segments,
                Style {
                    width: Length::Fill,
                    height: Length::Px(40.0),
                    ..Style::default()
                },
                Kind::Button(b),
            );
            self.targets
                .insert(id, Target::Set(Change::output(&s.name, "scale", value)));
        }

        // Its own desktops.
        let desk_row = self.add(section, fill_width(row(14.0)), Kind::Box(Look::default()));
        self.add(
            desk_row,
            Style {
                width: Length::Px(90.0),
                ..Style::default()
            },
            Kind::text_line(words::DESKTOPS.get(lang), theme.text.body),
        );
        let stepper = self.add(
            desk_row,
            Style {
                padding: Insets::all(4.0),
                ..row(4.0)
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 14.0,
                ..Look::default()
            }),
        );
        let n = s.desktops.max(1);
        for (label, name, to) in [
            ("−", words::FEWER_DESKTOPS, n.saturating_sub(1).max(1)),
            ("+", words::MORE_DESKTOPS, (n + 1).min(8)),
        ] {
            let b = Button::new(label, ButtonKind::Plain, theme).with_name(name.get(lang));
            let id = self.add(
                stepper,
                Style {
                    width: Length::Px(44.0),
                    height: Length::Px(40.0),
                    ..Style::default()
                },
                Kind::Button(b),
            );
            self.targets.insert(
                id,
                Target::Set(Change::output(&s.name, "desktops", i64::from(to))),
            );
            if label == "−" {
                self.add(
                    stepper,
                    Style {
                        width: Length::Px(36.0),
                        align: Align::Center,
                        ..Style::default()
                    },
                    Kind::text_line(n.to_string(), theme.text.title.with_weight(700)),
                );
            }
        }
        if wide {
            self.line(
                desk_row,
                words::ONE_DESKTOP_NOTE.get(lang),
                theme.text.label.with_color(Paint::Muted),
            );
        }

        // On, primary, variable refresh rate.
        let switches = self.add(section, fill_width(row(10.0)), Kind::Box(Look::default()));
        let others_primary: Vec<Change> = self
            .ctx
            .config
            .outputs
            .iter()
            .filter(|o| o.name != s.name && o.primary == Some(true))
            .map(|o| Change::output(&o.name, "primary", false))
            .collect();
        for (label, on, works, changes) in [
            (
                words::SCREEN_ON,
                s.enabled,
                true,
                vec![Change::output(&s.name, "enabled", !s.enabled)],
            ),
            (
                words::PRIMARY,
                s.primary,
                s.enabled && !s.primary,
                std::iter::once(Change::output(&s.name, "primary", true))
                    .chain(others_primary.clone())
                    .collect(),
            ),
            (
                words::VRR,
                s.vrr,
                s.enabled && s.vrr_capable,
                vec![Change::output(&s.name, "vrr", !s.vrr)],
            ),
        ] {
            let card = self.add(
                switches,
                Style {
                    height: Length::Px(56.0),
                    padding: Insets::symmetric(8.0, 14.0),
                    ..fill_width(row(12.0))
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 14.0,
                    ..Look::default()
                }),
            );
            let ink = if works { Paint::Text } else { Paint::Muted };
            if works {
                self.tree.set_clickable(card, label.get(lang));
                self.tree.set_selected_box(card, on);
                self.targets.insert(card, Target::SetAll(changes));
            }
            self.add(
                card,
                fill_width(Style::default()),
                Kind::text_line(
                    label.get(lang),
                    theme.text.label.with_weight(600).with_color(ink),
                ),
            );
            self.add(
                card,
                Style {
                    width: Length::Px(40.0),
                    height: Length::Px(24.0),
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Switch(on)),
                    label: None,
                },
            );
        }
    }

    /// The layouts the file keeps, the current set's first.
    fn layouts(&mut self, theme: &Theme, parent: NodeId, screens: &[hocus_api::ScreenInfo]) {
        let lang = self.lang;
        let section = self.add(parent, fill_width(column(12.0)), Kind::Box(Look::default()));
        self.caption(theme, section, words::LAYOUTS_KEPT);
        let connected: Vec<String> = screens.iter().map(|s| s.name.clone()).collect();
        let names: Vec<&str> = connected.iter().map(String::as_str).collect();
        let mut rows: Vec<(String, Vec<String>, bool)> = Vec::new();
        let current = self.ctx.config.layout_for(&names);
        rows.push((
            current.map_or_else(|| default_layout_name(&connected), |l| l.name.clone()),
            connected.clone(),
            true,
        ));
        for layout in &self.ctx.config.layouts {
            if !layout.is_for(&names) {
                rows.push((layout.name.clone(), layout.outputs.clone(), false));
            }
        }
        for (name, outputs, is_current) in rows {
            let row_box = self.add(
                section,
                Style {
                    padding: Insets::symmetric(12.0, 14.0),
                    ..fill_width(row(12.0))
                },
                Kind::Box(Look {
                    background: Some(if is_current {
                        Paint::Rgba(theme.colors.accent.with_alpha(26))
                    } else {
                        Paint::SurfaceHover
                    }),
                    radius: 14.0,
                    ..Look::default()
                }),
            );
            let words_box = self.add(row_box, fill_width(column(1.0)), Kind::Box(Look::default()));
            let renaming = self
                .screens
                .renaming
                .as_ref()
                .is_some_and(|(o, _)| *o == outputs);
            if renaming {
                let field = TextField::new(words::LAYOUT_NAME.get(lang), &name, theme.text.title);
                let id = self.add(
                    words_box,
                    Style {
                        width: Length::Fill,
                        height: Length::Px(28.0),
                        ..Style::default()
                    },
                    Kind::TextField(Box::new(field)),
                );
                self.tree.set_field_value(id, &name);
                if let Some((_, node)) = &mut self.screens.renaming {
                    *node = Some(id);
                }
            } else {
                self.line(words_box, &name, theme.text.title.with_weight(700));
            }
            let mut sub = outputs.join(" + ");
            if is_current {
                sub = format!("{sub} · {}", words::CURRENT.get(lang));
            }
            self.line(words_box, &sub, theme.text.caption.with_color(Paint::Muted));
            let id = self.add(
                row_box,
                Style {
                    width: Length::Px(40.0),
                    height: Length::Px(40.0),
                    ..Style::default()
                },
                Kind::icon_button(
                    stroked(PENCIL, 16.0),
                    &format!("{} {name}", words::RENAME.get(lang)),
                    ButtonKind::Plain,
                    theme,
                ),
            );
            self.targets.insert(id, Target::Rename(outputs));
        }
        self.add(
            section,
            Style::default(),
            Kind::text(
                words::LAYOUTS_NOTE.get(lang),
                theme.text.label.with_color(Paint::Muted),
            ),
        );
    }
}

/// How a highlighted row is named for screen readers.
fn setting_name(setting: &Setting) -> String {
    match setting {
        Setting::Shortcut(combo) | Setting::Screen(combo) => combo.clone(),
        other => format!("{other:?}"),
    }
}

/// A screen's place and logical size.
fn placed(s: &hocus_api::ScreenInfo) -> arrange::Placed {
    arrange::Placed {
        x: s.geometry.x,
        y: s.geometry.y,
        w: s.geometry.width.max(1),
        h: s.geometry.height.max(1),
    }
}

/// A screen's size in pixels, in its current mode.
fn pixels(s: &hocus_api::ScreenInfo) -> (i32, i32) {
    s.current_mode
        .and_then(|i| s.modes.get(i))
        .map_or((s.geometry.width, s.geometry.height), |m| {
            (m.width, m.height)
        })
}

/// 2 for 2.0, 1.25 for 1.25, 59.95 for 59.951.
fn trim(v: f64) -> String {
    let s = format!("{:.2}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// How a screen is called: the laptop's panel, else its model or name.
fn screen_title(s: &hocus_api::ScreenInfo, lang: Lang) -> String {
    if s.name.starts_with("eDP") || s.name.starts_with("LVDS") {
        return words::LAPTOP.get(lang).to_owned();
    }
    if !s.model.is_empty() && s.model != "Unknown" && s.model != "Headless" {
        return s.model.clone();
    }
    s.name.clone()
}

/// A layout's name until renamed: its screens.
fn default_layout_name(screens: &[String]) -> String {
    let mut s = screens.to_vec();
    s.sort();
    s.join(" + ")
}

/// "Resolution: 2560 × 1600", "Définition : 2560 × 1600".
fn labelled(label: words::Words, value: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{}: {value}", label.get(lang)),
        Lang::Fr => format!("{} : {value}", label.get(lang)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(lang: Lang) -> SettingsView {
        let mut v = SettingsView::new(
            Theme::default(),
            lang,
            Size::new(1200.0, 820.0),
            1.0,
            Context::default(),
        );
        v.draw(Duration::ZERO);
        v
    }

    fn find(node: &AccessNode, label: &str) -> Option<AccessNode> {
        if node.label.as_deref() == Some(label) {
            return Some(node.clone());
        }
        node.children.iter().find_map(|c| find(c, label))
    }

    fn click(v: &mut SettingsView, label: &str) -> Vec<Outcome> {
        let node = find(&v.access(), label)
            .unwrap_or_else(|| panic!("no {label}:\n{}", v.access().dump()));
        let at = Point::new(
            node.rect.x + node.rect.w / 2.0,
            node.rect.y + node.rect.h / 2.0,
        );
        v.pointer_move(at);
        let mut out = v.pointer_button(true);
        out.extend(v.pointer_button(false));
        v.draw(Duration::ZERO);
        out
    }

    /// Scrolls the page until the element labelled `label` shows whole,
    /// below the page's title.
    fn scroll_to(v: &mut SettingsView, label: &str) {
        v.scroll(-100_000.0);
        v.draw(Duration::ZERO);
        let room = v.tree.size().h;
        for _ in 0..200 {
            let node = find(&v.access(), label)
                .unwrap_or_else(|| panic!("no {label}:\n{}", v.access().dump()));
            if node.rect.y > 140.0 && node.rect.y + node.rect.h < room {
                return;
            }
            v.scroll(50.0);
            v.draw(Duration::ZERO);
        }
        panic!("{label} never shows");
    }

    fn keyboard_view(toml: &str) -> SettingsView {
        let mut v = view(Lang::En);
        v.set_context(Context {
            config: hocus_core::Config::parse(toml).expect("a configuration"),
            ..Context::default()
        });
        v.open(Some(Page::Keyboard));
        v.draw(Duration::ZERO);
        v
    }

    fn changes(out: &[Outcome]) -> Vec<(String, Option<String>)> {
        out.iter()
            .filter_map(|o| match o {
                Outcome::Change(c) => Some((
                    c.key.clone(),
                    c.value.as_ref().map(|v| v.to_string().trim().to_owned()),
                )),
                _ => None,
            })
            .collect()
    }

    fn combo(s: &str) -> hocus_core::shortcuts::Combo {
        s.parse().expect("keys")
    }

    #[test]
    fn layouts_are_added_removed_and_put_first() {
        let mut v = keyboard_view("[keyboard]\nlayouts = [\"us\", \"fr\"]\n");
        let out = click(&mut v, "Put French · AZERTY before");
        assert_eq!(
            changes(&out),
            [("layouts".into(), Some("[\"fr\", \"us\"]".into()))]
        );
        let out = click(&mut v, "Remove English · US");
        assert_eq!(changes(&out), [("layouts".into(), Some("[\"fr\"]".into()))]);
        click(&mut v, "Add a layout");
        let out = click(&mut v, "German");
        assert_eq!(
            changes(&out),
            [("layouts".into(), Some("[\"us\", \"fr\", \"de\"]".into()))]
        );
    }

    #[test]
    fn keys_taken_change_nothing_until_replaced() {
        let mut v = keyboard_view("");
        let out = click(&mut v, "Change Fullscreen, Super+F11");
        assert!(
            matches!(out.as_slice(), [Outcome::Inhibit(true)]),
            "{out:?}"
        );
        assert!(v.recording());
        // Typing keys alone is refused: they are for typing.
        assert!(v.record(combo("M")).is_empty());
        assert!(v.recording());
        let out = v.record(combo("Super+Down"));
        assert!(
            matches!(out.as_slice(), [Outcome::Inhibit(false)]),
            "{out:?}"
        );
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Already used by “Minimize”: replace?").is_some());
        let out = click(&mut v, "Replace");
        assert_eq!(
            changes(&out),
            [
                ("Super+Down".into(), Some("\"toggle-fullscreen\"".into())),
                ("Super+F11".into(), Some("\"none\"".into())),
            ]
        );
    }

    #[test]
    fn a_new_shortcut_runs_a_command_or_an_action() {
        let mut v = keyboard_view("");
        // Under the table.
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        click(&mut v, "New shortcut: start an app, a command or an action");
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        v.text("foot --server");
        v.draw(Duration::ZERO);
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        let out = click(&mut v, "Choose its keys");
        assert!(
            matches!(out.as_slice(), [Outcome::Inhibit(true)]),
            "{out:?}"
        );
        v.record(combo("Super+Return"));
        v.draw(Duration::ZERO);
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        let out = click(&mut v, "Save");
        assert_eq!(
            changes(&out),
            [(
                "Super+Return".into(),
                Some("\"spawn foot --server\"".into())
            )]
        );
        // Escape while recording cancels and gives the shortcuts back.
        let mut v = keyboard_view("");
        click(&mut v, "Change Fullscreen, Super+F11");
        let out = v.key(Key::Escape);
        assert!(
            matches!(out.as_slice(), [Outcome::Inhibit(false)]),
            "{out:?}"
        );
        assert!(!v.recording());
    }

    #[test]
    fn the_original_shortcuts_come_back_once_asked_twice() {
        let mut v = keyboard_view("[shortcuts]\n\"Super+M\" = \"toggle-maximized\"\n");
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        assert!(click(&mut v, "Back to the original shortcuts").is_empty());
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        let out = click(&mut v, "Go back");
        assert!(
            matches!(out.as_slice(), [Outcome::Change(c)] if c.key.is_empty() && c.value.is_none()),
            "{out:?}"
        );
    }

    #[test]
    fn contrast_typed_then_enter_opens_appearance_with_the_switch_focused() {
        let mut v = view(Lang::Fr);
        v.text("contraste");
        v.draw(Duration::ZERO);
        let dump = v.access().dump();
        let first = dump.lines().position(|l| l.contains("\"Contraste élevé\""));
        assert!(first.is_some(), "{dump}");
        v.key(Key::Enter);
        v.draw(Duration::ZERO);
        assert_eq!(v.screen(), Screen::Page(Page::Appearance));
        let switch = find(&v.access(), "Contraste élevé").expect("the switch");
        assert!(switch.focused, "{}", v.access().dump());
    }

    #[test]
    fn clicking_an_accent_asks_for_it_in_the_file() {
        let mut v = view(Lang::En);
        v.open(Some(Page::Appearance));
        v.draw(Duration::ZERO);
        let out = click(&mut v, "Accent Amber");
        let [Outcome::Change(change)] = out.as_slice() else {
            panic!("{out:?}");
        };
        assert_eq!(
            (&change.place, change.key.as_str()),
            (&crate::edit::Place::Section("appearance"), "accent")
        );
        assert_eq!(
            change.value.as_ref().and_then(|v| v.as_str()),
            Some("amber")
        );
        let out = click(&mut v, "High contrast");
        assert!(
            matches!(out.as_slice(), [Outcome::Change(c)] if c.key == "high_contrast" && c.value.as_ref().and_then(|v| v.as_bool()) == Some(true)),
            "{out:?}"
        );
    }

    #[test]
    fn a_value_changed_elsewhere_shows() {
        let mut v = view(Lang::En);
        v.open(Some(Page::Appearance));
        v.draw(Duration::ZERO);
        assert!(!find(&v.access(), "Light").expect("light").selected);
        let mut ctx = Context::default();
        ctx.config.appearance.theme = "light".into();
        v.set_context(ctx);
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Light").expect("light").selected);
    }

    #[test]
    fn the_home_tiles_lead_to_their_pages_and_escape_comes_back() {
        let mut v = view(Lang::En);
        click(&mut v, "Keyboard and shortcuts");
        assert_eq!(v.screen(), Screen::Page(Page::Keyboard));
        assert!(v.access().dump().contains("Quick settings"));
        v.key(Key::Escape);
        v.draw(Duration::ZERO);
        assert_eq!(v.screen(), Screen::Home);
        assert_eq!(v.key(Key::Escape).len(), 1, "Escape on the home closes");
    }

    fn page_view(toml: &str, page: Page, outputs: &[&str]) -> SettingsView {
        let mut v = view(Lang::En);
        let outputs = outputs
            .iter()
            .map(|name| hocus_api::OutputInfo {
                name: (*name).into(),
                geometry: hocus_api::Rect {
                    x: 0,
                    y: 0,
                    width: 1280,
                    height: 800,
                },
                pixel_width: 1280,
                pixel_height: 800,
                scale: 1.0,
                temperature: 6500,
            })
            .collect();
        v.set_context(Context {
            config: hocus_core::Config::parse(toml).expect("a configuration"),
            outputs,
            ..Context::default()
        });
        v.open(Some(page));
        v.draw(Duration::ZERO);
        v
    }

    fn places(out: &[Outcome]) -> Vec<crate::edit::Place> {
        out.iter()
            .filter_map(|o| match o {
                Outcome::Change(c) => Some(c.place.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_screen_gets_its_own_wallpaper_and_a_colour_typed() {
        let mut v = page_view("", Page::Wallpaper, &["HEADLESS-1", "HEADLESS-2"]);
        assert!(find(&v.access(), "Glow").expect("glow").selected);
        let out = click(&mut v, "Stars");
        assert_eq!(changes(&out), [("kind".into(), Some("\"stars\"".into()))]);
        assert_eq!(places(&out), [crate::edit::Place::Section("wallpaper")]);
        click(&mut v, "HEADLESS-2");
        let out = click(&mut v, "Colour");
        assert_eq!(
            places(&out),
            [crate::edit::Place::WallpaperScreen("HEADLESS-2".into())]
        );
        // The file says so: the screen's colour shows, and its field.
        v.set_context(Context {
            config: hocus_core::Config::parse(
                "[[wallpaper.screens]]\noutput = \"HEADLESS-2\"\nkind = \"colour\"\n",
            )
            .expect("valid"),
            outputs: v.context().outputs.clone(),
            ..Context::default()
        });
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Colour").expect("colour").selected);
        assert!(find(&v.access(), "This screen has its own wallpaper.").is_some());
        click(&mut v, "Colour code");
        v.text("not one");
        let out = v.key(Key::Enter);
        assert!(out.is_empty(), "{out:?}");
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Not a colour: write it like #0e0f12.").is_some());
        click(&mut v, "Colour code");
        for _ in 0.."not one".len() {
            v.key(Key::Backspace);
        }
        v.text("203040");
        let out = v.key(Key::Enter);
        assert_eq!(
            changes(&out),
            [("colour".into(), Some("\"#203040\"".into()))]
        );
        let out = click(&mut v, "Like the other screens");
        assert!(
            matches!(out.as_slice(), [Outcome::Change(c)] if c.key.is_empty() && c.value.is_none()),
            "{out:?}"
        );
    }

    #[test]
    fn the_column_lines_and_the_dock_apps_change() {
        let mut v = page_view("", Page::ColumnDock, &["HEADLESS-1"]);
        let out = click(&mut v, "Coding agents");
        assert_eq!(changes(&out), [("enabled".into(), Some("false".into()))]);
        assert_eq!(places(&out), [crate::edit::Place::Section("agents")]);
        let out = click(&mut v, "Remove Firefox");
        let changed = changes(&out);
        let [(key, Some(list))] = changed.as_slice() else {
            panic!("{out:?}");
        };
        assert_eq!(key, "pinned");
        assert!(!list.contains("\"firefox.desktop\""), "{list}");
        assert!(list.contains("firefox-esr.desktop"), "{list}");
        let out = click(&mut v, "Put Nautilus before");
        assert!(
            changes(&out)[0]
                .1
                .as_deref()
                .is_some_and(|l| l.find("Nautilus") < l.find("firefox-esr")),
            "{out:?}"
        );
    }

    #[test]
    fn pandas_and_accessibility_switch_and_lead_on() {
        let mut v = page_view("", Page::Pandas, &["HEADLESS-1"]);
        assert!(find(&v.access(), "Panda waiting for an answer").is_some());
        let out = click(&mut v, "Show the pandas");
        assert_eq!(changes(&out), [("enabled".into(), Some("false".into()))]);
        let mut v = page_view(
            "[appearance]\nreduced_motion = true\n",
            Page::Accessibility,
            &[],
        );
        assert!(
            find(&v.access(), "Reduced motion")
                .expect("motion")
                .selected
        );
        let out = click(&mut v, "Reduced motion");
        assert_eq!(
            changes(&out),
            [("reduced_motion".into(), Some("false".into()))]
        );
        click(&mut v, "Change the keys");
        assert_eq!(v.screen(), Screen::Page(Page::Keyboard));
    }

    #[test]
    fn the_clock_and_the_system_s_time_zone_are_chosen() {
        use catalogue::SystemZone;
        let mut v = page_view("", Page::Language, &[]);
        assert!(find(&v.access(), "24 hours · 14:37").expect("24").selected);
        let out = click(&mut v, "12 hours · 2:37 PM");
        assert_eq!(changes(&out), [("format".into(), Some("\"12h\"".into()))]);
        v.set_zone(SystemZone::Known("Europe/Paris".into()));
        v.draw(Duration::ZERO);
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Paris").expect("paris").selected);
        // Asked of the system, never written in the file.
        let out = click(&mut v, "Tokyo");
        assert!(
            matches!(out.as_slice(), [Outcome::SetTimeZone(z)] if z == "Asia/Tokyo"),
            "{out:?}"
        );
        v.zone_set(Err("Permission denied".into()));
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Not changed: Permission denied").is_some());
        assert!(find(&v.access(), "Paris").expect("paris").selected);
        v.zone_set(Ok("Asia/Tokyo".into()));
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Tokyo").expect("tokyo").selected);
        assert!(find(&v.access(), "Not changed: Permission denied").is_none());
        // One the list does not offer shows, chosen; the compositor's news
        // keep it.
        v.set_zone(SystemZone::Known("America/Halifax".into()));
        v.set_context(v.context().clone());
        v.set_context(Context {
            config: v.context().config.clone(),
            ..Context::default()
        });
        v.draw(Duration::ZERO);
        assert!(
            find(&v.access(), "America/Halifax")
                .expect("its own")
                .selected
        );
        // Without timedated, nothing is offered to change.
        v.set_zone(SystemZone::Absent("no system bus".into()));
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Tokyo").expect("tokyo").disabled);
        assert!(click(&mut v, "Tokyo").is_empty());
    }

    fn screen(name: &str, desktops: u32) -> hocus_api::ScreenInfo {
        hocus_api::ScreenInfo {
            name: name.into(),
            make: String::new(),
            model: "Headless".into(),
            enabled: true,
            modes: Vec::new(),
            current_mode: None,
            preferred_mode: None,
            scale: 1.0,
            transform: "normal".into(),
            geometry: hocus_api::Rect {
                x: 0,
                y: 0,
                width: 1280,
                height: 800,
            },
            vrr: false,
            vrr_capable: false,
            primary: false,
            desktops,
            layout: None,
        }
    }

    fn values(out: &[Outcome]) -> Vec<(crate::edit::Place, String, Option<String>)> {
        out.iter()
            .filter_map(|o| match o {
                Outcome::Change(c) => Some((
                    c.place.clone(),
                    c.key.clone(),
                    c.value.as_ref().map(|v| v.to_string().trim().to_owned()),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn each_screen_s_desktops_are_counted_and_named() {
        use crate::edit::Place;
        let mut v = page_view(
            "[[outputs]]\nname = \"HEADLESS-2\"\ndesktop_names = [\"Mail\"]\n",
            Page::Desktops,
            &[],
        );
        let mut ctx = v.context().clone();
        ctx.screens = vec![screen("HEADLESS-1", 2), screen("HEADLESS-2", 1)];
        v.set_context(ctx);
        v.draw(Duration::ZERO);
        let out = click(&mut v, "One desktop more on HEADLESS-1");
        assert_eq!(
            values(&out),
            [(
                Place::Output("HEADLESS-1".into()),
                "desktops".into(),
                Some("3".into())
            )]
        );
        let field = v.fields.node(&Field::DesktopName {
            output: "HEADLESS-2".into(),
            index: 0,
        });
        assert_eq!(field.and_then(|f| v.tree.field_value(f)), Some("Mail"));
        click(&mut v, "Name of desktop 2 on HEADLESS-1");
        v.text("Code");
        let out = v.key(Key::Enter);
        assert_eq!(
            values(&out),
            [(
                Place::Output("HEADLESS-1".into()),
                "desktop_names".into(),
                Some("[\"\", \"Code\"]".into())
            )]
        );
        // Emptied, the name goes, and the key with the last one.
        click(&mut v, "Name of desktop 1 on HEADLESS-2");
        for _ in 0.."Mail".len() {
            v.key(Key::Backspace);
        }
        let out = v.key(Key::Enter);
        assert_eq!(
            values(&out),
            [(
                Place::Output("HEADLESS-2".into()),
                "desktop_names".into(),
                None
            )]
        );
    }

    #[test]
    fn window_rules_are_switched_removed_and_added() {
        use crate::edit::Place;
        let mut v = page_view(
            "[[rules]]\napp_id = \"chat*\"\ndesktop = 2\nmaximized = true\n",
            Page::Windows,
            &[],
        );
        let out = click(&mut v, "Between windows: 8 px");
        assert_eq!(
            values(&out),
            [(
                Place::Section("windows.gaps"),
                "inner".into(),
                Some("8".into())
            )]
        );
        let out = click(&mut v, "Snap at the edges");
        assert_eq!(changes(&out), [("snap".into(), Some("false".into()))]);
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "desktop 2").is_some());
        assert!(
            find(&v.access(), "chat*: Maximized")
                .expect("chip")
                .selected
        );
        let out = click(&mut v, "chat*: Maximized");
        assert_eq!(values(&out), [(Place::Rule(0), "maximized".into(), None)]);
        let out = click(&mut v, "chat*: Above the others");
        assert_eq!(
            values(&out),
            [(Place::Rule(0), "on_top".into(), Some("true".into()))]
        );
        let out = click(&mut v, "Remove the rule for chat*");
        assert!(
            matches!(out.as_slice(), [Outcome::Change(c)] if c.place == Place::Rule(0) && c.key.is_empty() && c.value.is_none()),
            "{out:?}"
        );
        click(&mut v, "App of a new rule");
        v.text("editor");
        let out = click(&mut v, "Add the rule");
        assert_eq!(
            values(&out),
            [(Place::Rule(1), "app_id".into(), Some("\"editor\"".into()))]
        );
    }

    #[test]
    fn the_touchpad_the_mouse_gestures_and_corners_are_set_apart() {
        use crate::edit::Place;
        let mut v = page_view("", Page::Mouse, &[]);
        let out = click(&mut v, "Touchpad: Natural scrolling");
        assert_eq!(
            values(&out),
            [(
                Place::Section("touchpad"),
                "natural_scroll".into(),
                Some("true".into())
            )]
        );
        let out = click(&mut v, "Mouse: Natural scrolling");
        assert_eq!(
            values(&out),
            [(
                Place::Section("mouse"),
                "natural_scroll".into(),
                Some("true".into())
            )]
        );
        assert!(
            find(&v.access(), "Mouse: Speed: Normal")
                .expect("speed")
                .selected
        );
        let out = click(&mut v, "Mouse: Speed: Fast");
        assert_eq!(
            values(&out),
            [(Place::Section("mouse"), "speed".into(), Some("0.3".into()))]
        );
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        let out = click(&mut v, "Four fingers down: Desktop");
        assert_eq!(
            values(&out),
            [(
                Place::Section("gestures"),
                "four_down".into(),
                Some("\"show-desktop\"".into())
            )]
        );
        let out = click(&mut v, "Top left: Overview");
        assert_eq!(
            values(&out),
            [(
                Place::Section("hot_corners"),
                "top_left".into(),
                Some("\"overview\"".into())
            )]
        );
    }

    #[test]
    fn captures_and_the_clipboard_are_set() {
        use crate::edit::Place;
        let mut v = page_view("", Page::Captures, &[]);
        let out = click(&mut v, "Copy to the clipboard");
        assert_eq!(
            values(&out),
            [(
                Place::Section("screenshots"),
                "copy".into(),
                Some("false".into())
            )]
        );
        click(&mut v, "Screenshots folder");
        v.text("~/Pictures/Shots");
        let out = v.key(Key::Enter);
        assert_eq!(
            values(&out),
            [(
                Place::Section("screenshots"),
                "folder".into(),
                Some("\"~/Pictures/Shots\"".into())
            )]
        );
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        let out = click(&mut v, "Frames a second: 60");
        assert_eq!(
            values(&out),
            [(Place::Section("recording"), "fps".into(), Some("60".into()))]
        );
        let out = click(&mut v, "Microphone");
        assert_eq!(changes(&out), [("microphone".into(), Some("true".into()))]);

        let mut v = page_view("[clipboard]\nhistory = false\n", Page::Clipboard, &[]);
        // Without a history, nothing is kept across sessions.
        assert!(!find(&v.access(), "Across sessions").expect("kept").selected);
        let out = click(&mut v, "Copies kept: 50");
        assert_eq!(
            values(&out),
            [(
                Place::Section("clipboard"),
                "entries".into(),
                Some("50".into())
            )]
        );
        let out = click(&mut v, "Keep a history");
        assert_eq!(changes(&out), [("history".into(), Some("true".into()))]);
    }

    fn hardware_view(toml: &str, page: Page) -> SettingsView {
        let mut v = page_view(toml, page, &[]);
        let mut ctx = v.context().clone();
        ctx.hardware = hocus_api::HardwareInfo {
            known: true,
            wifi: true,
            network: Some("Bamboo grove".into()),
            bluetooth: true,
            volume: 40,
            output_device: "Speakers".into(),
            profile: "balanced".into(),
            profiles: vec!["power-saver".into(), "balanced".into()],
            ..Default::default()
        };
        v.set_context(ctx);
        v.draw(Duration::ZERO);
        v
    }

    fn hardware(out: &[Outcome]) -> Vec<hocus_api::HardwareChange> {
        out.iter()
            .filter_map(|o| match o {
                Outcome::Hardware(c) => Some(c.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_services_are_changed_through_the_compositor() {
        use hocus_api::HardwareChange as H;
        let mut v = hardware_view("", Page::Sound);
        assert!(find(&v.access(), "Volume 40%").is_some());
        let out = click(&mut v, "Mute");
        assert_eq!(hardware(&out), [H::Muted { on: true }]);
        let mut v = hardware_view("", Page::Network);
        assert!(find(&v.access(), "Joined: Bamboo grove").is_some());
        let out = click(&mut v, "Wi-Fi");
        assert_eq!(hardware(&out), [H::Wifi { on: false }]);
        let mut v = hardware_view("", Page::Power);
        // Only the profiles the machine offers.
        assert!(find(&v.access(), "Power profile: Performance").is_none());
        let out = click(&mut v, "Power profile: Saver");
        assert_eq!(
            hardware(&out),
            [H::Profile {
                profile: "power-saver".into()
            }]
        );
        // Until the services answer, their switches do nothing.
        let mut v = page_view("", Page::Bluetooth, &[]);
        assert!(click(&mut v, "Bluetooth radio").is_empty());
    }

    fn network(
        ssid: &str,
        strength: u8,
        security: hocus_platform::wifi::Security,
    ) -> hocus_platform::wifi::Network {
        hocus_platform::wifi::Network {
            ssid: ssid.into(),
            strength,
            security,
            connected: false,
            saved: false,
        }
    }

    #[test]
    fn networks_are_joined_with_a_password_left_and_forgotten() {
        use hocus_platform::wifi::{JoinError, Security, Wifi};
        let mut v = hardware_view("", Page::Network);
        assert!(find(&v.access(), "Looking for networks…").is_some());
        let mut joined = network("Bamboo grove", 82, Security::Personal);
        joined.connected = true;
        joined.saved = true;
        v.set_wifi(Listed::Known(Wifi::Networks(vec![
            joined,
            network("Panda den", 64, Security::Personal),
            network("Café du Parc", 47, Security::Open),
            network("Office", 58, Security::Other),
        ])));
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Connected · Secured · 82%").is_some());
        // An account's network is joined elsewhere.
        assert!(find(&v.access(), "Join Office").is_none());
        let out = click(&mut v, "Disconnect Bamboo grove");
        assert!(
            matches!(out.as_slice(), [Outcome::Wifi(WifiAction::Disconnect(s))] if s == "Bamboo grove"),
            "{out:?}"
        );
        v.wifi_done(Ok(()));
        // An open one at once; a secured one asks for its password first.
        let out = click(&mut v, "Join Café du Parc");
        assert!(
            matches!(out.as_slice(), [Outcome::Wifi(WifiAction::Join { ssid, password: None })] if ssid == "Café du Parc"),
            "{out:?}"
        );
        assert!(find(&v.access(), "Joining…").is_some());
        v.wifi_done(Ok(()));
        v.draw(Duration::ZERO);
        assert!(click(&mut v, "Join Panda den").is_empty());
        let field = find(&v.access(), "Password for Panda den").expect("the field");
        assert!(field.focused);
        v.text("bamboo shoots");
        let out = v.key(Key::Enter);
        match out.as_slice() {
            [
                Outcome::Wifi(WifiAction::Join {
                    ssid,
                    password: Some(p),
                }),
            ] => {
                assert_eq!(
                    (ssid.as_str(), p.0.as_str()),
                    ("Panda den", "bamboo shoots")
                );
                assert!(!format!("{out:?}").contains("bamboo"), "never printed");
            }
            other => panic!("{other:?}"),
        }
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Password for Panda den").is_none());
        v.wifi_done(Err(JoinError::Failed));
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Not joined: check the password").is_some());
        // Escape forgets what was typed.
        click(&mut v, "Join Panda den");
        v.text("oops");
        v.key(Key::Escape);
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Password for Panda den").is_none());
        // The radio off: no list.
        v.set_wifi(Listed::Known(Wifi::Off));
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Turn Wi-Fi on to see the networks in reach.").is_some());
    }

    #[test]
    fn devices_are_paired_with_their_passkey_and_forgotten() {
        use hocus_platform::bluetooth::{Bluetooth, Device, Kind};
        let device = |name: &str, kind, paired, connected, battery| Device {
            path: format!("/org/bluez/hci0/{name}"),
            name: name.into(),
            kind,
            paired,
            connected,
            battery,
        };
        let mut v = hardware_view("", Page::Bluetooth);
        v.set_bluetooth(Listed::Known(Bluetooth::Devices(vec![
            device("Headphones", Kind::Headphones, true, true, Some(80)),
            device("Keyboard", Kind::Keyboard, false, false, None),
        ])));
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Connected: Headphones").is_some());
        assert!(find(&v.access(), "Headphones · Connected · Battery 80%").is_some());
        let out = click(&mut v, "Pair Keyboard");
        assert!(
            matches!(out.as_slice(), [Outcome::Bluetooth(BluetoothAction::Pair(p))] if p.ends_with("Keyboard")),
            "{out:?}"
        );
        assert!(find(&v.access(), "Pairing…").is_some());
        v.set_pairing(Some(Pairing::Confirm(123_456)));
        v.draw(Duration::ZERO);
        assert!(
            find(
                &v.access(),
                "Does Keyboard show 123456? Pair only if it is the same."
            )
            .is_some()
        );
        let out = click(&mut v, "Same code: pair Keyboard");
        assert!(
            matches!(out.as_slice(), [Outcome::PairingAnswer(true)]),
            "{out:?}"
        );
        v.bluetooth_done(Err("Authentication Rejected".into()));
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Did not work: Authentication Rejected").is_some());
        let out = click(&mut v, "Forget Headphones");
        assert!(
            matches!(
                out.as_slice(),
                [Outcome::Bluetooth(BluetoothAction::Forget(_))]
            ),
            "{out:?}"
        );
    }

    #[test]
    fn a_city_clicked_on_the_map_gives_its_zone() {
        use catalogue::SystemZone;
        use hocus_platform::timedate::Place;
        let mut v = page_view("", Page::Language, &[]);
        v.set_zone(SystemZone::Known("Europe/Paris".into()));
        v.set_places(vec![
            Place {
                zone: "Europe/Paris".into(),
                latitude: 48.87,
                longitude: 2.33,
            },
            Place {
                zone: "Asia/Tokyo".into(),
                latitude: 35.65,
                longitude: 139.74,
            },
        ]);
        v.draw(Duration::ZERO);
        let map = find(&v.access(), "World map of the time zones").expect("the map");
        assert!(find(&v.access(), "In effect: Paris").is_some());
        // Near Tokyo's dot.
        let rect = hocus_ui::Rect::new(map.rect.x, map.rect.y, map.rect.w, map.rect.h);
        let tokyo = map::project(rect, 35.65, 139.74);
        v.pointer_move(Point::new(tokyo.x + 4.0, tokyo.y - 3.0));
        let mut out = v.pointer_button(true);
        out.extend(v.pointer_button(false));
        assert!(
            matches!(out.as_slice(), [Outcome::SetTimeZone(z)] if z == "Asia/Tokyo"),
            "{out:?}"
        );
        // Far from every city: nothing.
        v.pointer_move(Point::new(map.rect.x + 2.0, map.rect.y + map.rect.h - 2.0));
        let mut out = v.pointer_button(true);
        out.extend(v.pointer_button(false));
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn the_battery_s_delays_go_to_their_own_section() {
        use crate::edit::Place;
        let mut v = hardware_view("[idle]\ncurtain_after = 600\n", Page::Power);
        // Left out on battery, a laptop's: shorter than the charger's.
        let battery = find(&v.access(), "On battery: Dim the screens after: 2 min");
        assert!(battery.expect("the battery's").selected);
        let out = click(&mut v, "On battery: Dim the screens after: 1 min");
        assert_eq!(
            values(&out),
            [(
                Place::Section("idle.battery"),
                "curtain_after".into(),
                Some("60".into())
            )]
        );
        let out = click(&mut v, "On the charger: Sleep after: 1 h");
        assert_eq!(
            values(&out),
            [(
                Place::Section("idle"),
                "sleep_after".into(),
                Some("3600".into())
            )]
        );
        let mut v = page_view("", Page::Lock, &[]);
        let out = click(&mut v, "Curtain before locking: 1 min");
        assert_eq!(changes(&out), [("lock_after".into(), Some("60".into()))]);
        let out = click(&mut v, "Lock after the curtain");
        assert_eq!(changes(&out), [("lock".into(), Some("false".into()))]);
    }

    #[test]
    fn each_application_s_notifications_and_do_not_disturb_are_set() {
        use crate::edit::Place;
        let mut v = page_view(
            "[notifications.apps]\nChat = \"off\"\n",
            Page::Notifications,
            &[],
        );
        let mut ctx = v.context().clone();
        ctx.notifications.apps = vec!["Slack".into()];
        v.set_context(ctx);
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Slack: Banners").expect("Slack").selected);
        assert!(find(&v.access(), "Chat: Off").expect("Chat").selected);
        let out = click(&mut v, "Do not disturb");
        assert!(
            matches!(out.as_slice(), [Outcome::DoNotDisturb(true)]),
            "{out:?}"
        );
        let out = click(&mut v, "The column keeps: 50");
        assert_eq!(changes(&out), [("history".into(), Some("50".into()))]);
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        let out = click(&mut v, "Slack: Column only");
        assert_eq!(
            values(&out),
            [(
                Place::Section("notifications.apps"),
                "Slack".into(),
                Some("\"history\"".into())
            )]
        );
        // Back to banners, the key goes.
        let out = click(&mut v, "Chat: Banners");
        assert_eq!(
            values(&out),
            [(Place::Section("notifications.apps"), "Chat".into(), None)]
        );
    }

    #[test]
    fn default_applications_are_chosen_and_autostart_turned_off() {
        let mut v = page_view("", Page::Apps, &[]);
        assert!(find(&v.access(), "Reading the applications…").is_some());
        v.set_apps(catalogue::AppsState::Known(catalogue::Apps {
            defaults: vec![hocus_platform::mime::DefaultApp {
                kind: "web",
                current: Some("firefox.desktop".into()),
                choices: vec![
                    ("firefox.desktop".into(), "Firefox".into()),
                    ("web.desktop".into(), "Web".into()),
                ],
            }],
            autostart: vec![hocus_platform::apps::Autostart {
                id: "tray.desktop".into(),
                name: "Tray".into(),
                enabled: true,
            }],
        }));
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Web browser: Web").is_none());
        click(&mut v, "Change: Web browser");
        assert!(
            find(&v.access(), "Web browser: Firefox")
                .expect("it")
                .selected
        );
        let out = click(&mut v, "Web browser: Web");
        assert!(
            matches!(out.as_slice(), [Outcome::SetDefaultApp { kind, app }] if kind == "web" && app == "web.desktop"),
            "{out:?}"
        );
        assert!(find(&v.access(), "Web browser: Web").is_none(), "closed");
        let out = click(&mut v, "Tray");
        assert!(
            matches!(out.as_slice(), [Outcome::SetAutostart { id, on: false }] if id == "tray.desktop"),
            "{out:?}"
        );
        let out = click(&mut v, "Last session’s windows: Always");
        assert_eq!(
            changes(&out),
            [("restore".into(), Some("\"always\"".into()))]
        );
    }

    #[test]
    fn the_account_s_name_goes_to_accounts_service_and_updates_are_set() {
        use hocus_platform::account::{AccountInfo, Fingerprint, User, UserState};
        let mut v = page_view("", Page::Account, &[]);
        let mut ctx = v.context().clone();
        ctx.user = "alex".into();
        v.set_context(ctx);
        assert!(find(&v.access(), "Full name").is_none(), "not asked yet");
        v.set_account(AccountInfo {
            user: UserState::Known(User {
                real_name: "Alex Panda".into(),
                icon_file: None,
                picture: None,
            }),
            fingerprint: Fingerprint::Reader {
                name: "Panda Touch".into(),
                fingers: vec!["right-index-finger".into()],
            },
        });
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "alex · Hocuspanda 0.1.0").is_some());
        assert!(find(&v.access(), "Panda Touch · 1 finger enrolled").is_some());
        click(&mut v, "Full name");
        for _ in 0.."Alex Panda".len() {
            v.key(Key::Backspace);
        }
        v.text("Alex P.");
        let out = v.key(Key::Enter);
        assert!(
            matches!(out.as_slice(), [Outcome::SetRealName(n)] if n == "Alex P."),
            "{out:?}"
        );
        v.account_refused("Not authorized".into());
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Not authorized").is_some());
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        let out = click(&mut v, "Look every: 1 day");
        assert_eq!(changes(&out), [("every_hours".into(), Some("24".into()))]);
        let out = click(&mut v, "Look for updates");
        assert_eq!(changes(&out), [("enabled".into(), Some("false".into()))]);
    }

    fn with_config(v: &mut SettingsView, toml: &str, keys: Option<Vec<String>>) {
        let mut ctx = v.context().clone();
        ctx.config = hocus_core::Config::parse(toml).expect("a configuration");
        ctx.ai.keys = keys;
        v.set_context(ctx);
        v.draw(Duration::ZERO);
    }

    #[test]
    fn the_ai_its_provider_address_and_key_are_set() {
        let mut v = page_view(
            "[ai]\nurl = \"http://127.0.0.1:11434\"\nmodel = \"llama3\"\n",
            Page::Agents,
            &[],
        );
        assert!(find(&v.access(), "ollama needs no key.").is_some());
        assert!(find(&v.access(), "Provider: ollama").expect("it").selected);
        let out = click(&mut v, "Ask the AI");
        assert_eq!(changes(&out), [("enabled".into(), Some("true".into()))]);
        // The chosen one again changes nothing.
        assert!(changes(&click(&mut v, "Provider: ollama")).is_empty());
        // Another: its own address and model, not ollama's.
        let out = click(&mut v, "Provider: Claude");
        assert_eq!(
            changes(&out),
            [
                ("provider".into(), Some("\"claude\"".into())),
                ("url".into(), None),
                ("model".into(), None)
            ]
        );
        with_config(&mut v, "[ai]\nprovider = \"claude\"\n", None);
        assert!(find(&v.access(), "Reading the keyring…").is_some());
        with_config(&mut v, "[ai]\nprovider = \"claude\"\n", Some(Vec::new()));
        assert!(find(&v.access(), "No key").is_some());
        assert!(find(&v.access(), "Remove the key").is_none());
        click(&mut v, "Provider key");
        v.text("sk-panda");
        let out = v.key(Key::Enter);
        assert!(
            matches!(out.as_slice(), [Outcome::AiKey { provider, key }] if provider == "claude" && key == "sk-panda"),
            "{out:?}"
        );
        v.draw(Duration::ZERO);
        let field = v.fields.node(&Field::AiKey).expect("the field");
        assert_eq!(v.tree.field_value(field), Some(""), "never kept here");
        with_config(
            &mut v,
            "[ai]\nprovider = \"claude\"\n",
            Some(vec!["claude".into()]),
        );
        assert!(find(&v.access(), "A key is kept").is_some());
        let out = click(&mut v, "Remove the key");
        assert!(
            matches!(out.as_slice(), [Outcome::AiKey { provider, key }] if provider == "claude" && key.is_empty()),
            "{out:?}"
        );
        // In the clear to another machine: not written.
        click(&mut v, "AI address");
        v.text("http://example.org");
        assert!(v.key(Key::Enter).is_empty());
        v.draw(Duration::ZERO);
        assert!(
            find(
                &v.access(),
                "Not written: an https:// address, or http:// to this machine."
            )
            .is_some()
        );
        click(&mut v, "AI address");
        for _ in 0.."http://example.org".len() {
            v.key(Key::Backspace);
        }
        v.text("https://llm.example.org");
        let out = v.key(Key::Enter);
        assert_eq!(
            changes(&out),
            [("url".into(), Some("\"https://llm.example.org\"".into()))]
        );
        // OpenAI's API elsewhere: chosen until its address is typed.
        let out = click(&mut v, "Provider: OpenAI-compatible");
        assert_eq!(
            changes(&out),
            [("provider".into(), Some("\"openai\"".into()))]
        );
        with_config(&mut v, "[ai]\nprovider = \"openai\"\n", Some(Vec::new()));
        assert!(
            find(&v.access(), "Provider: OpenAI-compatible")
                .expect("it")
                .selected
        );
        assert!(!find(&v.access(), "Provider: OpenAI").expect("it").selected);
        with_config(
            &mut v,
            "[ai]\nprovider = \"openai\"\nurl = \"http://127.0.0.1:1234\"\n",
            Some(Vec::new()),
        );
        assert!(
            find(&v.access(), "Provider: OpenAI-compatible")
                .expect("it")
                .selected
        );
        let out = click(&mut v, "Provider: OpenAI");
        assert_eq!(
            changes(&out),
            [
                ("provider".into(), Some("\"openai\"".into())),
                ("url".into(), None)
            ]
        );
    }

    #[test]
    fn herdr_sockets_the_journal_the_stop_and_the_circle_are_set() {
        use crate::edit::Place;
        let mut v = page_view("", Page::Agents, &[]);
        let mut ctx = v.context().clone();
        ctx.screens = vec![screen("HEADLESS-1", 4), screen("HEADLESS-2", 2)];
        v.set_context(ctx);
        v.scroll(700.0);
        v.draw(Duration::ZERO);
        assert!(
            find(
                &v.access(),
                "No herdr named: the session looks for herdr’s own socket, ~/.config/herdr/herdr.sock."
            )
            .is_some()
        );
        click(&mut v, "New herdr socket");
        v.text("~/web.sock");
        let out = v.key(Key::Enter);
        assert_eq!(
            values(&out),
            [(
                Place::Herdr(0),
                "socket".into(),
                Some("\"~/web.sock\"".into())
            )]
        );
        let mut ctx = v.context().clone();
        ctx.config = hocus_core::Config::parse(
            "[agents]\nherdr_socket = \"~/old.sock\"\n\n[[agents.herdr]]\nsocket = \"~/web.sock\"\n",
        )
        .expect("valid");
        v.set_context(ctx);
        v.draw(Duration::ZERO);
        assert!(
            find(&v.access(), "Desktop of ~/web.sock: None")
                .expect("it")
                .selected
        );
        // The primary screen's desktops, the first's without one.
        assert!(find(&v.access(), "Desktop of ~/web.sock: 4").is_some());
        assert!(find(&v.access(), "Desktop of ~/web.sock: 5").is_none());
        let out = click(&mut v, "Desktop of ~/web.sock: 3");
        assert_eq!(
            values(&out),
            [(Place::Herdr(0), "desktop".into(), Some("3".into()))]
        );
        let out = click(&mut v, "Screen of ~/web.sock: HEADLESS-2");
        assert_eq!(
            values(&out),
            [(
                Place::Herdr(0),
                "output".into(),
                Some("\"HEADLESS-2\"".into())
            )]
        );
        let out = click(&mut v, "Remove ~/old.sock");
        assert_eq!(
            values(&out),
            [(Place::Section("agents"), "herdr_socket".into(), None)]
        );
        let out = click(&mut v, "Remove ~/web.sock");
        assert_eq!(places(&out), [Place::Herdr(0)]);
        scroll_to(&mut v, "Journal keeps typed text");
        let out = click(&mut v, "Journal keeps typed text");
        assert_eq!(
            changes(&out),
            [("journal_text".into(), Some("true".into()))]
        );
        scroll_to(&mut v, "Stop all agents");
        let out = click(&mut v, "Stop all agents");
        assert!(
            matches!(out.as_slice(), [Outcome::StopAgents(true)]),
            "{out:?}"
        );
        let mut ctx = v.context().clone();
        ctx.agents_stopped = true;
        v.set_context(ctx);
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Stopped: their requests are refused").is_some());
        scroll_to(&mut v, "Allow agents");
        let out = click(&mut v, "Allow agents");
        assert!(
            matches!(out.as_slice(), [Outcome::StopAgents(false)]),
            "{out:?}"
        );
        // What an agent shows first and what asks first (#229).
        scroll_to(&mut v, "Shows before a click: At once");
        assert!(
            find(&v.access(), "Shows before a click: 0.6 s")
                .expect("it")
                .selected
        );
        let out = click(&mut v, "Shows before a click: At once");
        assert_eq!(
            changes(&out),
            [("show_before_click_ms".into(), Some("0".into()))]
        );
        scroll_to(&mut v, "Waits for your answer: 2 min");
        let out = click(&mut v, "Waits for your answer: 2 min");
        assert_eq!(
            changes(&out),
            [("confirm_timeout_s".into(), Some("120".into()))]
        );
        scroll_to(&mut v, "Remove 1Password");
        let out = click(&mut v, "Remove 1Password");
        let changed = changes(&out);
        let [(key, Some(list))] = changed.as_slice() else {
            panic!("{out:?}");
        };
        assert_eq!(key, "confirm_apps");
        assert!(
            list.contains("\"Bitwarden\"") && !list.contains("1Password"),
            "{list}"
        );
        scroll_to(&mut v, "New application asking first");
        click(&mut v, "New application asking first");
        v.text("my.bank.*");
        let out = v.key(Key::Enter);
        let changed = changes(&out);
        let [(_, Some(list))] = changed.as_slice() else {
            panic!("{out:?}");
        };
        assert!(list.ends_with("\"my.bank.*\"]"), "{list}");
        v.scroll(10_000.0);
        v.draw(Duration::ZERO);
        assert!(
            find(&v.access(), "Keys held: Super+Shift")
                .expect("it")
                .selected
        );
        let out = click(&mut v, "Keys held: Ctrl+Alt");
        assert_eq!(
            changes(&out),
            [("modifiers".into(), Some("\"Ctrl+Alt\"".into()))]
        );
        let out = click(&mut v, "Circle to ask");
        assert_eq!(changes(&out), [("enabled".into(), Some("false".into()))]);
    }

    fn search_view(toml: &str) -> SettingsView {
        let mut v = page_view(toml, Page::Search, &[]);
        let mut ctx = v.context().clone();
        ctx.home = Some("/home/alex".into());
        ctx.search = hocus_api::SearchInfo {
            state: "ready".into(),
            documents: 8,
            never_read: 3,
            last_look_seconds: Some(300),
            ..Default::default()
        };
        v.set_context(ctx);
        v.draw(Duration::ZERO);
        v
    }

    fn type_in(v: &mut SettingsView, field: &str, text: &str) -> Vec<Outcome> {
        click(v, field);
        let node = v
            .tree
            .focused()
            .unwrap_or_else(|| panic!("{field} has no keyboard:\n{}", v.access().dump()));
        for _ in 0..v.tree.field_value(node).map_or(0, |t| t.chars().count()) {
            v.key(Key::Backspace);
        }
        v.text(text);
        let out = v.key(Key::Enter);
        v.draw(Duration::ZERO);
        out
    }

    #[test]
    fn search_s_folders_are_added_unless_never_read_and_its_state_shows() {
        use crate::edit::Place;
        let mut v = search_view("[search]\nfolders = [\"~/Notes\"]\n");
        assert!(find(&v.access(), "8 documents").is_some());
        assert!(find(&v.access(), "Up to date · Last look 5 min ago").is_some());
        assert!(find(&v.access(), "3 never read").is_some());
        let out = click(&mut v, "Search again now");
        assert!(matches!(out.as_slice(), [Outcome::SearchAgain]), "{out:?}");
        let out = click(&mut v, "Search documents");
        assert_eq!(changes(&out), [("documents".into(), Some("false".into()))]);
        // Inside a place never read: not added, and said why.
        let out = type_in(&mut v, "New folder to search", "~/.config/gcloud/work");
        assert!(out.is_empty(), "{out:?}");
        let why = "Not added: ~/.config/gcloud is never read; keys, passwords and browser profiles never are.";
        assert!(find(&v.access(), why).is_some(), "{}", v.access().dump());
        let field = v.fields.node(&Field::NewSearchFolder).expect("the field");
        assert_eq!(v.tree.field_value(field), Some("~/.config/gcloud/work"));
        let out = type_in(&mut v, "New folder to search", "/srv/docs/");
        assert_eq!(
            values(&out),
            [(
                Place::Section("search"),
                "folders".into(),
                Some("[\"~/Notes\", \"/srv/docs\"]".into())
            )]
        );
        let out = type_in(&mut v, "New folder to search", "~/Notes/");
        assert!(out.is_empty());
        assert!(find(&v.access(), "Not added: it is searched already.").is_some());
        // The last one removed: the key goes, the usual folders come back.
        let out = click(&mut v, "Remove ~/Notes");
        assert_eq!(changes(&out), [("folders".into(), None)]);
        let out = click(&mut v, "Screenshots’ text");
        assert_eq!(
            changes(&out),
            [("screenshots".into(), Some("false".into()))]
        );
        let mut ctx = v.context().clone();
        ctx.search.state = "paused".into();
        v.set_context(ctx);
        v.draw(Duration::ZERO);
        assert!(find(&v.access(), "Search again now").expect("it").disabled);
        assert!(
            find(
                &v.access(),
                "Paused on battery: it goes on once on the charger · Last look 5 min ago"
            )
            .is_some()
        );
    }

    #[test]
    fn names_left_out_meaning_and_the_command_field_s_results_are_set() {
        let mut v = search_view("");
        v.scroll(500.0);
        v.draw(Duration::ZERO);
        let out = type_in(&mut v, "Name to leave out", "drafts/old");
        assert!(out.is_empty());
        assert!(find(&v.access(), "Not added: a name, without /.").is_some());
        let out = type_in(&mut v, "Name to leave out", "*.log");
        assert_eq!(
            changes(&out),
            [("exclude".into(), Some("[\"*.log\"]".into()))]
        );
        // Words only for now, and why.
        assert!(
            find(
                &v.access(),
                "The AI is off: documents are found by their words."
            )
            .is_some()
        );
        assert!(
            find(&v.access(), "Search by meaning: On this machine")
                .expect("it")
                .selected
        );
        let out = click(&mut v, "Search by meaning: Wherever the AI is");
        assert_eq!(changes(&out), [("meaning".into(), Some("\"on\"".into()))]);
        with_config(
            &mut v,
            "[search]\nmeaning = \"on\"\n\n[ai]\nenabled = true\nprovider = \"openai\"\n",
            None,
        );
        let sent = "Wherever the AI is: the start of every document (a few thousand characters each), and what is typed in the command field, are sent to its provider, even when it is another company’s server.";
        assert!(find(&v.access(), sent).is_some());
        let out = type_in(&mut v, "Embedding model", "nomic-embed-text");
        assert_eq!(
            changes(&out),
            [(
                "embedding_model".into(),
                Some("\"nomic-embed-text\"".into())
            )]
        );
        with_config(&mut v, "[search]\nmeaning = \"off\"\n", None);
        assert!(find(&v.access(), sent).is_none());
        assert!(find(&v.access(), "Embedding model").is_none());
        v.scroll(2000.0);
        v.draw(Duration::ZERO);
        let out = click(&mut v, "Calculator");
        assert_eq!(changes(&out), [("calculator".into(), Some("false".into()))]);
        let out = type_in(&mut v, "Search engine address", "https://example.org/?q=");
        assert!(out.is_empty());
        assert!(
            find(
                &v.access(),
                "Not written: an http(s) address with {} where the words go."
            )
            .is_some()
        );
        let out = type_in(&mut v, "Search engine address", "https://example.org/?q={}");
        assert!(
            matches!(&out[..], [Outcome::Change(c)] if c.place == crate::edit::Place::Section("launcher")),
            "{out:?}"
        );
        assert_eq!(
            changes(&out),
            [(
                "search_engine".into(),
                Some("\"https://example.org/?q={}\"".into())
            )]
        );
    }
}
