//! Keyboard + mouse dispatch. Routing order: overlays → active editor → normal mode.

use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use tui_textarea::{CursorMove, Input};

use crate::app::*;
use crate::model::{BodyType, Method, KV, RequestModel};

pub fn on_key(app: &mut App, key: KeyEvent) {
    // Ctrl+C always quits, no matter what is focused.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.quit();
        return;
    }

    match &mut app.overlay {
        Overlay::Prompt { .. } => return prompt_key(app, key),
        Overlay::Confirm { .. } => return confirm_key(app, key),
        Overlay::Help { scroll } => {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Enter => {
                    app.overlay = Overlay::None
                }
                KeyCode::Char('j') | KeyCode::Down => *scroll = scroll.saturating_add(1),
                KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
                _ => {}
            }
            return;
        }
        Overlay::Method { sel } => {
            let n = Method::ALL.len();
            match key.code {
                KeyCode::Char('j') | KeyCode::Down | KeyCode::Char('m') | KeyCode::Tab => {
                    *sel = (*sel + 1) % n
                }
                KeyCode::Char('k') | KeyCode::Up | KeyCode::Char('M') | KeyCode::BackTab => {
                    *sel = (*sel + n - 1) % n
                }
                KeyCode::Char('g') => *sel = 0,
                KeyCode::Char('G') => *sel = n - 1,
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let m = Method::ALL[*sel];
                    app.draft.method = m;
                    app.overlay = Overlay::None;
                }
                KeyCode::Esc | KeyCode::Char('q') => app.overlay = Overlay::None,
                _ => {}
            }
            return;
        }
        _ => {}
    }

    match app.edit {
        EditTarget::Url => return url_edit_key(app, key),
        EditTarget::Body => return body_edit_key(app, key),
        EditTarget::Cell => return cell_edit_key(app, key),
        EditTarget::Auth => return auth_edit_key(app, key),
        EditTarget::None => {}
    }

    if matches!(app.overlay, Overlay::Env) {
        return env_key(app, key);
    }

    normal_key(app, key);
}

// ---- overlays --------------------------------------------------------------

fn prompt_key(app: &mut App, key: KeyEvent) {
    let Overlay::Prompt { ta, kind, .. } = &mut app.overlay else { return };
    match key.code {
        KeyCode::Enter => {
            let text = ta.lines().join("").trim().to_string();
            let kind = std::mem::replace(kind, PromptKind::SaveAs);
            app.overlay = Overlay::None;
            match kind {
                PromptKind::SaveAs => app.save_as(text),
                PromptKind::Rename(i) => {
                    if text.is_empty() {
                        app.toast("name can't be empty", ToastKind::Err);
                    } else if let Some(r) = app.saved.get_mut(i) {
                        r.name = text.clone();
                        app.persist();
                        app.toast(format!("renamed to “{text}” ✓"), ToastKind::Ok);
                    }
                }
            }
        }
        KeyCode::Esc => app.overlay = Overlay::None,
        _ => {
            ta.input(Input::from(key));
        }
    }
}

fn confirm_key(app: &mut App, key: KeyEvent) {
    let Overlay::Confirm { kind, .. } = &app.overlay else { return };
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            match kind {
                ConfirmKind::DeleteSaved(i) => {
                    let i = *i;
                    if i < app.saved.len() {
                        let name = app.saved[i].display_name().to_string();
                        app.saved.remove(i);
                        app.toast(format!("deleted “{name}”"), ToastKind::Info);
                    }
                }
                ConfirmKind::DeleteHistory(i) => {
                    let i = *i;
                    if i < app.history.len() {
                        app.history.remove(i);
                    }
                }
            }
            let len = app.side_len();
            app.side_sel = app.side_sel.min(len.saturating_sub(1));
            app.overlay = Overlay::None;
            app.persist();
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.overlay = Overlay::None,
        _ => {}
    }
}

fn env_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('e') | KeyCode::Char('q') => {
            app.commit_cell();
            app.env.retain(|kv| !kv.is_blank());
            app.overlay = Overlay::None;
            app.persist();
            app.toast("environment saved ✓", ToastKind::Ok);
        }
        _ => grid_normal_key(app, key),
    }
}

// ---- editors ---------------------------------------------------------------

