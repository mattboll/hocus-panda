//! Screen sharing's chooser (5.11, variant A, docs/design/sharing.md): an
//! application asks to see the screen through the desktop portal, which
//! the compositor serves; it says so here (`share-choose`) and the shell
//! shows what can be shared, screens or windows, the asking application
//! at its head. The compositor darkens the screen under it, pops it in,
//! draws its shadow and the live thumbnails where the shell says they go
//! (`share thumbs`); the shell says what was chosen and whether the
//! pointer shows (`share chosen`), or that it was cancelled.
//!
//! The chooser's state is [`Share`], the shell's own (#773); its surface
//! is the widget kit's ([`view`]): what its widgets ask is handled at
//! once, and it alone draws again.

pub mod view;

use hocus_words::shell::share as txt;
use std::time::{Duration, Instant};

use hocus_link::{
    Rect, SHARE_CHOOSER_NAMESPACE, SHARE_MONITOR, SHARE_WINDOW, ShareAsk, ShareChoose, ShareSource,
    ToCompositor,
};
use iced::{Element, keyboard, window};

use crate::{
    clock::time::Lang,
    launcher::entries::Entry,
    popover::Effect,
    surfaces::{Asked, Context, Lent, Surface},
};

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

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(if self.fr {
            hocus_words::Lang::French
        } else {
            hocus_words::Lang::English
        })
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
        let choose = self.pick(txt::CHOOSE_WHAT_TO_SHARE);
        if detail.is_empty() {
            choose.to_owned()
        } else {
            format!("{detail} · {choose}")
        }
    }

    pub fn tab(self, tab: Tab) -> &'static str {
        match tab {
            Tab::Screens => self.pick(txt::SCREENS),
            Tab::Windows => self.pick(txt::WINDOWS),
        }
    }

    /// A screen's name: the laptop's, its model's, or an external one.
    pub fn screen(self, builtin: bool, model: &str) -> String {
        if builtin {
            self.pick(txt::LAPTOP_SCREEN).to_owned()
        } else if model.is_empty() {
            self.pick(txt::EXTERNAL_SCREEN).to_owned()
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
        self.pick(txt::SHOW_THE_POINTER)
    }

    pub fn cancel(self) -> &'static str {
        self.pick(txt::CANCEL)
    }

    pub fn share(self) -> &'static str {
        self.pick(txt::SHARE)
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

/// The chooser, while one shows, and what its view last said in the log.
#[derive(Debug, Default)]
pub struct Share {
    chooser: Option<Chooser>,
    said: Option<Vec<String>>,
}

/// What changes the chooser.
#[derive(Debug, Clone)]
pub enum Said {
    /// The compositor's `share-choose`: the chooser opens for that
    /// request, or lists its sources anew.
    Choose(ShareChoose),
    /// `share-choose-end`: the chooser of that request goes.
    End(u64),
    /// Done on the chooser on surface `on` (`None`: the one showing, as a
    /// screen reader asks).
    Act { on: Option<window::Id>, act: Act },
    /// A key on surface `id`.
    Key(window::Id, keyboard::Key),
}

/// What screen readers are told of the chooser ([`a11y`]).
#[derive(Debug, Clone)]
pub struct Seen {
    id: window::Id,
    app: String,
    detail: String,
    selected: usize,
    size: (u32, u32),
}

/// Its surface is built again and draws (an icon came). Nothing shows
/// when no chooser does.
pub fn wake() {
    crate::ui::touch();
    crate::popover::send(vec![Effect::Wake(SHARE_CHOOSER_NAMESPACE)]);
}

impl Share {
    /// Whether surface `id` is the chooser's.
    pub fn is_surface(&self, id: window::Id) -> bool {
        self.chooser.as_ref().is_some_and(|c| c.id == id)
    }

    /// The chooser on surface `id`, if it shows there.
    pub fn chooser_on(&self, id: window::Id) -> Option<&Chooser> {
        self.chooser.as_ref().filter(|c| c.id == id)
    }

    fn seen(&self) -> Option<Seen> {
        self.chooser.as_ref().map(|c| Seen {
            id: c.id,
            app: c.choose.app.clone(),
            detail: c.choose.detail.clone(),
            selected: c.selected,
            size: c.size,
        })
    }

    /// `said` taken in at `now`: what the shell then does.
    pub fn update(&mut self, said: Said, now: Instant) -> Vec<Effect> {
        let effects = match said {
            Said::Choose(choose) => self.choose(choose),
            Said::End(request) => self.end(request),
            Said::Act { on, act } => self.act(on, act, now),
            Said::Key(id, key) => self.key_on(id, &key).unwrap_or_default(),
        };
        crate::a11y::shown(crate::a11y::Shown::Share(self.seen()));
        effects
    }

    fn choose(&mut self, choose: ShareChoose) -> Vec<Effect> {
        let mut effects = Vec::new();
        match &mut self.chooser {
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
                if let Some(old) = self.chooser.take() {
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
                self.chooser = Some(shown);
                // One at a time: the bar's popover, the launcher, the
                // picker close.
                crate::popover::event(crate::popover::Event::PressElsewhere);
                crate::launcher::close();
                crate::emoji::close();
                crate::actions::close();
            }
        }
        effects
    }

