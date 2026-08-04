//! Application state and state transitions (everything except key dispatch).

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Text;
use tui_textarea::{CursorMove, TextArea};

use crate::http::{self, ResponseData};
use crate::model::*;
use crate::theme;
use crate::ui::json::response_text;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Sidebar,
    Url,
    ReqContent,
    Response,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReqTab {
    Query,
    Headers,
    Body,
    Auth,
}

impl ReqTab {
    pub const ALL: [ReqTab; 4] = [ReqTab::Query, ReqTab::Headers, ReqTab::Body, ReqTab::Auth];

    pub fn idx(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn next(self) -> ReqTab {
        Self::ALL[(self.idx() + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> ReqTab {
        Self::ALL[(self.idx() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RespTab {
    Body,
    Headers,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SideTab {
    Saved,
    History,
}

/// What the keyboard is currently typing into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditTarget {
    None,
    Url,
    Body,
    Cell,
    Auth,
}

pub struct CellEdit {
    /// 0 = key column, 1 = value column.
    pub col: usize,
    pub ta: TextArea<'static>,
}

pub enum PromptKind {
    SaveAs,
    Rename(usize),
}

pub enum ConfirmKind {
    DeleteSaved(usize),
    DeleteHistory(usize),
}

// One overlay exists at a time (a plain App field), so variant size is irrelevant.
#[allow(clippy::large_enum_variant)]
pub enum Overlay {
    None,
    Help { scroll: u16 },
    Prompt { title: String, ta: TextArea<'static>, kind: PromptKind },
    Confirm { msg: String, kind: ConfirmKind },
    Env,
    /// Method picker dropdown anchored under the method chip.
    Method { sel: usize },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Ok,
    Err,
}

/// Streaming response lifecycle. Every event carries the generation counter of
/// the send that produced it, so events from cancelled requests are dropped.
pub enum AppEvent {
    Started {
        seq: u64,
        status: u16,
        reason: String,
        version: String,
        headers: Vec<(String, String)>,
        ttfb_ms: u64,
    },
    Chunk { seq: u64, bytes: Vec<u8> },
    Finished { seq: u64, total_ms: u64 },
    Failed { seq: u64, error: String },
}

pub struct RespView {
    pub data: ResponseData,
    pub body_text: Text<'static>,
    pub body_lines: usize,
}

#[derive(Default, Clone, Copy)]
pub struct Rects {
    pub sidebar: Rect,
    pub url: Rect,
    pub request: Rect,
    pub response: Rect,
}

/// A clickable label recorded during draw for mouse hit-testing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TabTarget {
    Req(ReqTab),
    Resp(RespTab),
    Side(SideTab),
    /// The method chip in the URL bar; opens the method dropdown.
    Method,
}

/// What the left mouse button is currently dragging.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drag {
    None,
    /// The response scrollbar thumb, carrying the row grabbed as an offset from
    /// the top of the thumb so it stays put under the pointer.
    Thumb(u16),
    /// Selecting text in the active editor.
    Field,
    /// Selecting text in the response pane.
    Response,
}

/// A selection over the response pane, anchored to screen cells rather than to
/// the document: it covers exactly what is rendered there — wrapped or not,
/// body or headers. Anything that changes what those cells show retires it,
/// see [`App::resp_sel_live`].
#[derive(Clone, Copy)]
pub struct RespSel {
    pub anchor: (u16, u16),
    pub head: (u16, u16),
    scroll: usize,
    tab: RespTab,
    req: u64,
}

impl RespSel {
    /// The two corners in reading order.
    pub fn ordered(&self) -> ((u16, u16), (u16, u16)) {
        let (a, h) = (self.anchor, self.head);
        if (a.1, a.0) <= (h.1, h.0) { (a, h) } else { (h, a) }
    }

    /// A press that never moved selects nothing.
    pub fn is_click(&self) -> bool {
        self.anchor == self.head
    }
}

/// Mirror of a textarea's scroll offset. tui-textarea keeps its viewport
/// private, so a click could not be mapped back to a position in the text
/// without recomputing it here. [`View::sync`] runs once per render, alongside
/// the widget's own update and by the same rule, so the two stay in lockstep.
#[derive(Default, Clone, Copy)]
pub struct View {
    row: u16,
    col: u16,
}

/// Keep the cursor inside the window; scroll only when it leaves.
fn next_top(prev: u16, cursor: u16, len: u16) -> u16 {
    if cursor < prev {
        cursor
    } else if prev.saturating_add(len) <= cursor {
        cursor + 1 - len
    } else {
        prev
    }
}

/// Width of the line-number gutter, for fields that draw one.
fn gutter(ta: &TextArea) -> u16 {
    if ta.line_number_style().is_none() {
        return 0;
    }
    // digits, plus a space either side
    ta.lines().len().to_string().len() as u16 + 2
}

impl View {
    /// Follow the widget's scrolling. Call once per render of `ta`.
    pub fn sync(&mut self, ta: &TextArea, area: Rect) {
        let (row, col) = ta.cursor();
        self.row = next_top(self.row, row as u16, area.height);
        let g = gutter(ta);
        let col = match col as u16 {
            // What the widget does: slide the gutter in on the way left, and
            // offset by it once the cursor is past it.
            c if g == 0 => c,
            c if c <= g => c * 2,
            c => c + g,
        };
        self.col = next_top(self.col, col, area.width);
    }

    /// Screen cell → position in the text, for [`CursorMove::Jump`] (which
    /// clamps it to the text).
    pub fn pos_at(&self, ta: &TextArea, area: Rect, col: u16, row: u16) -> (u16, u16) {
        let r = self.row + row.saturating_sub(area.y);
        let c = col.saturating_sub(area.x) + self.col;
        (r, c.saturating_sub(gutter(ta)))
    }
}

/// Scroll mirrors for the fields a click can land in. Only one transient editor
/// (grid cell, auth field, prompt) exists at a time, so they share `inline`.
#[derive(Default, Clone, Copy)]
pub struct Views {
    pub url: View,
    pub body: View,
    pub inline: View,
}

pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const HISTORY_CAP: usize = 50;

/// Content types that arrive as a running log rather than one document.
pub fn is_live_stream(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.to_ascii_lowercase())
        .is_some_and(|ct| {
            ct.contains("event-stream") || ct.contains("x-ndjson") || ct.contains("stream+json")
        })
}

pub struct App {
    pub focus: Focus,
    pub req_tab: ReqTab,
    pub resp_tab: RespTab,
    pub side_tab: SideTab,
    pub overlay: Overlay,
    pub edit: EditTarget,

    pub draft: RequestModel,
    pub url_ta: TextArea<'static>,
    pub body_ta: TextArea<'static>,
    pub cell: Option<CellEdit>,
    pub auth_ta: Option<TextArea<'static>>,

    pub params_sel: usize,
    pub headers_sel: usize,
    pub form_sel: usize,
    pub auth_sel: usize,
    pub env_sel: usize,
    pub side_sel: usize,
    /// First visible row of the sidebar list (set during draw, used by mouse clicks).
    pub side_off: usize,

    pub saved: Vec<RequestModel>,
    pub history: Vec<HistoryEntry>,
    pub env: Vec<KV>,

    pub resp: Option<RespView>,
    pub resp_err: Option<String>,
    pub resp_scroll: usize,
    pub wrap: bool,

    pub loading: bool,
    /// Headers arrived, body chunks are still coming in.
    pub streaming: bool,
    /// Keep the view pinned to the tail while a stream grows.
    pub follow: bool,
    /// Body grew since the last rebuild of the rendered text.
    pub resp_dirty: bool,
    /// Request belonging to the in-flight send (for the history entry).
    pub pending_req: Option<RequestModel>,
    /// Generation counter matching in-flight events to the current send.
    pub req_gen: u64,
    pub spin: usize,
    pub sent_at: Option<Instant>,
    pub sidebar_open: bool,
    pub toast: Option<(String, ToastKind, Instant)>,
    pub should_quit: bool,
    pub rects: Rects,
    pub tab_hits: Vec<(Rect, TabTarget)>,
    /// Response scrollbar track (set during draw, empty when the body fits).
    pub resp_sb: Rect,
    /// What the left button is dragging, if anything.
    pub drag: Drag,
    /// Rect of the editor currently in insert mode (zero-sized when none).
    pub edit_rect: Rect,
    /// The url and body textareas as last rendered, so a click can land in them
    /// even before they are being edited. Both are zero-sized when off screen.
    pub url_rect: Rect,
    pub body_rect: Rect,
    /// Response text area (set during draw): the region a selection covers.
    pub resp_rect: Rect,
    /// Screen-anchored selection over the response pane.
    pub resp_sel: Option<RespSel>,
    /// Text under `resp_sel`, read back out of the frame while painting it.
    pub resp_sel_text: String,
    /// Copy the response selection at the end of the next draw, once the text
    /// under it is known.
    pub copy_pending: bool,
    /// Scroll mirrors of the textareas, for mapping clicks into them.
    pub views: Views,
    /// Last text handed to the clipboard. Kept for the tests; nothing reads it.
    pub last_copy: Option<String>,
    /// Send copies to the system clipboard via OSC 52. Off in tests, which have
    /// no terminal on the other end.
    pub osc_clipboard: bool,
    /// Mouse is over the active editor (drives the OSC 22 pointer shape:
    /// I-beam there, default arrow everywhere else).
    pub hover_text: bool,
    /// Workspace file, or `None` for an ephemeral session.
    pub state_path: Option<PathBuf>,

    pub tx: Sender<AppEvent>,
    pub rx: Receiver<AppEvent>,
    pub rt: tokio::runtime::Handle,
    pub client: reqwest::Client,
    pub inflight: Option<tokio::task::JoinHandle<()>>,
}

pub fn line_input(text: &str, placeholder: &str) -> TextArea<'static> {
    let mut ta = TextArea::from([text.to_string()]);
    ta.set_cursor_line_style(Style::default());
    ta.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
    ta.set_selection_style(Style::default().fg(theme::CHIP_FG).bg(theme::ACCENT));
    ta.set_placeholder_text(placeholder);
    ta.set_placeholder_style(Style::default().fg(theme::DIM));
    ta.move_cursor(CursorMove::End);
    ta
}

pub fn body_input(text: &str) -> TextArea<'static> {
    let mut ta = TextArea::from(text.lines().map(str::to_string).collect::<Vec<_>>());
    ta.set_cursor_line_style(Style::default());
    // Cursor is hidden until the body is being edited.
    ta.set_cursor_style(Style::default());
    ta.set_selection_style(Style::default().fg(theme::CHIP_FG).bg(theme::ACCENT));
    ta.set_line_number_style(Style::default().fg(theme::BORDER));
    ta.set_placeholder_text("{\n  \"hello\": \"world\"\n}");
    ta.set_placeholder_style(Style::default().fg(theme::DIM));
    ta
}

pub fn ta_text(ta: &TextArea) -> String {
    ta.lines().join("\n")
}

/// The selected text of a field, leaving the selection alone — unlike
/// `TextArea::copy`, which consumes it.
pub fn ta_selection(ta: &TextArea) -> String {
    let Some(((sr, sc), (er, ec))) = ta.selection_range() else {
        return String::new();
    };
    let slice = |row: usize, from: usize, to: usize| -> String {
        ta.lines()[row].chars().skip(from).take(to - from).collect()
    };
    if sr == er {
        return slice(sr, sc, ec);
    }
    let mut out = vec![slice(sr, sc, usize::MAX)];
    out.extend(ta.lines()[sr + 1..er].iter().cloned());
    out.push(slice(er, 0, ec));
    out.join("\n")
}

impl App {
    /// The real app: workspace loaded from (and saved to) the user's config dir.
    pub fn new(rt: tokio::runtime::Handle) -> App {
        App::with_workspace(rt, default_state_path())
    }

    /// An app with no workspace file — nothing is loaded, nothing is written.
    /// This is what the end-to-end tests run against.
    pub fn ephemeral(rt: tokio::runtime::Handle) -> App {
        let mut app = App::with_workspace(rt, None);
        // No terminal on the other end of stdout, so no escape sequences.
        app.osc_clipboard = false;
        app
    }

    pub fn with_workspace(rt: tokio::runtime::Handle, state_path: Option<PathBuf>) -> App {
        let persisted = Persisted::load(state_path.as_deref());
        let draft = persisted.draft.unwrap_or_default();
        let mut url_ta = line_input(&draft.url, "https://api.example.com/v1/…  ({{vars}} ok)");
        url_ta.set_cursor_style(Style::default());
        let body_ta = body_input(&draft.body);
        let (tx, rx) = channel();
        App {
            focus: Focus::Url,
            req_tab: ReqTab::Query,
            resp_tab: RespTab::Body,
            side_tab: SideTab::Saved,
            overlay: Overlay::None,
            edit: EditTarget::None,
            draft,
            url_ta,
            body_ta,
            cell: None,
            auth_ta: None,
            params_sel: 0,
            headers_sel: 0,
            form_sel: 0,
            auth_sel: 0,
            env_sel: 0,
            side_sel: 0,
            side_off: 0,
            saved: persisted.saved,
            history: persisted.history,
            env: persisted.env,
            resp: None,
            resp_err: None,
            resp_scroll: 0,
            wrap: true,
            loading: false,
            streaming: false,
            follow: false,
            resp_dirty: false,
            pending_req: None,
            req_gen: 0,
            spin: 0,
            sent_at: None,
            sidebar_open: true,
            toast: None,
            should_quit: false,
            rects: Rects::default(),
            tab_hits: Vec::new(),
            resp_sb: Rect::default(),
            drag: Drag::None,
            edit_rect: Rect::default(),
            url_rect: Rect::default(),
            body_rect: Rect::default(),
            resp_rect: Rect::default(),
            resp_sel: None,
            resp_sel_text: String::new(),
            copy_pending: false,
            views: Views::default(),
            last_copy: None,
            osc_clipboard: true,
            hover_text: false,
            state_path,
            tx,
            rx,
            rt,
            client: http::build_client(),
            inflight: None,
        }
    }

    // ---- misc -------------------------------------------------------------

    /// Where the method dropdown lives: anchored right under the method chip.
    pub fn method_popup(&self) -> Rect {
        let u = self.rects.url;
        Rect {
            x: u.x + 1,
            y: u.y + 2,
            width: 13,
            height: Method::ALL.len() as u16 + 2,
        }
    }

    pub fn open_method_menu(&mut self) {
        let sel = Method::ALL
            .iter()
            .position(|m| *m == self.draft.method)
            .unwrap_or(0);
        self.focus = Focus::Url;
        self.overlay = Overlay::Method { sel };
    }

    pub fn toast(&mut self, msg: impl Into<String>, kind: ToastKind) {
        self.toast = Some((msg.into(), kind, Instant::now()));
    }

    pub fn tick(&mut self) {
        if self.loading || self.streaming {
            self.spin = self.spin.wrapping_add(1);
        }
        if let Some((_, _, at)) = &self.toast
            && at.elapsed() > Duration::from_millis(2600) {
                self.toast = None;
            }
    }

    pub fn in_insert(&self) -> bool {
        self.edit != EditTarget::None || matches!(self.overlay, Overlay::Prompt { .. })
    }

    pub fn quit(&mut self) {
        self.persist();
        self.should_quit = true;
    }

    /// Drain worker events and rebuild the rendered body if chunks arrived.
    pub fn pump(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            self.on_app_event(ev);
        }
        self.flush_resp();
    }

    pub fn persist(&mut self) {
        self.sync_draft();
        Persisted {
            saved: self.saved.clone(),
            history: self.history.clone(),
            env: self.env.clone(),
            draft: Some(self.draft.clone()),
        }
        .save(self.state_path.as_deref());
    }

    // ---- selection & clipboard --------------------------------------------

    /// The textarea the keyboard and mouse are working in, with the mirror and
    /// rect needed to map screen positions into it.
    pub fn editor(&mut self) -> Option<(&mut TextArea<'static>, &mut View, Rect)> {
        match self.edit {
            EditTarget::Url => Some((&mut self.url_ta, &mut self.views.url, self.url_rect)),
            EditTarget::Body => Some((&mut self.body_ta, &mut self.views.body, self.body_rect)),
            EditTarget::Cell => match &mut self.cell {
                Some(c) => Some((&mut c.ta, &mut self.views.inline, self.edit_rect)),
                None => None,
            },
            EditTarget::Auth => match &mut self.auth_ta {
                Some(ta) => Some((ta, &mut self.views.inline, self.edit_rect)),
                None => None,
            },
            EditTarget::None => match &mut self.overlay {
                Overlay::Prompt { ta, .. } => Some((ta, &mut self.views.inline, self.edit_rect)),
                _ => None,
            },
        }
    }

    /// Keep the transient editor's scroll mirror in step with its widget.
    /// Called from the draw that rendered it, once per frame.
    pub fn sync_inline(&mut self, rect: Rect) {
        let ta = match self.edit {
            EditTarget::Cell => self.cell.as_ref().map(|c| &c.ta),
            EditTarget::Auth => self.auth_ta.as_ref(),
            _ => None,
        };
        if let Some(ta) = ta {
            self.views.inline.sync(ta, rect);
        }
    }

    pub fn copy(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.last_copy = Some(text.to_string());
        if self.osc_clipboard {
            crate::clip::set(text);
        }
        let lines = text.lines().count();
        let msg = if lines > 1 {
            format!("copied {lines} lines ✓")
        } else {
            format!("copied {} chars ✓", text.chars().count())
        };
        self.toast(msg, ToastKind::Ok);
    }

    /// Copy what is selected in the active editor, if anything.
    pub fn copy_field_selection(&mut self) {
        let text = match self.editor() {
            // Masked fields render bullets; a stray drag must not push the real
            // password onto the system clipboard.
            Some((ta, _, _)) if ta.mask_char().is_none() => ta_selection(ta),
            _ => String::new(),
        };
        self.copy(&text);
    }

    fn resp_cell(&self, col: u16, row: u16) -> Option<(u16, u16)> {
        let a = self.resp_rect;
        if a.width == 0 || a.height == 0 {
            return None;
        }
        Some((col.clamp(a.x, a.right() - 1), row.clamp(a.y, a.bottom() - 1)))
    }

    pub fn begin_resp_sel(&mut self, col: u16, row: u16) {
        let Some(at) = self.resp_cell(col, row) else { return };
        self.resp_sel = Some(RespSel {
            anchor: at,
            head: at,
            scroll: self.resp_scroll,
            tab: self.resp_tab,
            req: self.req_gen,
        });
    }

    pub fn extend_resp_sel(&mut self, col: u16, row: u16) {
        let Some(at) = self.resp_cell(col, row) else { return };
        if let Some(sel) = &mut self.resp_sel {
            sel.head = at;
        }
    }

    /// Whether the selection still covers the text it was made over. It is
    /// pinned to screen cells, so scrolling, switching tab or a new response
    /// all retire it rather than silently highlighting something else.
    pub fn resp_sel_live(&self) -> bool {
        self.resp_sel.is_some_and(|s| {
            s.scroll == self.resp_scroll && s.tab == self.resp_tab && s.req == self.req_gen
        })
    }

    pub fn clear_resp_sel(&mut self) {
        self.resp_sel = None;
        self.resp_sel_text.clear();
        self.copy_pending = false;
    }

    // ---- draft <-> inputs -------------------------------------------------

    /// Pull the textarea contents back into the draft model.
    pub fn sync_draft(&mut self) {
        self.draft.url = ta_text(&self.url_ta).replace('\n', "");
        self.draft.body = ta_text(&self.body_ta);
    }

    pub fn new_request(&mut self) {
        self.commit_cell();
        self.stop_edit_body();
        self.draft = RequestModel::default();
        self.url_ta = line_input("", "https://api.example.com/v1/…  ({{vars}} ok)");
        self.url_ta.set_cursor_style(Style::default());
        self.body_ta = body_input("");
        self.params_sel = 0;
        self.headers_sel = 0;
        self.form_sel = 0;
        self.auth_sel = 0;
        self.req_tab = ReqTab::Query;
        self.focus = Focus::Url;
        self.edit = EditTarget::None;
        self.toast("new request", ToastKind::Info);
    }

    pub fn load_model(&mut self, m: RequestModel) {
        self.commit_cell();
        self.stop_edit_body();
        self.draft = m;
        if self.draft.params.is_empty() {
            self.draft.params.push(KV::default());
        }
        if self.draft.headers.is_empty() {
            self.draft.headers.push(KV::default());
        }
        if self.draft.form.is_empty() {
            self.draft.form.push(KV::default());
        }
        self.url_ta = line_input(&self.draft.url, "https://api.example.com/v1/…");
        self.url_ta.set_cursor_style(Style::default());
        self.body_ta = body_input(&self.draft.body);
        self.params_sel = 0;
        self.headers_sel = 0;
        self.form_sel = 0;
        self.auth_sel = 0;
        self.edit = EditTarget::None;
        self.cell = None;
    }

    // ---- grids ------------------------------------------------------------

    /// The KV grid the cursor currently operates on (depends on overlay + tab).
    pub fn active_grid(&mut self) -> Option<(&mut Vec<KV>, &mut usize)> {
        if matches!(self.overlay, Overlay::Env) {
            return Some((&mut self.env, &mut self.env_sel));
        }
        if self.focus != Focus::ReqContent {
            return None;
        }
        match self.req_tab {
            ReqTab::Query => Some((&mut self.draft.params, &mut self.params_sel)),
            ReqTab::Headers => Some((&mut self.draft.headers, &mut self.headers_sel)),
            ReqTab::Body if self.draft.body_type == BodyType::Form => {
                Some((&mut self.draft.form, &mut self.form_sel))
            }
            _ => None,
        }
    }

    pub fn start_cell_edit(&mut self, col: usize) {
        let Some((rows, sel)) = self.active_grid() else { return };
        if rows.is_empty() {
            rows.push(KV::default());
        }
        let i = (*sel).min(rows.len() - 1);
        *sel = i;
        let text = if col == 0 { rows[i].key.clone() } else { rows[i].value.clone() };
        let placeholder = if col == 0 { "key" } else { "value" };
        let mut ta = line_input(&text, placeholder);
        ta.set_style(Style::default().fg(theme::FG).bg(theme::SEL_BG));
        if !text.is_empty() {
            // Spreadsheet ergonomics: typing replaces, arrows keep the old value.
            ta.select_all();
        }
        self.cell = Some(CellEdit { col, ta });
        self.edit = EditTarget::Cell;
        self.views.inline = View::default();
    }

    /// Write the in-progress cell edit back into the grid. Safe to call anytime.
    pub fn commit_cell(&mut self) {
        let Some(cell) = self.cell.take() else { return };
        let col = cell.col;
        let text = ta_text(&cell.ta).replace('\n', "");
        if let Some((rows, sel)) = self.active_grid()
            && let Some(row) = rows.get_mut(*sel) {
                if col == 0 {
                    row.key = text;
                } else {
                    row.value = text;
                }
            }
        if self.edit == EditTarget::Cell {
            self.edit = EditTarget::None;
        }
    }

    pub fn grid_add_row(&mut self) {
        let Some((rows, sel)) = self.active_grid() else { return };
        let at = if rows.is_empty() { 0 } else { (*sel + 1).min(rows.len()) };
        rows.insert(at, KV::default());
        *sel = at;
        self.start_cell_edit(0);
    }

    pub fn grid_delete_row(&mut self) {
        self.cell = None;
        if self.edit == EditTarget::Cell {
            self.edit = EditTarget::None;
        }
        let Some((rows, sel)) = self.active_grid() else { return };
        if rows.is_empty() {
            return;
        }
        rows.remove(*sel);
        if rows.is_empty() {
            rows.push(KV::default());
        }
        *sel = (*sel).min(rows.len() - 1);
    }

    pub fn grid_toggle_row(&mut self) {
        let Some((rows, sel)) = self.active_grid() else { return };
        if let Some(row) = rows.get_mut(*sel) {
            row.enabled = !row.enabled;
        }
    }

    // ---- auth fields ------------------------------------------------------

    pub fn auth_field_count(&self) -> usize {
        match self.draft.auth.typ {
            AuthType::None => 0,
            AuthType::Bearer => 1,
            AuthType::Basic => 2,
        }
    }

    pub fn start_auth_edit(&mut self) {
        if self.auth_field_count() == 0 {
            return;
        }
        self.auth_sel = self.auth_sel.min(self.auth_field_count() - 1);
        let (text, placeholder, mask) = match (self.draft.auth.typ, self.auth_sel) {
            (AuthType::Bearer, _) => (self.draft.auth.token.clone(), "token", false),
            (AuthType::Basic, 0) => (self.draft.auth.user.clone(), "username", false),
            _ => (self.draft.auth.pass.clone(), "password", true),
        };
        let mut ta = line_input(&text, placeholder);
        ta.set_style(Style::default().fg(theme::FG).bg(theme::SEL_BG));
        if mask {
            ta.set_mask_char('•');
        }
        if !text.is_empty() {
            ta.select_all();
        }
        self.auth_ta = Some(ta);
        self.edit = EditTarget::Auth;
        self.views.inline = View::default();
    }

    pub fn commit_auth(&mut self) {
        let Some(ta) = self.auth_ta.take() else { return };
        let text = ta_text(&ta).replace('\n', "");
        match (self.draft.auth.typ, self.auth_sel) {
            (AuthType::Bearer, _) => self.draft.auth.token = text,
            (AuthType::Basic, 0) => self.draft.auth.user = text,
            (AuthType::Basic, _) => self.draft.auth.pass = text,
            _ => {}
        }
        if self.edit == EditTarget::Auth {
            self.edit = EditTarget::None;
        }
    }

    // ---- url / body editing ----------------------------------------------

    pub fn start_edit_url(&mut self) {
        self.commit_cell();
        self.commit_auth();
        self.focus = Focus::Url;
        self.edit = EditTarget::Url;
        self.url_ta
            .set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
        self.url_ta.move_cursor(CursorMove::End);
    }

    pub fn stop_edit_url(&mut self) {
        if self.edit == EditTarget::Url {
            self.edit = EditTarget::None;
        }
        self.url_ta.set_cursor_style(Style::default());
        self.sync_draft();
    }

    pub fn start_edit_body(&mut self) {
        self.commit_cell();
        self.focus = Focus::ReqContent;
        self.req_tab = ReqTab::Body;
        self.edit = EditTarget::Body;
        self.body_ta
            .set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
    }

    pub fn stop_edit_body(&mut self) {
        if self.edit == EditTarget::Body {
            self.edit = EditTarget::None;
        }
        self.body_ta.set_cursor_style(Style::default());
        self.draft.body = ta_text(&self.body_ta);
    }

    pub fn format_body(&mut self) {
        let text = ta_text(&self.body_ta);
        if text.trim().is_empty() {
            return;
        }
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => {
                let pretty = serde_json::to_string_pretty(&v).unwrap_or(text);
                let editing = self.edit == EditTarget::Body;
                self.body_ta = body_input(&pretty);
                if editing {
                    self.body_ta
                        .set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
                }
                self.draft.body = pretty;
                self.toast("formatted ✓", ToastKind::Ok);
            }
            Err(e) => self.toast(format!("not valid JSON: {e}"), ToastKind::Err),
        }
    }

    // ---- sending ----------------------------------------------------------

    pub fn send(&mut self) {
        self.commit_cell();
        if self.edit == EditTarget::Body {
            self.stop_edit_body();
        }
        if self.edit == EditTarget::Url {
            self.stop_edit_url();
        }
        self.sync_draft();
        self.cancel_inflight();

        let req = self.draft.clone();
        if req.url.trim().is_empty() {
            self.toast("no URL — press i to type one", ToastKind::Err);
            return;
        }
        self.loading = true;
        self.streaming = false;
        self.spin = 0;
        self.sent_at = Some(Instant::now());
        self.resp_err = None;
        self.resp_scroll = 0;
        self.req_gen = self.req_gen.wrapping_add(1);
        self.pending_req = Some(req.clone());

        let seq = self.req_gen;
        let client = self.client.clone();
        let env = self.env.clone();
        let tx = self.tx.clone();
        self.inflight = Some(self.rt.spawn(http::send_streaming(client, req, env, seq, tx)));
    }

    pub fn cancel_inflight(&mut self) {
        if let Some(h) = self.inflight.take() {
            h.abort();
        }
        if self.loading || self.streaming {
            self.loading = false;
            self.streaming = false;
            // Orphan any events the aborted task already queued.
            self.req_gen = self.req_gen.wrapping_add(1);
            self.pending_req = None;
            self.toast("request cancelled", ToastKind::Info);
        }
    }

    fn push_history(&mut self, status: Option<u16>, elapsed_ms: u64) {
        if let Some(req) = self.pending_req.take() {
            self.history.insert(0, HistoryEntry { request: req, status, elapsed_ms });
            self.history.truncate(HISTORY_CAP);
        }
    }

    pub fn on_app_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Started { seq, status, reason, version, headers, ttfb_ms } => {
                if seq != self.req_gen {
                    return;
                }
                self.loading = false;
                self.streaming = true;
                self.resp_err = None;
                self.resp = Some(RespView {
                    data: ResponseData {
                        status,
                        reason,
                        version,
                        headers,
                        body: Vec::new(),
                        elapsed_ms: ttfb_ms,
                    },
                    body_text: Text::default(),
                    body_lines: 0,
                });
                self.resp_scroll = 0;
                self.resp_tab = RespTab::Body;
                // Live formats read like a log, so pin to the newest line.
                // Ordinary bodies (JSON, HTML…) stay at the top.
                self.follow = self
                    .resp
                    .as_ref()
                    .is_some_and(|v| is_live_stream(&v.data.headers));
                self.resp_dirty = true;
            }
            AppEvent::Chunk { seq, bytes } => {
                if seq != self.req_gen {
                    return;
                }
                if let Some(v) = &mut self.resp {
                    v.data.body.extend_from_slice(&bytes);
                    self.resp_dirty = true;
                }
            }
            AppEvent::Finished { seq, total_ms } => {
                if seq != self.req_gen {
                    return;
                }
                self.loading = false;
                self.streaming = false;
                self.inflight = None;
                let status = if let Some(v) = &mut self.resp {
                    v.data.elapsed_ms = total_ms;
                    self.resp_dirty = true;
                    Some(v.data.status)
                } else {
                    None
                };
                self.push_history(status, total_ms);
            }
            AppEvent::Failed { seq, error } => {
                if seq != self.req_gen {
                    return;
                }
                // Streaming means headers (and possibly chunks) already landed.
                let partial = self.streaming;
                self.loading = false;
                self.streaming = false;
                self.inflight = None;
                let elapsed_ms = self
                    .sent_at
                    .map(|t| t.elapsed().as_millis() as u64)
                    .unwrap_or(0);
                let status = if partial {
                    self.resp.as_ref().map(|v| v.data.status)
                } else {
                    None
                };
                self.push_history(status, elapsed_ms);
                if partial {
                    self.toast(format!("stream ended: {error}"), ToastKind::Err);
                    self.resp_dirty = true;
                } else {
                    self.resp = None;
                    self.resp_err = Some(error);
                }
            }
        }
    }