fn url_edit_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            app.stop_edit_url();
            app.send();
        }
        KeyCode::Esc => app.stop_edit_url(),
        KeyCode::Tab => {
            app.stop_edit_url();
            app.focus = Focus::ReqContent;
        }
        KeyCode::Down => {
            app.stop_edit_url();
            app.focus = Focus::ReqContent;
        }
        // Shell idiom: Ctrl+U wipes the line (tui-textarea would treat it as undo).
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.url_ta.select_all();
            app.url_ta.cut();
        }
        _ => {
            app.url_ta.input(Input::from(key));
        }
    }
}

fn body_edit_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.stop_edit_body(),
        _ => {
            app.body_ta.input(Input::from(key));
        }
    }
}

fn cell_edit_key(app: &mut App, key: KeyEvent) {
    let Some(cell) = &mut app.cell else {
        app.edit = EditTarget::None;
        return;
    };
    let col = cell.col;
    match key.code {
        KeyCode::Esc => app.commit_cell(),
        KeyCode::Tab => {
            app.commit_cell();
            app.start_cell_edit(1 - col);
        }
        KeyCode::BackTab => {
            app.commit_cell();
            app.start_cell_edit(1 - col);
        }
        KeyCode::Enter => {
            app.commit_cell();
            if col == 0 {
                app.start_cell_edit(1);
            } else {
                // Finishing a value on the last non-blank row grows the grid.
                let grow = app
                    .active_grid()
                    .map(|(rows, sel)| *sel + 1 == rows.len() && !rows[*sel].is_blank())
                    .unwrap_or(false);
                if grow {
                    if let Some((rows, sel)) = app.active_grid() {
                        rows.push(KV::default());
                        *sel = rows.len() - 1;
                    }
                    app.start_cell_edit(0);
                }
            }
        }
        KeyCode::Up => {
            app.commit_cell();
            if let Some((_, sel)) = app.active_grid() {
                *sel = sel.saturating_sub(1);
            }
            app.start_cell_edit(col);
        }
        KeyCode::Down => {
            app.commit_cell();
            if let Some((rows, sel)) = app.active_grid() {
                *sel = (*sel + 1).min(rows.len().saturating_sub(1));
            }
            app.start_cell_edit(col);
        }
        _ => {
            cell.ta.input(Input::from(key));
        }
    }
}

fn auth_edit_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.commit_auth(),
        KeyCode::Enter | KeyCode::Tab => {
            app.commit_auth();
            let count = app.auth_field_count();
            if key.code == KeyCode::Tab && count > 1 {
                app.auth_sel = (app.auth_sel + 1) % count;
                app.start_auth_edit();
            } else if key.code == KeyCode::Enter && app.auth_sel + 1 < count {
                app.auth_sel += 1;
                app.start_auth_edit();
            }
        }
        _ => {
            if let Some(ta) = &mut app.auth_ta {
                ta.input(Input::from(key));
            }
        }
    }
}

// ---- normal mode -----------------------------------------------------------