    fn end(&mut self, request: u64) -> Vec<Effect> {
        match &self.chooser {
            Some(shown) if shown.choose.request == request => {
                tracing::debug!(request, "the share's chooser goes");
                let id = shown.id;
                self.chooser = None;
                vec![Effect::Remove(id)]
            }
            _ => Vec::new(),
        }
    }

    /// What was done on the chooser on surface `on`, at `now`.
    fn act(&mut self, on: Option<window::Id>, act: Act, now: Instant) -> Vec<Effect> {
        let Some(chooser) = self
            .chooser
            .as_mut()
            .filter(|c| on.is_none_or(|id| c.id == id))
        else {
            return Vec::new();
        };
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
                    return Vec::new();
                }
                chooser.scroll = scroll;
                effects.extend(thumbs(chooser));
            }
        }
        if done {
            self.chooser = None;
        }
        effects
    }

    /// `key` on surface `id`: what it does, if the chooser took it.
    fn key_on(&mut self, id: window::Id, key: &keyboard::Key) -> Option<Vec<Effect>> {
        use keyboard::{Key, key::Named};
        let chooser = self.chooser.as_mut().filter(|c| c.id == id)?;
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
        if done {
            self.chooser = None;
        } else {
            effects.extend(thumbs(chooser));
        }
        Some(effects)
    }
}

impl Surface for Lent<Share> {
    type Message = Said;

    fn update(&mut self, said: Said, now: Instant) -> Vec<Effect> {
        self.get_mut().update(said, now)
    }

    fn view(&self, id: window::Id, context: &Context) -> Option<Element<'_, crate::Message>> {
        self.read(|s| s.is_surface(id))?
            .then(|| view::view(self, id, words(), context.look.glass))
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

/// The chooser for screen readers: a dialog named by what is asked,
/// holding what its controls declared (the tabs, the cards, the pointer's
/// box, the buttons), the selected card holding the focus.
pub fn a11y(seen: &Seen) -> Option<hocus_a11y::Window> {
    use hocus_a11y::{Node, Role, Window};
    let words = words();
    let rows = crate::a11y::rows::of(seen.id)?;
    let title = words.asks(app_name(&seen.app).as_deref());
    let dialog = Node::new("dialog", Role::Dialog, title.clone())
        .describe(words.choose(&seen.detail))
        .children(rows.nodes);
    let mut window = Window::new("share", title);
    window.nodes = vec![dialog];
    window.active = true;
    window.focus = Some(format!("share:Card({})", seen.selected));
    window.size = Some((seen.size.0 as f32, seen.size.1 as f32));
    Some(window)
}

/// A screen reader clicks control `key` of the chooser: asked of the
/// loop, which owns it.
pub fn a11y_click(key: &str) {
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
    crate::surfaces::ask(Asked::Share(what));
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_link::{ShareScreen, ShareWindow};

    fn chooser(types: u32, windows: usize) -> Chooser {
        Chooser {
            id: window::Id::unique(),
            pointer: true,
            choose: choose(types, windows),
            tab: Tab::Windows,
            selected: 0,
            scroll: 0,
            last_click: None,
            told: None,
            size: (0, 0),
        }
    }

    fn choose(types: u32, windows: usize) -> ShareChoose {
        ShareChoose {
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

    /// The chooser alone (#773): told, it opens its surface; the keys move
    /// its selection, which its view shows; `Enter` shares it and the
    /// chooser goes.
    #[test]
    fn the_chooser_alone_opens_moves_shares_and_goes() {
        let mut share = Share::default();
        let now = Instant::now();
        let effects = share.update(Said::Choose(choose(SHARE_WINDOW, 4)), now);
        let Some(Effect::ShareChooser { id, output, .. }) = effects
            .iter()
            .find(|e| matches!(e, Effect::ShareChooser { .. }))
        else {
            panic!("its surface opened: {effects:?}");
        };
        let id = *id;
        assert_eq!(output, "eDP-1");
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Tell(ToCompositor::Share(ShareAsk::Thumbs { request: 1, .. }))
        )));
        let words = Words::new(Lang::English);
        let shows = |share: &Share| view::content(share.chooser_on(id).expect("shown"), words).1;
        let before = shows(&share);
        // A key on another surface is not its own.
        let other = window::Id::unique();
        let right = keyboard::Key::Named(keyboard::key::Named::ArrowRight);
        assert!(
            share
                .update(Said::Key(other, right.clone()), now)
                .is_empty()
        );
        share.update(Said::Key(id, right), now);
        assert_eq!(share.chooser_on(id).map(|c| c.selected), Some(1));
        assert_ne!(shows(&share), before, "its view shows the selection moved");
        let enter = keyboard::Key::Named(keyboard::key::Named::Enter);
        let effects = share.update(Said::Key(id, enter), now);
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Tell(ToCompositor::Share(ShareAsk::Chosen {
                source: ShareSource::Window(2),
                ..
            }))
        )));
        assert!(effects.contains(&Effect::Remove(id)));
        assert!(!share.is_surface(id));
        // Ended by the compositor once gone: nothing more.
        assert!(share.update(Said::End(1), now).is_empty());
    }
}
