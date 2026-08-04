//! Drives the real app through the real input and render paths.
//!
//! A test types keys and clicks coordinates; the harness runs the same loop the
//! binary does (dispatch → pump → draw) against a `TestBackend`, so assertions
//! read the actual rendered frame, not a mock.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

use postcat::app::App;
use postcat::{keys, ui};

pub const WIDTH: u16 = 140;
pub const HEIGHT: u16 = 40;

pub struct Harness {
    pub app: App,
    pub term: Terminal<TestBackend>,
    // Declared last: the app's spawned tasks must not outlive the runtime.
    _rt: tokio::runtime::Runtime,
}

impl Harness {
    pub fn new() -> Harness {
        Harness::sized(WIDTH, HEIGHT)
    }

    pub fn sized(width: u16, height: u16) -> Harness {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .expect("tokio runtime");
        let app = App::ephemeral(rt.handle().clone());
        let term = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        let mut h = Harness { app, term, _rt: rt };
        h.draw();
        h
    }

    // ---- the loop ---------------------------------------------------------

    /// Render one frame. Also refreshes the layout rects that mouse hit-testing
    /// depends on, which is why every input helper draws afterwards.
    pub fn draw(&mut self) {
        let app = &mut self.app;
        self.term.draw(|f| ui::draw(f, app)).expect("draw");
    }

    /// Drain worker events, rebuild the body, redraw.
    pub fn pump(&mut self) {
        self.app.pump();
        self.draw();
    }

    // ---- input ------------------------------------------------------------

    pub fn key(&mut self, code: KeyCode, mods: KeyModifiers) {
        keys::on_key(&mut self.app, KeyEvent::new(code, mods));
        self.pump();
    }

    /// A single character keypress.
    pub fn press(&mut self, c: char) {
        self.key(KeyCode::Char(c), KeyModifiers::NONE);
    }

    /// Type a string one character at a time, as a human would.
    pub fn type_str(&mut self, text: &str) {
        for c in text.chars() {
            self.press(c);
        }
    }

    /// Press a sequence of plain character keys, e.g. `chords("3ll")`.
    pub fn presses(&mut self, chars: &str) {
        for c in chars.chars() {
            self.press(c);
        }
    }

    pub fn ctrl(&mut self, c: char) {
        self.key(KeyCode::Char(c), KeyModifiers::CONTROL);
    }

    pub fn enter(&mut self) {
        self.key(KeyCode::Enter, KeyModifiers::NONE);
    }

    pub fn esc(&mut self) {
        self.key(KeyCode::Esc, KeyModifiers::NONE);
    }

    pub fn tab(&mut self) {
        self.key(KeyCode::Tab, KeyModifiers::NONE);
    }

