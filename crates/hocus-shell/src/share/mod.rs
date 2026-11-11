//! Screen sharing's chooser (5.11, variant A, docs/design/sharing.md): an
//! application asks to see the screen through the desktop portal, which
//! the compositor serves; it says so here (`share-choose`) and the shell
//! shows what can be shared, screens or windows, the asking application
//! at its head. The compositor darkens the screen under it, pops it in,
//! draws its shadow and the live thumbnails where the shell says they go
//! (`share thumbs`); the shell says what was chosen and whether the
//! pointer shows (`share chosen`), or that it was cancelled.
//!
//! The chooser's state is here; its surface is the widget kit's
//! ([`view`]): what its widgets ask is handled at once, and it alone draws
//! again.

pub mod view;

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{
    Rect, SHARE_CHOOSER_NAMESPACE, SHARE_MONITOR, SHARE_WINDOW, ShareAsk, ShareChoose, ShareSource,
    ToCompositor,
};
use iced::{keyboard, window};

use crate::{clock::time::Lang, launcher::entries::Entry, popover::Effect};

/// Two clicks on a card this close share it at once.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// The chooser's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    /// "Firefox veut voir ton écran", the application's name known or not.
    pub fn asks(self, app: Option<&str>) -> String {
        match (app, self.fr) {
            (Some(app), true) => format!("{app} veut voir ton écran"),
            (Some(app), false) => format!("{app} wants to see your screen"),
            (None, true) => "Une application veut voir ton écran".to_owned(),
            (None, false) => "An application wants to see your screen".to_owned(),
        }
    }

    /// What it says of itself, then "choisis ce que tu partages".
    pub fn choose(self, detail: &str) -> String {
        let choose = self.pick("choisis ce que tu partages", "choose what to share");
        if detail.is_empty() {
            choose.to_owned()
        } else {
            format!("{detail} · {choose}")
        }
    }

    pub fn tab(self, tab: Tab) -> &'static str {
        match tab {
            Tab::Screens => self.pick("Écrans", "Screens"),
            Tab::Windows => self.pick("Fenêtres", "Windows"),
        }
    }

    /// A screen's name: the laptop's, its model's, or an external one.
    pub fn screen(self, builtin: bool, model: &str) -> String {
        if builtin {
            self.pick("Écran du portable", "Laptop screen").to_owned()
        } else if model.is_empty() {
            self.pick("Écran externe", "External screen").to_owned()
        } else {
            model.to_owned()
        }
    }

    pub fn size(self, width: u32, height: u32) -> String {
        format!("{width} × {height}")
    }

    /// A window's desktop: this one, or its number.
    pub fn desktop(self, here: bool, desktop: u8) -> String {
        match (here, self.fr) {
            (true, true) => "ce bureau".to_owned(),
            (true, false) => "this desktop".to_owned(),
            (false, true) => format!("bureau {desktop}"),
            (false, false) => format!("desktop {desktop}"),
        }
    }

    pub fn pointer(self) -> &'static str {
        self.pick("Montrer le pointeur", "Show the pointer")
    }

    pub fn cancel(self) -> &'static str {
        self.pick("Annuler", "Cancel")
    }

    pub fn share(self) -> &'static str {
        self.pick("Partager", "Share")
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// The application of id `app` among those the launcher lists: by its
/// id, its id's last part (`org.mozilla.firefox`, `firefox`), or its name.
pub fn entry_of(app: &str) -> Option<Entry> {
    if app.is_empty() {
        return None;
    }
    let last = |s: &str| s.rsplit('.').next().unwrap_or(s).to_lowercase();
    let wanted = last(app);
    let catalog = crate::launcher::apps::catalog();
    let entries = || catalog.apps.iter().map(|a| &a.entry);
    entries()
        .find(|e| e.id.eq_ignore_ascii_case(app))
        .or_else(|| entries().find(|e| last(&e.id) == wanted))
        .or_else(|| entries().find(|e| e.name.eq_ignore_ascii_case(app)))
        .cloned()
}