fn normal_key(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // Global chords first.
    match (key.code, ctrl) {
        (KeyCode::Char('s'), true) => {
            app.sync_draft();
            let name = if app.draft.name.is_empty() {
                short_name(&app.draft)
            } else {
                app.draft.name.clone()
            };
            let mut ta = line_input(&name, "request name");
            ta.select_all();
            app.views.inline = View::default();
            app.overlay = Overlay::Prompt {
                title: "Save request as".into(),
                ta,
                kind: PromptKind::SaveAs,
            };
            return;
        }
        (KeyCode::Char('b'), true) => {
            app.sidebar_open = !app.sidebar_open;
            if !app.sidebar_open && app.focus == Focus::Sidebar {
                app.focus = Focus::Url;
            }
            return;
        }
        (KeyCode::Char('d'), true) | (KeyCode::Char('u'), true) => {
            if app.focus == Focus::Response {
                let half = (app.rects.response.height.saturating_sub(4) / 2).max(1) as usize;
                scroll_resp(app, if key.code == KeyCode::Char('d') { half as i64 } else { -(half as i64) });
            }
            return;
        }
        _ => {}
    }

    match key.code {
        KeyCode::Char('q') => return app.quit(),
        KeyCode::Char('?') => {
            app.overlay = Overlay::Help { scroll: 0 };
            return;
        }
        KeyCode::Tab => return cycle_focus(app, 1),
        KeyCode::BackTab => return cycle_focus(app, -1),
        KeyCode::Char('1') => {
            if app.sidebar_open {
                app.focus = Focus::Sidebar;
            }
            return;
        }
        KeyCode::Char('2') => {
            app.focus = Focus::Url;
            return;
        }
        KeyCode::Char('3') => {
            app.focus = Focus::ReqContent;
            return;
        }
        KeyCode::Char('4') => {
            app.focus = Focus::Response;
            return;
        }
        KeyCode::Char('n') => return app.new_request(),
        KeyCode::Char('e') => {
            if app.env.is_empty() {
                app.env.push(KV::default());
            }
            app.overlay = Overlay::Env;
            return;
        }
        KeyCode::Char('u') => return app.start_edit_url(),
        KeyCode::Char('s') => return app.send(),
        KeyCode::Esc => {
            if app.loading || app.streaming {
                app.cancel_inflight();
            } else if app.resp_sel.is_some() {
                app.clear_resp_sel();
            } else {
                app.toast = None;
            }
            return;
        }
        _ => {}
    }

    if app.focus != Focus::Sidebar {
        match key.code {
            KeyCode::Char('m') => {
                app.draft.method = app.draft.method.next();
                return;
            }
            KeyCode::Char('M') => {
                app.draft.method = app.draft.method.prev();
                return;
            }
            _ => {}
        }
    }

    match app.focus {
        Focus::Sidebar => sidebar_key(app, key),
        Focus::Url => url_key(app, key),
        Focus::ReqContent => req_content_key(app, key),
        Focus::Response => response_key(app, key),
    }
}

fn short_name(r: &RequestModel) -> String {
    let url = r.url.trim();
    let stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    stripped.chars().take(40).collect()
}

fn cycle_focus(app: &mut App, dir: i32) {
    let order: Vec<Focus> = if app.sidebar_open {
        vec![Focus::Sidebar, Focus::Url, Focus::ReqContent, Focus::Response]
    } else {
        vec![Focus::Url, Focus::ReqContent, Focus::Response]
    };
    let i = order.iter().position(|f| *f == app.focus).unwrap_or(0) as i32;
    let n = order.len() as i32;
    app.focus = order[((i + dir + n) % n) as usize];
}

fn sidebar_key(app: &mut App, key: KeyEvent) {
    let len = app.side_len();
    match key.code {
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('l') | KeyCode::Right => {
            app.side_tab = match app.side_tab {
                SideTab::Saved => SideTab::History,
                SideTab::History => SideTab::Saved,
            };
            app.side_sel = 0;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if len > 0 {
                app.side_sel = (app.side_sel + 1).min(len - 1);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => app.side_sel = app.side_sel.saturating_sub(1),
        KeyCode::Char('g') => app.side_sel = 0,
        KeyCode::Char('G') => app.side_sel = len.saturating_sub(1),
        KeyCode::Enter => open_side_selected(app),
        KeyCode::Char('d') | KeyCode::Char('x') => {
            if len == 0 {
                return;
            }
            let (msg, kind) = match app.side_tab {
                SideTab::Saved => (
                    format!("Delete “{}”?", app.saved[app.side_sel].display_name()),
                    ConfirmKind::DeleteSaved(app.side_sel),
                ),
                SideTab::History => (
                    "Delete this history entry?".to_string(),
                    ConfirmKind::DeleteHistory(app.side_sel),
                ),
            };
            app.overlay = Overlay::Confirm { msg, kind };
        }
        KeyCode::Char('r') => {
            if app.side_tab == SideTab::Saved
                && let Some(r) = app.saved.get(app.side_sel) {
                    let mut ta = line_input(&r.name, "request name");
                    ta.select_all();
                    app.views.inline = View::default();
                    app.overlay = Overlay::Prompt {
                        title: "Rename request".into(),
                        ta,
                        kind: PromptKind::Rename(app.side_sel),
                    };
                }
        }
        _ => {}
    }
}

/// Load the highlighted library entry into the request editor.
fn open_side_selected(app: &mut App) {
    let model = match app.side_tab {
        SideTab::Saved => app.saved.get(app.side_sel).cloned(),
        SideTab::History => app.history.get(app.side_sel).map(|h| h.request.clone()),
    };
    if let Some(m) = model {
        let name = m.display_name().to_string();
        app.load_model(m);
        app.focus = Focus::Url;
        app.toast(format!("opened “{name}”"), ToastKind::Info);
    }
}

fn url_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('i') | KeyCode::Char('a') => app.start_edit_url(),
        KeyCode::Enter => app.send(),
        KeyCode::Char('j') | KeyCode::Down => app.focus = Focus::ReqContent,
        _ => {}
    }
}

