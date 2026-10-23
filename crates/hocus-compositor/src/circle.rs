//! Circle to ask (#46): a left drag with the configured keys held (Super+
//! Shift; Super alone draws on the screen, #86) draws a loop around
//! anything; let go, the menu shows beside it while the area is captured
//! after the turn and its text read on a worker (`[screenshots] ocr`).
//! What is chosen before the text is there waits for it. Copy the text,
//! Note and Search act at once and close it all; Explain, Translate and a
//! question typed (only while the AI is on) stream their answer into a
//! card beside the loop. Escape or a press anywhere else closes it all,
//! and no client sees the press.

use std::path::PathBuf;

use hocus_shell::{
    answer::News as AnswerNews,
    circle::{Act, Asked, Circle, CircleEvent},
};
use smithay::{
    backend::renderer::element::memory::MemoryRenderBufferRenderElement,
    input::keyboard::{Keysym, ModifiersState},
    output::Output,
    utils::{Logical, Point, Rectangle},
};

use crate::{App, State, ai::Asker, grabs::BTN_LEFT, shell::Panel};

impl crate::shell::Pixels for Circle {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

/// The circled area's text, as the worker read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Text {
    Reading,
    Read(String),
    /// Nothing read in it.
    Nothing,
    /// The program that reads text is not there.
    NoReader(String),
}

/// What waits for the text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Wanted {
    Act(Act),
    Ask(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The button is held: the loop follows the pointer.
    Drawing,
    /// Let go: the menu, and maybe the card.
    Open,
}

/// The circle on screen.
#[derive(Debug)]
struct Circled {
    output: String,
    panel: Panel<Circle>,
    phase: Phase,
    reading: Text,
    pending: Option<Wanted>,
    /// The question whose answer the card shows.
    asked: u64,
    /// Which circle it is, for the text read.
    serial: u64,
}

#[derive(Debug, Default)]
pub struct Circles {
    circle: Option<Circled>,
    /// To capture after the turn: the output, its logical area, the
    /// circle's serial.
    wanted: Option<(String, Rectangle<i32, Logical>, u64)>,
    /// While captured, the loop and its menu are left out of the picture.
    hidden: bool,
    next: u64,
    /// The button whose press closed it: its release is not a client's.
    swallow: Option<u32>,
}

impl State {
    /// The menu is open: it takes the keys and the pointer.
    pub fn circle_open(&self) -> bool {
        self.circles
            .circle
            .as_ref()
            .is_some_and(|c| c.phase == Phase::Open)
    }

    /// A button. With the keys held, the left one starts a loop, which its
    /// release closes; while the menu is open, every button is its own.
    pub fn circle_button(&mut self, at: Point<f64, Logical>, button: u32, pressed: bool) -> bool {
        if !pressed && self.circles.swallow == Some(button) {
            self.circles.swallow = None;
            return true;
        }
        if button == BTN_LEFT && pressed && self.circle_starts(at) {
            return true;
        }
        let Some(phase) = self.circles.circle.as_ref().map(|c| c.phase) else {
            return false;
        };
        match phase {
            Phase::Drawing if button == BTN_LEFT && !pressed => {
                self.circle_closed();
                true
            }
            Phase::Drawing => false,
            Phase::Open if button == BTN_LEFT => {
                self.circle_pointer(at, Some(pressed));
                if pressed && self.circles.circle.is_none() {
                    self.circles.swallow = Some(button);
                }
                true
            }
            Phase::Open => {
                if pressed {
                    self.close_circle();
                    self.circles.swallow = Some(button);
                }
                true
            }
        }
    }

    /// A loop starts at `at` when the keys are held.
    fn circle_starts(&mut self, at: Point<f64, Logical>) -> bool {
        let Some(held) = self.config.circle.held() else {
            return false;
        };
        let Some(m) = self.seat.get_keyboard().map(|k| k.modifier_state()) else {
            return false;
        };
        if (m.logo, m.shift, m.ctrl, m.alt) != (held.logo, held.shift, held.ctrl, held.alt) {
            return false;
        }
        if self.locked() || (self.modal_open() && !self.circle_open()) {
            return false;
        }
        let Some(output) = self.output_at(at) else {
            return false;
        };
        let Some(area) = self.space.output_geometry(&output) else {
            return false;
        };
        self.close_circle();
        let ai = self.ai_source().is_some();
        let view = Circle::new(
            self.theme(),
            self.shell_lang(),
            hocus_ui::Size::new(area.size.w as f32, area.size.h as f32),
            output.current_scale().fractional_scale() as f32,
            ai,
        );
        self.circles.next += 1;
        let mut circled = Circled {
            output: output.name(),
            panel: Panel::new(view),
            phase: Phase::Drawing,
            reading: Text::Reading,
            pending: None,
            asked: 0,
            serial: self.circles.next,
        };
        circled.panel.view.line_to(local(at, area.loc));
        self.circles.circle = Some(circled);
        self.needs_redraw = true;
        true
    }