/// The name people know application `app` by: its entry's, else its id's
/// last part with a capital (none: not known).
pub fn app_name(app: &str) -> Option<String> {
    if let Some(entry) = entry_of(app) {
        return Some(entry.name);
    }
    let last = app.rsplit('.').next().unwrap_or(app).trim();
    let mut letters = last.chars();
    let first = letters.next()?;
    Some(first.to_uppercase().chain(letters).collect())
}

/// What the chooser lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Screens,
    Windows,
}

/// The chooser shown.
#[derive(Debug, Clone)]
pub struct Chooser {
    pub id: window::Id,
    pub choose: ShareChoose,
    pub tab: Tab,
    pub selected: usize,
    pub pointer: bool,
    /// The first row in view.
    pub scroll: usize,
    last_click: Option<(usize, Instant)>,
    /// The thumbnails' places as last told, and the surface's size.
    told: Option<Vec<(ShareSource, Rect)>>,
    pub size: (u32, u32),
}

impl Chooser {
    /// The tabs it shows: those its request allows.
    pub fn tabs(&self) -> Vec<Tab> {
        let mut tabs = Vec::new();
        if self.choose.types & SHARE_MONITOR != 0 {
            tabs.push(Tab::Screens);
        }
        if self.choose.types & SHARE_WINDOW != 0 {
            tabs.push(Tab::Windows);
        }
        tabs
    }

    /// What the tab shown lists, in order.
    pub fn sources(&self) -> Vec<ShareSource> {
        match self.tab {
            Tab::Screens => self
                .choose
                .screens
                .iter()
                .map(|s| ShareSource::Screen(s.name.clone()))
                .collect(),
            Tab::Windows => self
                .choose
                .windows
                .iter()
                .map(|w| ShareSource::Window(w.id))
                .collect(),
        }
    }

    fn count(&self) -> usize {
        match self.tab {
            Tab::Screens => self.choose.screens.len(),
            Tab::Windows => self.choose.windows.len(),
        }
    }

    /// The selection moved by `by` (a column, or a row of three), kept in
    /// view.
    fn step(&mut self, by: isize) {
        let n = self.count();
        if n == 0 {
            return;
        }
        let at = (self.selected as isize + by).clamp(0, n as isize - 1);
        self.selected = at as usize;
        self.keep_in_view();
    }

    fn keep_in_view(&mut self) {
        let rows = view::Layout::of(self).rows_shown.max(1);
        let row = self.selected / view::COLUMNS;
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + rows {
            self.scroll = row + 1 - rows;
        }
    }

    fn switch(&mut self, tab: Tab) {
        if self.tab != tab && self.tabs().contains(&tab) {
            self.tab = tab;
            self.selected = 0;
            self.scroll = 0;
        }
    }

    /// The source selected, if any.
    fn chosen(&self) -> Option<ShareSource> {
        self.sources().get(self.selected).cloned()
    }
}

static CHOOSER: Mutex<Option<Chooser>> = Mutex::new(None);