fn req_content_key(app: &mut App, key: KeyEvent) {
    // Tab switching works on every request tab.
    match key.code {
        KeyCode::Char(']') | KeyCode::Char('l') | KeyCode::Right => {
            app.commit_cell();
            app.req_tab = app.req_tab.next();
            return;
        }
        KeyCode::Char('[') | KeyCode::Char('h') | KeyCode::Left => {
            app.commit_cell();
            app.req_tab = app.req_tab.prev();
            return;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            // From the top row, k walks up to the URL bar.
            let at_top = match app.req_tab {
                ReqTab::Query => app.params_sel == 0,
                ReqTab::Headers => app.headers_sel == 0,
                ReqTab::Body => app.draft.body_type != BodyType::Form || app.form_sel == 0,
                ReqTab::Auth => app.auth_sel == 0,
            };
            if at_top {
                app.focus = Focus::Url;
                return;
            }
        }
        _ => {}
    }

    match app.req_tab {
        ReqTab::Query | ReqTab::Headers => grid_normal_key(app, key),
        ReqTab::Body => body_tab_key(app, key),
        ReqTab::Auth => auth_tab_key(app, key),
    }
}

fn grid_normal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some((rows, sel)) = app.active_grid() {
                *sel = (*sel + 1).min(rows.len().saturating_sub(1));
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some((_, sel)) = app.active_grid() {
                *sel = sel.saturating_sub(1);
            }
        }
        KeyCode::Char('g') => {
            if let Some((_, sel)) = app.active_grid() {
                *sel = 0;
            }
        }
        KeyCode::Char('G') => {
            if let Some((rows, sel)) = app.active_grid() {
                *sel = rows.len().saturating_sub(1);
            }
        }
        KeyCode::Enter | KeyCode::Char('i') => {
            // Smart edit: jump to the value if the key is already filled in.
            let col = app
                .active_grid()
                .map(|(rows, sel)| {
                    let row = &rows[(*sel).min(rows.len() - 1)];
                    usize::from(!row.key.is_empty() && row.value.is_empty())
                })
                .unwrap_or(0);
            app.start_cell_edit(col);
        }
        KeyCode::Char('a') => app.grid_add_row(),
        KeyCode::Char('d') | KeyCode::Char('x') => app.grid_delete_row(),
        KeyCode::Char(' ') => app.grid_toggle_row(),
        _ => {}
    }
}

fn body_tab_key(app: &mut App, key: KeyEvent) {
    if app.draft.body_type == BodyType::Form {
        match key.code {
            KeyCode::Char('t') => {
                app.commit_cell();
                app.draft.body_type = app.draft.body_type.next();
            }
            _ => grid_normal_key(app, key),
        }
        return;
    }
    match key.code {
        KeyCode::Char('t') => app.draft.body_type = app.draft.body_type.next(),
        KeyCode::Char('f') => app.format_body(),
        KeyCode::Enter | KeyCode::Char('i') => {
            if app.draft.body_type == BodyType::None {
                app.draft.body_type = BodyType::Json;
            }
            app.start_edit_body();
        }
        _ => {}
    }
}

fn auth_tab_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('t') => {
            app.draft.auth.typ = app.draft.auth.typ.next();
            app.auth_sel = 0;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let count = app.auth_field_count();
            if count > 0 {
                app.auth_sel = (app.auth_sel + 1).min(count - 1);
            }
        }
        KeyCode::Enter | KeyCode::Char('i') => app.start_auth_edit(),
        _ => {}
    }
}

