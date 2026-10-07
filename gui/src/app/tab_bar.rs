//! The window's tabs: opening and closing them, switching (`Ctrl+Tab`,
//! a click on the bar), their titles, and the bar's colors -- the shown
//! tab's theme, as the rest of the window.

use std::io;

use alacritty_terminal::event::Event;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color as TermColor, NamedColor, Rgb};
use winit::event::MouseButton;
use winit::event_loop::EventLoopProxy;

use crate::colors;
use crate::input::{self, TabChord};
use crate::render;
use crate::session::{EventProxy, Session, UserEvent};
use crate::tabs::{self, BarPart, Tab, TabId, Tabs};
use crate::window_style;

use super::{App, Shown};

/// A tab's title before its program names itself.
pub(super) const DEFAULT_TITLE: &str = "litastum";


/// Starts a litastum in a new tab, right after the shown one, and shows it.
pub(super) fn spawn_tab(shown: &mut Shown, proxy: &EventLoopProxy<UserEvent>) -> io::Result<()> {
    let id = shown.tabs.new_id();
    let cell = (shown.font.cell_width, shown.font.cell_height);
    let session = Session::spawn(&shown.program, &shown.working_directory, shown.grid, cell, EventProxy { proxy: proxy.clone(), tab: id })?;
    shown.tabs.add(Tab { id, session, title: DEFAULT_TITLE.to_string() });
    Ok(())
}


/// The tab bar as cells in their colors, and the bar's background.
pub(super) struct Bar {
    pub cells: Vec<(char, Rgb, Rgb)>,
    pub background: Rgb,
}

/// The shown tab in the terminal's own background, the others and the
/// rest of the bar a shade off it, their titles dimmed.
pub(super) fn cells(tabs: &Tabs<Session>, columns: usize, term_colors: &Colors) -> Bar {
    let background = colors::resolve(TermColor::Named(NamedColor::Background), term_colors);
    let text = colors::resolve(TermColor::Named(NamedColor::Foreground), term_colors);
    let bar = render::mix(background, text, 24);
    let dim = render::mix(bar, text, 150);
    let titles: Vec<&str> = tabs.iter().map(|tab| tab.title.as_str()).collect();
    let active = tabs.active_index();
    let cells = tabs::bar(&titles, columns)
        .into_iter()
        .map(|(c, part)| match part {
            BarPart::Tab(index) if index == active => (c, text, background),
            BarPart::Close(index) if index == active => (c, dim, background),
            BarPart::Tab(_) | BarPart::Close(_) => (c, dim, bar),
            BarPart::New | BarPart::Empty => (c, text, bar),
        })
        .collect();
    Bar { cells, background: bar }
}


impl App {
    /// A terminal event from tab `id`. A background tab's output just
    /// waits for its turn to be drawn; its replies and title still go to
    /// it, and its program ending closes it.
    pub(super) fn term_event(&mut self, id: TabId, event: Event) {
        let active = self.shown.as_ref().and_then(|shown| shown.tabs.active()).map(|tab| tab.id) == Some(id);
        match event {
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange if active => {
                self.sync_frame_colors();
                self.request_redraw();
            }
            Event::Title(title) => self.set_title(id, title),
            Event::ResetTitle => self.set_title(id, DEFAULT_TITLE.to_string()),
            // Replies the terminal owes the program (cursor position and
            // the like).
            Event::PtyWrite(text) => self.write_to(id, text.into_bytes()),
            // `CSI 14 t`: the text area's size in pixels.
            Event::TextAreaSizeRequest(format) => {
                if let Some(size) = self.text_area_size() {
                    self.write_to(id, format(size).into_bytes());
                }
            }
            // `OSC 10/11/12 ; ?`: a color's current value.
            Event::ColorRequest(index, format) => {
                let color = self.shown.as_ref().and_then(|shown| shown.tabs.get(id)).map(|tab| crate::colors::resolve_index(index, tab.session.term.lock().colors()));
                if let Some(color) = color {
                    self.write_to(id, format(color).into_bytes());
                }
            }
            Event::Exit | Event::ChildExit(_) => self.close_tab(id),
            _ => {}
        }
    }

    fn write_to(&self, id: TabId, bytes: Vec<u8>) {
        if let Some(tab) = self.shown.as_ref().and_then(|shown| shown.tabs.get(id)) {
            tab.session.write(bytes);
        }
    }

    /// `Ctrl+Shift+T`, `Ctrl+Shift+W`, `Ctrl+Tab`, `Ctrl+Shift+Tab`.
    pub(super) fn tab_chord(&mut self, chord: TabChord) {
        match chord {
            TabChord::New => self.new_tab(),
            TabChord::Close => {
                if let Some(id) = self.active_tab_id() {
                    self.close_tab(id);
                }
            }
            TabChord::Next => self.switch_tab(|tabs| tabs.step(true)),
            TabChord::Previous => self.switch_tab(|tabs| tabs.step(false)),
        }
    }