    pub fn paste(&mut self, text: &str) {
        keys::on_paste(&mut self.app, text.to_string());
        self.pump();
    }

    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) {
        keys::on_mouse(
            &mut self.app,
            MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE },
        );
        self.pump();
    }

    pub fn click(&mut self, column: u16, row: u16) {
        self.mouse(MouseEventKind::Down(MouseButton::Left), column, row);
    }

    /// Click the first cell of `label` as rendered on screen, offset a couple of
    /// columns in so the click lands inside the word.
    pub fn click_text(&mut self, label: &str) {
        let (x, y) = self.find(label);
        let nudge = (label.chars().count() as u16 / 2).min(3);
        self.click(x + nudge, y);
    }

    pub fn move_mouse(&mut self, column: u16, row: u16) {
        self.mouse(MouseEventKind::Moved, column, row);
    }

    /// Hold the left button and move — what a terminal reports mid-drag.
    pub fn drag(&mut self, column: u16, row: u16) {
        self.mouse(MouseEventKind::Drag(MouseButton::Left), column, row);
    }

    pub fn release(&mut self, column: u16, row: u16) {
        self.mouse(MouseEventKind::Up(MouseButton::Left), column, row);
    }

    /// Press, drag and let go: one text selection, the way a hand makes it.
    pub fn select(&mut self, from: (u16, u16), to: (u16, u16)) {
        self.click(from.0, from.1);
        self.drag(to.0, to.1);
        self.release(to.0, to.1);
    }

    pub fn scroll_down(&mut self, column: u16, row: u16) {
        self.mouse(MouseEventKind::ScrollDown, column, row);
    }

    pub fn scroll_up(&mut self, column: u16, row: u16) {
        self.mouse(MouseEventKind::ScrollUp, column, row);
    }

    // ---- reading the screen ----------------------------------------------

    pub fn lines(&self) -> Vec<String> {
        let buf = self.term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    pub fn screen(&self) -> String {
        self.lines().join("\n")
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.lines().iter().any(|l| l.contains(needle))
    }

    /// Column/row of the first cell of `needle`, or panic with the whole screen.
    pub fn find(&self, needle: &str) -> (u16, u16) {
        self.try_find(needle)
            .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{}", self.screen()))
    }

    pub fn try_find(&self, needle: &str) -> Option<(u16, u16)> {
        self.lines().iter().enumerate().find_map(|(y, line)| {
            // char_indices keeps the column correct for multi-byte glyphs.
            let byte_at = line.find(needle)?;
            let col = line[..byte_at].chars().count() as u16;
            Some((col, y as u16))
        })
    }

    /// Foreground colour of the cell at the first character of `needle`.
    pub fn fg_at(&self, needle: &str) -> Color {
        let (x, y) = self.find(needle);
        self.term.backend().buffer()[(x, y)].fg
    }

    /// Background colour of a screen cell.
    pub fn bg_at(&self, column: u16, row: u16) -> Color {
        self.term.backend().buffer()[(column, row)].bg
    }

    /// The line containing `needle`.
    pub fn line_with(&self, needle: &str) -> String {
        let (_, y) = self.find(needle);
        self.lines()[y as usize].clone()
    }

    // ---- waiting ----------------------------------------------------------

    /// Pump until `needle` renders, or the deadline passes.
    pub fn wait_for(&mut self, needle: &str, timeout: Duration) -> bool {
        self.wait_until(timeout, |h| h.contains(needle))
    }

    /// Like [`wait_for`], but fails the test with a screen dump on timeout.
    pub fn expect_text(&mut self, needle: &str, timeout: Duration) {
        if !self.wait_for(needle, timeout) {
            panic!(
                "timed out after {timeout:?} waiting for {needle:?}. screen:\n{}",
                self.screen()
            );
        }
    }

    pub fn wait_until(
        &mut self,
        timeout: Duration,
        mut ready: impl FnMut(&Harness) -> bool,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump();
            if ready(self) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Keep the loop running for a fixed stretch (used to prove that nothing
    /// further arrives, e.g. after a cancel).
    pub fn settle(&mut self, dur: Duration) {
        let deadline = Instant::now() + dur;
        while Instant::now() < deadline {
            self.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // ---- common flows -----------------------------------------------------

    /// Replace the URL and leave insert mode (no send). Clears first, so calling
    /// it twice in one test really replaces rather than appends.
    pub fn set_url(&mut self, url: &str) {
        self.press('u');
        self.ctrl('u');
        self.type_str(url);
        self.esc();
    }

    /// Set the URL and send, waiting for the response to land.
    pub fn send_url(&mut self, url: &str) {
        self.set_url(url);
        self.press('s');
        self.await_response();
    }

    /// Wait until the request is fully done — body complete and history written,
    /// not merely "headers arrived". Streaming tests use `expect_text` instead.
    pub fn await_response(&mut self) {
        let ok = self.wait_until(Duration::from_secs(10), |h| {
            !h.app.loading && !h.app.streaming && (h.app.resp.is_some() || h.app.resp_err.is_some())
        });
        assert!(ok, "request never completed. screen:\n{}", self.screen());
    }

    pub fn status(&self) -> Option<u16> {
        self.app.resp.as_ref().map(|r| r.data.status)
    }

    /// The response body as text.
    pub fn body(&self) -> String {
        self.app
            .resp
            .as_ref()
            .map(|r| String::from_utf8_lossy(&r.data.body).into_owned())
            .unwrap_or_default()
    }
}

impl Default for Harness {
    fn default() -> Self {
        Harness::new()
    }
}