fn response_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => scroll_resp(app, 1),
        KeyCode::Char('k') | KeyCode::Up => scroll_resp(app, -1),
        KeyCode::PageDown => scroll_resp(app, 20),
        KeyCode::PageUp => scroll_resp(app, -20),
        KeyCode::Char('g') => {
            app.resp_scroll = 0;
            app.follow = false;
        }
        KeyCode::Char('G') => {
            let visible = app.rects.response.height.saturating_sub(4) as usize;
            app.resp_scroll = resp_lines(app).saturating_sub(visible);
            // Jumping to the end re-arms tail-following for live streams.
            app.follow = true;
        }
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('[') => {
            app.resp_tab = RespTab::Body;
            app.resp_scroll = 0;
        }
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(']') => {
            app.resp_tab = RespTab::Headers;
            app.resp_scroll = 0;
        }
        KeyCode::Char('w') => app.wrap = !app.wrap,
        KeyCode::Char('y') => copy_response(app),
        KeyCode::Enter => app.send(),
        _ => {}
    }
}

/// `y` copies what is highlighted; with nothing selected it takes the whole
/// body (or the whole header list, on that tab).
fn copy_response(app: &mut App) {
    if !app.resp_sel_text.is_empty() {
        let text = app.resp_sel_text.clone();
        return app.copy(&text);
    }
    let text = app.resp.as_ref().map(|v| match app.resp_tab {
        RespTab::Body => String::from_utf8_lossy(&v.data.body).into_owned(),
        RespTab::Headers => v
            .data
            .headers
            .iter()
            .map(|(k, val)| format!("{k}: {val}"))
            .collect::<Vec<_>>()
            .join("\n"),
    });
    match text {
        Some(t) if !t.is_empty() => app.copy(&t),
        _ => app.toast("nothing to copy", ToastKind::Err),
    }
}

fn resp_lines(app: &App) -> usize {
    match app.resp_tab {
        RespTab::Body => app.resp.as_ref().map(|r| r.body_lines).unwrap_or(0),
        RespTab::Headers => app.resp.as_ref().map(|r| r.data.headers.len()).unwrap_or(0),
    }
}

fn scroll_resp(app: &mut App, delta: i64) {
    let max = resp_lines(app).saturating_sub(1);
    let cur = app.resp_scroll as i64;
    app.resp_scroll = (cur + delta).clamp(0, max as i64) as usize;
    // Manual scrolling takes over from tail-following (G re-arms it).
    app.follow = false;
}

// ---- response scrollbar dragging -------------------------------------------

/// Row offset and length of the scrollbar thumb, mirroring the geometry ratatui
/// renders so a dragged thumb stays under the pointer.
fn resp_thumb(pos: usize, total: usize, track: u16) -> (u16, u16) {
    let track_f = f64::from(track);
    // The widget pins the last line of the body to the top of the viewport.
    let max_pos = total.saturating_sub(track as usize).saturating_sub(1) as f64;
    let span = max_pos + track_f;
    let start = (pos as f64).clamp(0.0, max_pos);
    let head = (start * track_f / span).round().clamp(0.0, track_f - 1.0) as u16;
    let tail = ((start + track_f) * track_f / span).round().clamp(0.0, track_f) as u16;
    (head, tail.saturating_sub(head).max(1))
}

/// Press on the track: grab the thumb where it was clicked, or centre it on a
/// click elsewhere on the track. Either way the drag continues from there.
fn press_resp_sb(app: &mut App, row: u16) {
    let (track, total) = (app.resp_sb.height, resp_lines(app));
    if track == 0 || total <= track as usize {
        return;
    }
    let (thumb, len) = resp_thumb(app.resp_scroll, total, track);
    let off = row.saturating_sub(app.resp_sb.y);
    app.drag = Drag::Thumb(if off >= thumb && off < thumb + len {
        off - thumb
    } else {
        len / 2
    });
    drag_resp_sb(app, row);
}

/// Scroll the body so the thumb follows the pointer.
fn drag_resp_sb(app: &mut App, row: u16) {
    let (track, total) = (app.resp_sb.height, resp_lines(app));
    if track == 0 || total <= track as usize {
        return;
    }
    let grab = match app.drag {
        Drag::Thumb(off) => i64::from(off),
        _ => 0,
    };
    // Where the top of the thumb wants to sit, as an offset into the track.
    let head = (i64::from(row) - i64::from(app.resp_sb.y) - grab)
        .clamp(0, i64::from(track) - 1) as f64;
    let pos = (head * (total - 1) as f64 / f64::from(track)).round() as usize;
    // Bottom of the drag lands the last line at the bottom of the viewport, as G does.
    app.resp_scroll = pos.min(total - track as usize);
    app.follow = false;
}

// ---- paste & mouse ---------------------------------------------------------