    /// The pointer moved: the loop follows.
    pub fn circle_motion(&mut self, at: Point<f64, Logical>) {
        let Some(origin) = self
            .circles
            .circle
            .as_ref()
            .filter(|c| c.phase == Phase::Drawing)
            .and_then(|c| self.find_output(&c.output))
            .and_then(|o| self.space.output_geometry(&o))
            .map(|g| g.loc)
        else {
            return;
        };
        if let Some(c) = self.circles.circle.as_mut() {
            c.panel.view.line_to(local(at, origin));
            self.needs_redraw = true;
        }
    }

    /// The button let go: the area is read, and the menu shows; a drag too
    /// small circled nothing.
    fn circle_closed(&mut self) {
        let Some(c) = self.circles.circle.as_mut() else {
            return;
        };
        let Some(rect) = c.panel.view.circled() else {
            self.close_circle();
            return;
        };
        c.phase = Phase::Open;
        c.panel.view.open_menu();
        let area = Rectangle::new(
            (rect.x.floor() as i32, rect.y.floor() as i32).into(),
            (rect.w.ceil() as i32, rect.h.ceil() as i32).into(),
        );
        tracing::info!(?area, output = c.output, "circled");
        self.circles.wanted = Some((c.output.clone(), area, c.serial));
        self.needs_redraw = true;
    }

    pub fn close_circle(&mut self) {
        if self.circles.circle.take().is_some() {
            self.circles.wanted = None;
            self.needs_redraw = true;
        }
    }

    /// The pointer over the open menu: its buttons and the card's light
    /// up; no window sees it.
    pub fn circle_pointer_motion(&mut self, at: Point<f64, Logical>) -> bool {
        self.circle_open() && self.circle_pointer(at, None)
    }

    fn circle_pointer(&mut self, at: Point<f64, Logical>, pressed: Option<bool>) -> bool {
        let Some(c) = self.circles.circle.as_ref() else {
            return false;
        };
        let Some(g) = self
            .find_output(&c.output)
            .and_then(|o| self.space.output_geometry(&o))
        else {
            return false;
        };
        // Another screen: a press there closes it.
        if !g.to_f64().contains(at) {
            if pressed == Some(true) {
                self.close_circle();
            }
            return true;
        }
        let p = local(at, g.loc);
        let Some(c) = self.circles.circle.as_mut() else {
            return false;
        };
        c.panel.view.pointer_move(p);
        let event = pressed.and_then(|pressed| c.panel.view.pointer_button(p, pressed));
        if let Some(event) = event {
            self.circle_event(event);
        }
        self.needs_redraw = true;
        true
    }

    /// The wheel over the card scrolls a long answer.
    pub fn circle_wheel(&mut self, dy: f64) -> bool {
        if !self.circle_open() {
            return false;
        }
        let Some(at) = self.seat.get_pointer().map(|p| p.current_location()) else {
            return false;
        };
        let Some(origin) = self
            .circles
            .circle
            .as_ref()
            .and_then(|c| self.find_output(&c.output))
            .and_then(|o| self.space.output_geometry(&o))
            .map(|g| g.loc)
        else {
            return false;
        };
        let Some(c) = self.circles.circle.as_mut() else {
            return false;
        };
        let scrolled = c.panel.view.wheel(local(at, origin), dy as f32);
        self.needs_redraw |= scrolled;
        // Over the menu or the card, the wheel is theirs.
        scrolled || c.panel.view.hit(local(at, origin))
    }

    /// The open menu takes every key: Escape closes it, the rest goes to
    /// its field.
    pub fn circle_key(&mut self, sym: Keysym, modifiers: &ModifiersState) {
        let Some(c) = self.circles.circle.as_mut() else {
            return;
        };
        let event = match crate::launcher::key_of(sym) {
            Some(key) => c.panel.view.key(key),
            None if !modifiers.ctrl && !modifiers.alt && !modifiers.logo => {
                if let Some(ch) = sym.key_char().filter(|ch| !ch.is_control()) {
                    c.panel.view.text(ch.encode_utf8(&mut [0; 4]));
                }
                None
            }
            None => None,
        };
        if let Some(event) = event {
            self.circle_event(event);
        }
        self.needs_redraw = true;
    }