    /// A click on the bar at `column`: a tab shows it (a middle click
    /// closes it), its `×` closes it, `+` opens a new one.
    pub(super) fn bar_click(&mut self, column: usize, button: MouseButton) {
        let Some(shown) = &self.shown else {
            return;
        };
        let titles: Vec<&str> = shown.tabs.iter().map(|tab| tab.title.as_str()).collect();
        let Some(&(_, part)) = tabs::bar(&titles, shown.grid.columns).get(column) else {
            return;
        };
        match (part, button) {
            (BarPart::Tab(index), MouseButton::Middle) | (BarPart::Close(index), MouseButton::Left) => {
                if let Some(id) = shown.tabs.id_at(index) {
                    self.close_tab(id);
                }
            }
            (BarPart::Tab(index), MouseButton::Left) => self.switch_tab(|tabs| tabs.select(index)),
            (BarPart::New, MouseButton::Left) => self.new_tab(),
            _ => {}
        }
    }

    fn new_tab(&mut self) {
        let old = self.active_tab_id();
        let Some(shown) = &mut self.shown else {
            return;
        };
        if let Err(err) = spawn_tab(shown, &self.proxy) {
            shown.window.set_title(&format!("litastum: couldn't open a tab: {err}"));
            return;
        }
        self.focus_left(old);
        self.after_tab_change();
    }

    /// Closes tab `id`, ending the litastum in it (its pseudoconsole
    /// closes). The last one closes the window.
    pub(super) fn close_tab(&mut self, id: TabId) {
        let was_shown = self.active_tab_id() == Some(id);
        let Some(shown) = &mut self.shown else {
            return;
        };
        if !shown.tabs.remove(id) {
            self.exit_requested = true;
            return;
        }
        if was_shown {
            self.focus_entered();
            self.after_tab_change();
        } else {
            self.request_redraw();
        }
    }

    fn switch_tab(&mut self, change: impl FnOnce(&mut Tabs<Session>) -> bool) {
        let old = self.active_tab_id();
        let changed = self.shown.as_mut().is_some_and(|shown| change(&mut shown.tabs));
        if changed {
            self.focus_left(old);
            self.focus_entered();
            self.after_tab_change();
        }
    }

    /// Tab `id` named itself (`OSC 0`/`2`): on the bar, and on the window
    /// while it's shown.
    pub(super) fn set_title(&mut self, id: TabId, title: String) {
        let Some(shown) = &mut self.shown else {
            return;
        };
        if let Some(tab) = shown.tabs.get_mut(id) {
            tab.title = title;
        }
        if self.active_tab_id() == Some(id) {
            self.sync_window_title();
        }
        self.request_redraw();
    }

    /// Another tab is shown: everything is drawn anew, in its colors and
    /// with its title.
    pub(super) fn after_tab_change(&mut self) {
        if let Some(shown) = &mut self.shown {
            shown.renderer.invalidate();
        }
        self.sync_window_title();
        self.frame_colors = None;
        self.sync_frame_colors();
        self.request_redraw();
    }

    /// The window frame in the shown program's colors (`window_style`),
    /// when they changed.
    pub(super) fn sync_frame_colors(&mut self) {
        let Some(shown) = &self.shown else {
            return;
        };
        let Some(tab) = shown.tabs.active() else {
            return;
        };
        let wanted = {
            let term = tab.session.term.lock();
            let term_colors = term.colors();
            (colors::resolve(TermColor::Named(NamedColor::Background), term_colors), colors::resolve(TermColor::Named(NamedColor::Foreground), term_colors))
        };
        if self.frame_colors != Some(wanted) {
            self.frame_colors = Some(wanted);
            window_style::set_frame_colors(&shown.window, wanted.0, wanted.1);
        }
    }

    fn sync_window_title(&self) {
        let Some(shown) = &self.shown else {
            return;
        };
        let title = shown.tabs.active().map_or(DEFAULT_TITLE, |tab| tab.title.as_str());
        let title = if title == DEFAULT_TITLE { title.to_string() } else { format!("{title} - {DEFAULT_TITLE}") };
        shown.window.set_title(&title);
    }

    fn active_tab_id(&self) -> Option<TabId> {
        Some(self.shown.as_ref()?.tabs.active()?.id)
    }

    /// The tab that was shown loses the focus: each litastum polls
    /// `Ctrl+V` system-wide and takes it only while focused, so a
    /// background one mustn't think it still is.
    fn focus_left(&self, old: Option<TabId>) {
        let Some(tab) = old.and_then(|id| self.shown.as_ref()?.tabs.get(id)) else {
            return;
        };
        let mode = *tab.session.term.lock().mode();
        if let Some(report) = input::focus_report(false, mode) {
            tab.session.write(report.to_vec());
        }
    }

    fn focus_entered(&self) {
        if let Some(report) = input::focus_report(true, self.mode()) {
            self.write(report);
        }
    }
}