/// `with` on the chooser, if one shows.
pub fn with<T>(with: impl FnOnce(&mut Option<Chooser>) -> T) -> T {
    with(&mut CHOOSER.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Whether surface `id` is the chooser's.
pub fn is_surface(id: window::Id) -> bool {
    with(|c| c.as_ref().is_some_and(|c| c.id == id))
}

/// Its surface is built again and draws (an icon came).
pub fn wake() {
    if with(|c| c.is_some()) {
        crate::ui::touch();
        crate::popover::send(vec![Effect::Wake(SHARE_CHOOSER_NAMESPACE)]);
    }
}

/// The compositor's `share-choose`: the chooser opens for that request,
/// or lists its sources anew.
pub fn set(choose: ShareChoose) {
    let mut opened = false;
    let effects = with(|chooser| {
        let mut effects = Vec::new();
        match chooser {
            Some(shown) if shown.choose.request == choose.request => {
                // The same selection, if it is still there.
                let kept = shown.chosen();
                shown.choose = choose;
                shown.selected = kept
                    .and_then(|k| shown.sources().iter().position(|s| *s == k))
                    .unwrap_or(0);
                shown.keep_in_view();
                let size = view::Layout::of(shown).surface();
                if size != shown.size {
                    shown.size = size;
                    effects.push(Effect::Resize {
                        id: shown.id,
                        size,
                        margin: None,
                    });
                }
                effects.push(Effect::Wake(SHARE_CHOOSER_NAMESPACE));
                effects.extend(thumbs(shown));
            }
            _ => {
                if let Some(old) = chooser.take() {
                    effects.push(Effect::Remove(old.id));
                }
                let tab = if choose.types & SHARE_WINDOW != 0 {
                    Tab::Windows
                } else {
                    Tab::Screens
                };
                let mut shown = Chooser {
                    id: window::Id::unique(),
                    pointer: choose.pointer,
                    choose,
                    tab,
                    selected: 0,
                    scroll: 0,
                    last_click: None,
                    told: None,
                    size: (0, 0),
                };
                shown.size = view::Layout::of(&shown).surface();
                let app = shown.choose.app.clone();
                let icon = entry_of(&app).and_then(|e| e.icon);
                if let Some(icon) = &icon {
                    crate::launcher::icons::want(
                        [icon.as_str()],
                        view::ICON_PX,
                        shown.choose.scale as f32 / 120.0,
                    );
                }
                tracing::debug!(
                    request = shown.choose.request,
                    output = shown.choose.output,
                    app,
                    screens = shown.choose.screens.len(),
                    windows = shown.choose.windows.len(),
                    "the share's chooser opens"
                );
                effects.push(Effect::ShareChooser {
                    id: shown.id,
                    output: shown.choose.output.clone(),
                    size: shown.size,
                });
                effects.extend(thumbs(&mut shown));
                *chooser = Some(shown);
                opened = true;
            }
        }
        effects
    });
    if opened {
        // One at a time: the bar's popover, the launcher, the picker close.
        crate::popover::event(crate::popover::Event::PressElsewhere);
        crate::launcher::close();
        crate::emoji::close();
        crate::actions::close();
    }
    crate::popover::send(effects);
}

/// `share-choose-end`: the chooser of that request goes.
pub fn end(request: u64) {
    let effects = with(|chooser| match chooser {
        Some(shown) if shown.choose.request == request => {
            let id = shown.id;
            *chooser = None;
            vec![Effect::Remove(id)]
        }
        _ => Vec::new(),
    });
    if !effects.is_empty() {
        tracing::debug!(request, "the share's chooser goes");
        crate::popover::send(effects);
    }
}

/// The thumbnails' places, when they changed: to the compositor.
fn thumbs(chooser: &mut Chooser) -> Option<Effect> {
    let layout = view::Layout::of(chooser);
    let sources = chooser.sources();
    let rects: Vec<(ShareSource, Rect)> = layout
        .cards
        .iter()
        .filter_map(|card| {
            let source = sources.get(card.index)?.clone();
            let r = card.thumb;
            // Inside its 1 px rim.
            Some((
                source,
                Rect {
                    x: (r.x + 1.0).round() as i32,
                    y: (r.y + 1.0).round() as i32,
                    width: (r.width - 2.0).round().max(1.0) as i32,
                    height: (r.height - 2.0).round().max(1.0) as i32,
                },
            ))
        })
        .collect();
    if chooser.told.as_ref() == Some(&rects) {
        return None;
    }
    chooser.told = Some(rects.clone());
    Some(Effect::Tell(ToCompositor::Share(ShareAsk::Thumbs {
        request: chooser.choose.request,
        rects,
    })))
}

/// What sharing `chooser`'s selection says, and its surface going.
fn share(chooser: &Chooser) -> Vec<Effect> {
    let Some(source) = chooser.chosen() else {
        return Vec::new();
    };
    tracing::info!(request = chooser.choose.request, %source, pointer = chooser.pointer, "shared");
    vec![
        Effect::Tell(ToCompositor::Share(ShareAsk::Chosen {
            request: chooser.choose.request,
            source,
            pointer: chooser.pointer,
        })),
        Effect::Remove(chooser.id),
    ]
}

fn cancel(chooser: &Chooser) -> Vec<Effect> {
    tracing::info!(request = chooser.choose.request, "sharing cancelled");
    vec![
        Effect::Tell(ToCompositor::Share(ShareAsk::Cancel {
            request: chooser.choose.request,
        })),
        Effect::Remove(chooser.id),
    ]
}

/// What was done on the chooser on surface `id`, at `now`.
pub fn act(id: window::Id, act: Act, now: Instant) {
    let effects = with(|slot| {
        let chooser = slot.as_mut().filter(|c| c.id == id)?;
        let mut effects = Vec::new();
        let mut done = false;
        match act {
            Act::Tab(tab) => {
                chooser.switch(tab);
                effects.extend(thumbs(chooser));
            }
            Act::Card(index) => {
                let double = chooser.last_click.is_some_and(|(at, when)| {
                    at == index && now.saturating_duration_since(when) < DOUBLE_CLICK
                });
                chooser.selected = index;
                chooser.last_click = Some((index, now));
                if double {
                    effects.extend(share(chooser));
                    done = true;
                }
            }
            Act::Pointer => chooser.pointer = !chooser.pointer,
            Act::Cancel => {
                effects.extend(cancel(chooser));
                done = true;
            }
            Act::Share => {
                effects.extend(share(chooser));
                done = true;
            }
            Act::Wheel(rows) => {
                let layout = view::Layout::of(chooser);
                let most = layout.rows.saturating_sub(layout.rows_shown);
                let scroll = (chooser.scroll as isize + rows).clamp(0, most as isize) as usize;
                if scroll == chooser.scroll {
                    return None;
                }
                chooser.scroll = scroll;
                effects.extend(thumbs(chooser));
            }
        }
        Some((effects, done))
    });
    let Some((effects, done)) = effects else {
        return;
    };
    if done {
        with(|slot| *slot = None);
    }
    crate::popover::send(effects);
}

/// What can be done on the chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Tab(Tab),
    /// A card clicked: selected, shared on a second click.
    Card(usize),
    Pointer,
    Cancel,
    Share,
    /// The wheel, by rows.
    Wheel(isize),
}

/// `key` on surface `id`: whether the chooser took it.
pub fn key_on(id: window::Id, key: &keyboard::Key) -> bool {
    use keyboard::{Key, key::Named};
    let effects = with(|slot| {
        let chooser = slot.as_mut().filter(|c| c.id == id)?;
        let mut effects = Vec::new();
        let mut done = false;
        match key {
            Key::Named(Named::Escape) => {
                effects.extend(cancel(chooser));
                done = true;
            }
            Key::Named(Named::Enter) => {
                effects.extend(share(chooser));
                done = !effects.is_empty();
            }
            Key::Named(Named::ArrowLeft) => chooser.step(-1),
            Key::Named(Named::ArrowRight) => chooser.step(1),
            Key::Named(Named::ArrowUp) => chooser.step(-(view::COLUMNS as isize)),
            Key::Named(Named::ArrowDown) => chooser.step(view::COLUMNS as isize),
            // The other tab.
            Key::Named(Named::Tab) => {
                let other = match chooser.tab {
                    Tab::Screens => Tab::Windows,
                    Tab::Windows => Tab::Screens,
                };
                chooser.switch(other);
            }
            Key::Named(Named::Space) => chooser.pointer = !chooser.pointer,
            _ => return None,
        }
        if !done {
            effects.extend(thumbs(chooser));
        }
        Some((effects, done))
    });
    let Some((effects, done)) = effects else {
        return false;
    };
    if done {
        with(|slot| *slot = None);
    }
    crate::popover::send(effects);
    true
}

/// The chooser for screen readers: a dialog named by what is asked,
/// holding what its controls declared (the tabs, the cards, the pointer's
/// box, the buttons), the selected card holding the focus.
pub fn a11y() -> Option<hocus_a11y::Window> {
    use hocus_a11y::{Node, Role, Window};
    let words = words();
    with(|slot| {
        let chooser = slot.as_ref()?;
        let rows = crate::a11y::rows::of(chooser.id)?;
        let title = words.asks(app_name(&chooser.choose.app).as_deref());
        let dialog = Node::new("dialog", Role::Dialog, title.clone())
            .describe(words.choose(&chooser.choose.detail))
            .children(rows.nodes);
        let mut window = Window::new("share", title);
        window.nodes = vec![dialog];
        window.active = true;
        window.focus = Some(format!("share:Card({})", chooser.selected));
        window.size = Some((chooser.size.0 as f32, chooser.size.1 as f32));
        Some(window)
    })
}

/// A screen reader clicks control `key` of the chooser.
pub fn a11y_click(key: &str) {
    let Some(id) = with(|slot| slot.as_ref().map(|c| c.id)) else {
        return;
    };
    let what = match key {
        "share:Pointer" => Act::Pointer,
        "share:Cancel" => Act::Cancel,
        "share:Share" => Act::Share,
        "share:Tab(Screens)" => Act::Tab(Tab::Screens),
        "share:Tab(Windows)" => Act::Tab(Tab::Windows),
        _ => match key
            .strip_prefix("share:Card(")
            .and_then(|k| k.strip_suffix(')'))
            .and_then(|n| n.parse().ok())
        {
            Some(index) => Act::Card(index),
            None => return,
        },
    };
    act(id, what, Instant::now());
    wake();
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_link::{ShareScreen, ShareWindow};

    fn chooser(types: u32, windows: usize) -> Chooser {
        let choose = ShareChoose {
            request: 1,
            output: "eDP-1".into(),
            scale: 120,
            width: 2560,
            height: 1600,
            types,
            pointer: true,
            app: "firefox".into(),
            detail: String::new(),
            screens: vec![ShareScreen {
                name: "eDP-1".into(),
                width: 2560,
                height: 1600,
                builtin: true,
                model: String::new(),
            }],
            windows: (0..windows)
                .map(|i| ShareWindow {
                    id: i as u64 + 1,
                    desktop: 1,
                    here: true,
                    minimized: false,
                    app: "kitty".into(),
                    title: format!("window {i}"),
                })
                .collect(),
        };
        Chooser {
            id: window::Id::unique(),
            pointer: true,
            choose,
            tab: Tab::Windows,
            selected: 0,
            scroll: 0,
            last_click: None,
            told: None,
            size: (0, 0),
        }
    }

    #[test]
    fn its_words() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.asks(Some("Firefox")), "Firefox veut voir ton écran");
        assert_eq!(en.asks(None), "An application wants to see your screen");
        assert_eq!(
            fr.choose("meet.google.com"),
            "meet.google.com · choisis ce que tu partages"
        );
        assert_eq!(en.choose(""), "choose what to share");
        assert_eq!(fr.desktop(true, 1), "ce bureau");
        assert_eq!(fr.desktop(false, 2), "bureau 2");
        assert_eq!(en.desktop(false, 2), "desktop 2");
        assert_eq!(fr.screen(true, ""), "Écran du portable");
        assert_eq!(en.screen(false, ""), "External screen");
        assert_eq!(en.screen(false, "DELL U3421WE"), "DELL U3421WE");
        assert_eq!(fr.size(2560, 1600), "2560 × 1600");
        assert_eq!(app_name("org.mozilla.firefox").as_deref(), Some("Firefox"));
        assert_eq!(app_name(""), None);
    }

    #[test]
    fn the_arrows_move_in_the_grid_and_the_tabs_follow_the_request() {
        let mut c = chooser(SHARE_WINDOW, 7);
        assert_eq!(c.tabs(), [Tab::Windows]);
        c.step(1);
        assert_eq!(c.selected, 1);
        c.step(view::COLUMNS as isize);
        assert_eq!(c.selected, 4);
        c.step(100);
        assert_eq!(c.selected, 6);
        // Only the tabs asked for.
        c.switch(Tab::Screens);
        assert_eq!(c.tab, Tab::Windows);
        let mut both = chooser(SHARE_MONITOR | SHARE_WINDOW, 2);
        both.switch(Tab::Screens);
        assert_eq!(both.tab, Tab::Screens);
        assert_eq!(both.chosen(), Some(ShareSource::Screen("eDP-1".into())));
    }
}