    fn circle_event(&mut self, event: CircleEvent) {
        let lang = self.shell_lang();
        match event {
            CircleEvent::Close => self.close_circle(),
            CircleEvent::Act(act) => self.circle_wants(Wanted::Act(act)),
            CircleEvent::Ask(question) => self.circle_wants(Wanted::Ask(question)),
            CircleEvent::CopyAnswer(text) => {
                self.close_circle();
                self.copy_text(&text);
            }
            CircleEvent::NoteAnswer(text) => {
                self.close_circle();
                self.add_note(&one_line(&text));
                self.shell_toast(lang.added_to_notes(), "");
            }
            CircleEvent::StopAnswer => {
                self.stop_asking();
                if let Some(c) = self.circles.circle.as_mut() {
                    let stopped = lang.answer_stopped().to_owned();
                    c.panel.view.answer_news(AnswerNews::Failed(stopped));
                }
            }
            CircleEvent::Retry(asked) => {
                if let Some(Text::Read(text)) = self.circles.circle.as_ref().map(|c| &c.reading) {
                    let text = text.clone();
                    self.circle_ask(asked, &text);
                }
            }
        }
        self.needs_redraw = true;
    }

    /// Done now when the text is there, once it is otherwise.
    fn circle_wants(&mut self, wanted: Wanted) {
        let lang = self.shell_lang();
        let Some(c) = self.circles.circle.as_mut() else {
            return;
        };
        match c.reading.clone() {
            Text::Read(text) => self.circle_do(wanted, &text),
            Text::Reading => {
                c.pending = Some(wanted);
                c.panel.view.set_note(Some(lang.reading_text()));
            }
            // Said already: nothing to act on.
            Text::Nothing | Text::NoReader(_) => {}
        }
    }

    fn circle_do(&mut self, wanted: Wanted, text: &str) {
        let lang = self.shell_lang();
        match wanted {
            Wanted::Act(Act::Copy) => {
                self.close_circle();
                self.copy_text(text);
                self.shell_toast(lang.text_copied(), "");
            }
            Wanted::Act(Act::Note) => {
                self.close_circle();
                let line = one_line(text);
                self.add_note(&line);
                self.shell_toast(lang.added_to_notes(), &line);
            }
            Wanted::Act(Act::Search) => {
                self.close_circle();
                let words: String = one_line(text).chars().take(300).collect();
                let url = self
                    .config
                    .launcher
                    .search_engine
                    .replace("{}", &hocus_shell::providers::encode(&words));
                tracing::info!(url, "searching the circled text");
                self.launch("xdg-open", &[url]);
            }
            Wanted::Act(Act::Explain) => self.circle_ask(Asked::Explain, text),
            Wanted::Act(Act::Translate) => self.circle_ask(Asked::Translate, text),
            Wanted::Ask(question) => self.circle_ask(Asked::Question(question), text),
        }
    }

    fn circle_ask(&mut self, asked: Asked, text: &str) {
        let Some(source) = self.ai_source() else {
            return;
        };
        let lang = self.shell_lang();
        self.circles.next += 1;
        let id = self.circles.next;
        let Some(c) = self.circles.circle.as_mut() else {
            return;
        };
        c.asked = id;
        c.panel.view.ask(asked.clone(), source);
        let system = asked.instructions(lang);
        let question = asked.text(text);
        self.ask_ai_for(Asker::Circle, id, system, question);
    }

    /// More of an answer, for the card while it waits for it.
    pub fn circle_answer_news(&mut self, id: u64, news: AnswerNews) {
        if let Some(c) = self.circles.circle.as_mut().filter(|c| c.asked == id) {
            c.panel.view.answer_news(news);
        }
    }

    /// The question whose answer the card waits for.
    pub fn circle_awaited(&self) -> Option<u64> {
        self.circles
            .circle
            .as_ref()
            .filter(|c| c.panel.view.streaming())
            .map(|c| c.asked)
    }

    /// The worker read the area's text.
    pub(crate) fn circle_read(&mut self, serial: u64, reading: Text) {
        let lang = self.shell_lang();
        let Some(c) = self.circles.circle.as_mut().filter(|c| c.serial == serial) else {
            return;
        };
        tracing::info!(read = matches!(reading, Text::Read(_)), "circled text");
        c.reading = reading.clone();
        match reading {
            Text::Read(text) => {
                c.panel.view.set_note(None);
                if let Some(wanted) = c.pending.take() {
                    self.circle_do(wanted, &text);
                }
            }
            Text::Nothing => {
                c.pending = None;
                c.panel.view.set_note(Some(lang.no_text_circled()));
            }
            Text::NoReader(program) => {
                c.pending = None;
                c.panel.view.set_note(Some(&lang.no_text_reader(&program)));
            }
            Text::Reading => {}
        }
        self.needs_redraw = true;
    }