    /// Rebuild the rendered body once per frame when new chunks arrived.
    pub fn flush_resp(&mut self) {
        if !self.resp_dirty {
            return;
        }
        self.resp_dirty = false;
        let follow = self.follow;
        let visible = self.rects.response.height.saturating_sub(4).max(1) as usize;
        if let Some(v) = &mut self.resp {
            let (text, lines) = response_text(&v.data);
            v.body_text = text;
            v.body_lines = lines;
            // Not gated on `streaming`: the final chunks land alongside the
            // Finished event, and the tail must stay visible after they do.
            if follow && self.resp_tab == RespTab::Body {
                self.resp_scroll = v.body_lines.saturating_sub(visible);
            }
        }
    }

    // ---- library ----------------------------------------------------------

    pub fn side_len(&self) -> usize {
        match self.side_tab {
            SideTab::Saved => self.saved.len(),
            SideTab::History => self.history.len(),
        }
    }

    pub fn save_as(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.toast("name can't be empty", ToastKind::Err);
            return;
        }
        self.sync_draft();
        self.draft.name = name.clone();
        let model = self.draft.clone();
        if let Some(existing) = self.saved.iter_mut().find(|r| r.name == name) {
            *existing = model;
            self.toast(format!("updated “{name}” ✓"), ToastKind::Ok);
        } else {
            self.saved.push(model);
            self.side_tab = SideTab::Saved;
            self.side_sel = self.saved.len() - 1;
            self.toast(format!("saved “{name}” ✓"), ToastKind::Ok);
        }
        self.persist();
    }
}