pub fn on_paste(app: &mut App, text: String) {
    let one_line = text.replace(['\n', '\r'], " ");
    match app.edit {
        EditTarget::Url => {
            app.url_ta.insert_str(one_line.trim());
        }
        EditTarget::Body => {
            app.body_ta.insert_str(&text);
        }
        EditTarget::Cell => {
            if let Some(cell) = &mut app.cell {
                cell.ta.insert_str(one_line.trim());
            }
        }
        EditTarget::Auth => {
            if let Some(ta) = &mut app.auth_ta {
                ta.insert_str(one_line.trim());
            }
        }
        EditTarget::None => {
            if let Overlay::Prompt { ta, .. } = &mut app.overlay {
                ta.insert_str(one_line.trim());
            }
        }
    }
}

fn hit(r: ratatui::layout::Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

/// Commit whatever is being typed; every call is a no-op when nothing is active.
fn end_edits(app: &mut App) {
    app.commit_cell();
    app.commit_auth();
    app.stop_edit_body();
    app.stop_edit_url();
}

/// Put the caret where the pointer landed and arm a drag selection.
fn press_field(app: &mut App, col: u16, row: u16) {
    app.clear_resp_sel();
    let Some((ta, view, rect)) = app.editor() else { return };
    let (r, c) = view.pos_at(ta, rect, col, row);
    ta.cancel_selection();
    ta.move_cursor(CursorMove::Jump(r, c));
    ta.start_selection();
    app.drag = Drag::Field;
}

/// Extend whatever the button grabbed on its way down.
fn drag_to(app: &mut App, col: u16, row: u16) {
    match app.drag {
        Drag::Thumb(_) => drag_resp_sb(app, row),
        Drag::Field => {
            if let Some((ta, view, rect)) = app.editor() {
                let (r, c) = view.pos_at(ta, rect, col, row);
                ta.move_cursor(CursorMove::Jump(r, c));
            }
        }
        Drag::Response => app.extend_resp_sel(col, row),
        Drag::None => {}
    }
}

/// Let go: a drag that selected something copies it, a press that never moved
/// selects nothing.
fn release_drag(app: &mut App, col: u16, row: u16) {
    if app.drag == Drag::None {
        return;
    }
    drag_to(app, col, row);
    match std::mem::replace(&mut app.drag, Drag::None) {
        Drag::Field => {
            let selected = match app.editor() {
                Some((ta, _, _)) => match ta.selection_range() {
                    // A press that never moved: no selection, just a caret.
                    Some((start, end)) if start == end => {
                        ta.cancel_selection();
                        false
                    }
                    range => range.is_some(),
                },
                None => false,
            };
            if selected {
                app.copy_field_selection();
            }
        }
        // The text under the selection is only known once it is painted, so the
        // copy happens at the end of the next draw.
        Drag::Response => {
            if app.resp_sel.is_some_and(|s| s.is_click()) {
                app.clear_resp_sel();
            } else {
                app.copy_pending = true;
            }
        }
        Drag::Thumb(_) | Drag::None => {}
    }
}

pub fn on_mouse(app: &mut App, ev: MouseEvent) {
    // A drag owns the mouse until the button comes up, wherever it ends up.
    match ev.kind {
        MouseEventKind::Drag(MouseButton::Left) => return drag_to(app, ev.column, ev.row),
        MouseEventKind::Up(MouseButton::Left) => return release_drag(app, ev.column, ev.row),
        _ => {}
    }

    // The method dropdown owns the mouse while it is open.
    let popup = app.method_popup();
    if let Overlay::Method { sel } = &mut app.overlay {
        let items = ratatui::layout::Rect {
            x: popup.x + 1,
            y: popup.y + 1,
            width: popup.width.saturating_sub(2),
            height: Method::ALL.len() as u16,
        };
        match ev.kind {
            MouseEventKind::Moved => {
                app.hover_text = false;
                if hit(items, ev.column, ev.row) {
                    *sel = (ev.row - items.y) as usize;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if hit(items, ev.column, ev.row) {
                    app.draft.method = Method::ALL[(ev.row - items.y) as usize];
                }
                // Click anywhere else just dismisses the menu.
                app.overlay = Overlay::None;
                app.hover_text = false;
            }
            MouseEventKind::ScrollDown => *sel = (*sel + 1) % Method::ALL.len(),
            MouseEventKind::ScrollUp => {
                *sel = (*sel + Method::ALL.len() - 1) % Method::ALL.len()
            }
            _ => {}
        }
        return;
    }

    if matches!(ev.kind, MouseEventKind::Moved) {
        // Arrow everywhere; I-beam only over the field actively being edited.
        app.hover_text = app.in_insert() && hit(app.edit_rect, ev.column, ev.row);
        return;
    }

    // A press inside the open editor lands the caret there — overlays included,
    // so the save prompt and the env grid behave like any other field.
    if matches!(ev.kind, MouseEventKind::Down(MouseButton::Left))
        && app.in_insert()
        && hit(app.edit_rect, ev.column, ev.row)
    {
        return press_field(app, ev.column, ev.row);
    }

    if !matches!(app.overlay, Overlay::None) {
        return;
    }
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let (c, r) = (ev.column, ev.row);
            // Any press retires the response selection; the branches below open
            // a fresh one when the press lands in the response text.
            app.clear_resp_sel();

            // Tab labels take priority over their surrounding pane.
            if let Some(target) = app
                .tab_hits
                .iter()
                .find(|(rect, _)| hit(*rect, c, r))
                .map(|(_, t)| *t)
            {
                end_edits(app);
                match target {
                    TabTarget::Req(tab) => {
                        app.focus = Focus::ReqContent;
                        app.req_tab = tab;
                    }
                    TabTarget::Resp(tab) => {
                        app.focus = Focus::Response;
                        if app.resp_tab != tab {
                            app.resp_tab = tab;
                            app.resp_scroll = 0;
                        }
                    }
                    TabTarget::Side(tab) => {
                        app.focus = Focus::Sidebar;
                        if app.side_tab != tab {
                            app.side_tab = tab;
                            app.side_sel = 0;
                        }
                    }
                    TabTarget::Method => app.open_method_menu(),
                }
                return;
            }

            // The scrollbar is one column wide; the pane border beside it counts
            // as part of the grab target so it is not a pixel hunt.
            let grab_area = ratatui::layout::Rect { width: 2, ..app.resp_sb };
            if app.resp_sb.height > 0 && hit(grab_area, c, r) {
                end_edits(app);
                app.focus = Focus::Response;
                press_resp_sb(app, r);
                return;
            }

            if app.sidebar_open && hit(app.rects.sidebar, c, r) {
                if app.edit != EditTarget::None {
                    end_edits(app);
                }
                app.focus = Focus::Sidebar;
                let list_top = app.rects.sidebar.y + 3;
                if r >= list_top {
                    let idx = app.side_off + (r - list_top) as usize;
                    if idx < app.side_len() {
                        // Clicking a row selects and opens it in one go.
                        app.side_sel = idx;
                        open_side_selected(app);
                    }
                }
            } else if hit(app.rects.url, c, r) {
                app.start_edit_url();
                // Landing on the text itself puts the caret there, not at the end.
                if hit(app.url_rect, c, r) {
                    press_field(app, c, r);
                }
            } else if hit(app.rects.request, c, r) {
                if app.edit == EditTarget::Url {
                    app.stop_edit_url();
                }
                app.focus = Focus::ReqContent;
                // The JSON/text body is a field: clicking it starts editing right
                // where the pointer landed.
                if hit(app.body_rect, c, r) {
                    app.start_edit_body();
                    press_field(app, c, r);
                }
            } else if hit(app.rects.response, c, r) {
                if app.edit == EditTarget::Url {
                    app.stop_edit_url();
                }
                app.focus = Focus::Response;
                if hit(app.resp_rect, c, r) {
                    app.begin_resp_sel(c, r);
                    app.drag = Drag::Response;
                }
            }
        }
        MouseEventKind::ScrollDown => {
            if hit(app.rects.response, ev.column, ev.row) {
                scroll_resp(app, 3);
            } else if app.sidebar_open && hit(app.rects.sidebar, ev.column, ev.row) {
                let len = app.side_len();
                if len > 0 {
                    app.side_sel = (app.side_sel + 1).min(len - 1);
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if hit(app.rects.response, ev.column, ev.row) {
                scroll_resp(app, -3);
            } else if app.sidebar_open && hit(app.rects.sidebar, ev.column, ev.row) {
                app.side_sel = app.side_sel.saturating_sub(1);
            }
        }
        _ => {}
    }
}