    /// After each dispatch: locked, it goes; otherwise it draws what
    /// changed.
    pub fn refresh_circle(&mut self) {
        let gone = self
            .circles
            .circle
            .as_ref()
            .is_some_and(|c| self.find_output(&c.output).is_none());
        if gone || (self.circles.circle.is_some() && self.locked()) {
            self.close_circle();
        }
        let now = self.clock.now();
        let Some(c) = self.circles.circle.as_mut() else {
            return;
        };
        let drawn = c.panel.view.draw(now);
        if c.panel.upload(&drawn) {
            self.needs_redraw = true;
        }
    }

    pub fn circle_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Option<MemoryRenderBufferRenderElement<R>>
    where
        R: smithay::backend::renderer::Renderer + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let c = self.circles.circle.as_ref()?;
        if self.circles.hidden || c.output != output.name() {
            return None;
        }
        c.panel.element(renderer, output, Point::default())
    }

    /// Its menu and card on glass.
    pub(crate) fn circle_glass(&self, output: &Output) -> Vec<crate::blur::Spot> {
        match self.circles.circle.as_ref() {
            Some(c) if c.output == output.name() && !self.circles.hidden => crate::blur::spots(
                crate::blur::Surface::Circle,
                Default::default(),
                &c.panel.view.glass(),
            ),
            _ => Vec::new(),
        }
    }

    pub fn circle_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        let Some(c) = &self.circles.circle else {
            return;
        };
        if let Some(g) = self
            .find_output(&c.output)
            .and_then(|o| self.space.output_geometry(&o))
        {
            crate::shell::flatten(&c.panel.view.access(), g.loc, 0, out);
        }
    }
}

impl App {
    /// After a dispatch: captures the circled area, without the loop, and
    /// reads its text on a worker.
    pub fn serve_circle(&mut self) {
        let Some((name, area, serial)) = self.state.circles.wanted.take() else {
            return;
        };
        let Some(send) = self.state.circle_reader() else {
            return;
        };
        let Some(output) = self.state.find_output(&name) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        self.state.circles.hidden = true;
        let whole = self.capture_output(&output);
        self.state.circles.hidden = false;
        let whole = match whole {
            Ok(whole) => whole,
            Err(err) => {
                tracing::warn!("could not capture the circled area: {err:#}");
                send(serial, Text::Nothing);
                return;
            }
        };
        let p = area.to_f64().to_physical(scale);
        let part = crate::screenshot::crop(
            &whole,
            p.loc.x.round() as u32,
            p.loc.y.round() as u32,
            p.size.w.round() as u32,
            p.size.h.round() as u32,
        );
        let program = self.state.config.screenshots.ocr.clone();
        let file = self
            .state
            .socket_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "hocuspanda-circle-{}-{serial}.png",
                std::process::id()
            ));
        crate::screenshot::spawn("circle text", move || {
            send(serial, read(&program, &part, &file));
        });
    }
}

/// The text in `part`, written to `file` for the program that reads it,
/// then removed.
fn read(program: &str, part: &crate::capture::Pixels, file: &std::path::Path) -> Text {
    if !crate::screenshot::on_path(program) {
        return Text::NoReader(program.to_owned());
    }
    let png = match part.encode_png() {
        Ok(png) => png,
        Err(err) => {
            tracing::warn!("could not encode the circled area: {err:#}");
            return Text::Nothing;
        }
    };
    if let Err(err) = std::fs::write(file, &png) {
        tracing::warn!(%err, file = %file.display(), "could not write the circled area");
        return Text::Nothing;
    }
    let text = crate::screenshot::read_text(program, file);
    if let Err(err) = std::fs::remove_file(file) {
        tracing::warn!(%err, file = %file.display(), "could not remove the circled area");
    }
    match text {
        Some(text) => Text::Read(text),
        None => Text::Nothing,
    }
}

/// `at` from an output's corner.
fn local(at: Point<f64, Logical>, origin: Point<i32, Logical>) -> hocus_ui::Point {
    hocus_ui::Point::new(
        (at.x - f64::from(origin.x)) as f32,
        (at.y - f64::from(origin.y)) as f32,
    )
}

/// Lines and runs of spaces as single spaces: a line of the notes.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
