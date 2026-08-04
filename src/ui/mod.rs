//! Root layout: header, three panes, status bar, overlays.

pub mod grid;
pub mod json;
pub mod modal;
pub mod request;
pub mod response;
pub mod sidebar;

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, EditTarget, Focus, Overlay, ReqTab, ToastKind, SPINNER};
use crate::model::BodyType;
use crate::theme;

pub fn draw(f: &mut Frame, app: &mut App) {
    app.tab_hits.clear();
    app.edit_rect = Rect::default();
    app.resp_sb = Rect::default();
    // Cleared so a field that fell off the screen stops attracting clicks.
    app.url_rect = Rect::default();
    app.body_rect = Rect::default();
    app.resp_rect = Rect::default();
    // A blank row above and below keeps the header and mode chips from bleeding
    // their backgrounds into the terminal's top and bottom edges.
    let [_, header, main, status, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    draw_header(f, app, header);

    let (side_area, right) = if app.sidebar_open {
        let [s, r] =
            Layout::horizontal([Constraint::Length(32), Constraint::Min(40)]).areas(main);
        (Some(s), r)
    } else {
        (None, main)
    };

    let [url_area, req_area, resp_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Percentage(42),
        Constraint::Min(6),
    ])
    .areas(right);

    app.rects.sidebar = side_area.unwrap_or_default();
    app.rects.url = url_area;
    app.rects.request = req_area;
    app.rects.response = resp_area;

    if let Some(s) = side_area {
        sidebar::draw(f, app, s);
    }
    request::draw_url_bar(f, app, url_area);
    request::draw(f, app, req_area);
    response::draw(f, app, resp_area);
    // Runs over the rendered frame, so it highlights whatever the response pane
    // actually put on screen.
    response::paint_selection(f, app);
    draw_status(f, app, status);
    modal::draw(f, app);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let left = Line::from(vec![
        Span::raw(" "),
        Span::styled(
            " ⚡ postcat ",
            Style::new()
                .fg(theme::CHIP_FG)
                .bg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(concat!("  v", env!("CARGO_PKG_VERSION")), Style::new().fg(theme::DIM)),
    ]);
    f.render_widget(Paragraph::new(left), area);

    let env_active = crate::model::active_count(&app.env);
    let mut right_spans = Vec::new();
    if env_active > 0 {
        right_spans.push(Span::styled(
            format!("⬢ {env_active} env var{}", if env_active == 1 { "" } else { "s" }),
            Style::new().fg(theme::PURPLE),
        ));
        right_spans.push(Span::styled("  ·  ", Style::new().fg(theme::BORDER)));
    }
    right_spans.push(Span::styled("? ", theme::key_hint()));
    right_spans.push(Span::styled("help ", Style::new().fg(theme::DIM)));
    f.render_widget(
        Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right),
        area,
    );
}

fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    match &app.overlay {
        Overlay::Help { .. } => return vec![("j/k", "scroll"), ("esc", "close")],
        Overlay::Prompt { .. } => return vec![("⏎", "confirm"), ("esc", "cancel")],
        Overlay::Confirm { .. } => return vec![("y", "yes"), ("n", "no")],
        Overlay::Env => {
            return if app.edit == EditTarget::Cell {
                vec![("⏎", "next"), ("⇥", "key⇄value"), ("esc", "done")]
            } else {
                vec![("⏎", "edit"), ("a", "add"), ("d", "delete"), ("␣", "toggle"), ("esc", "close")]
            }
        }
        Overlay::Method { .. } => {
            return vec![("j/k", "choose"), ("⏎", "select"), ("esc", "close")]
        }
        Overlay::None => {}
    }
    match app.edit {
        EditTarget::Url => return vec![("⏎", "send"), ("esc", "done"), ("⇥", "params")],
        EditTarget::Body => return vec![("esc", "done editing")],
        EditTarget::Cell => {
            return vec![("⏎", "next"), ("⇥", "key⇄value"), ("↑↓", "row"), ("esc", "done")]
        }
        EditTarget::Auth => return vec![("⏎", "next"), ("esc", "done")],
        EditTarget::None => {}
    }
    match app.focus {
        Focus::Sidebar => vec![
            ("⏎", "open"),
            ("h/l", "saved⇄history"),
            ("d", "delete"),
            ("r", "rename"),
            ("n", "new"),
        ],
        Focus::Url => vec![
            ("i", "edit url"),
            ("⏎", "send"),
            ("m", "method"),
            ("^s", "save"),
            ("⇥", "next pane"),
        ],
        Focus::ReqContent => match app.req_tab {
            ReqTab::Query | ReqTab::Headers => vec![
                ("⏎", "edit"),
                ("a", "add"),
                ("␣", "toggle"),
                ("d", "delete"),
                ("[ ]", "tab"),
                ("s", "send"),
            ],
            ReqTab::Body => {
                if app.draft.body_type == BodyType::Form {
                    vec![("⏎", "edit"), ("a", "add"), ("t", "type"), ("[ ]", "tab"), ("s", "send")]
                } else {
                    vec![("⏎", "edit"), ("t", "type"), ("f", "format"), ("[ ]", "tab"), ("s", "send")]
                }
            }
            ReqTab::Auth => vec![("t", "type"), ("⏎", "edit"), ("[ ]", "tab"), ("s", "send")],
        },
        Focus::Response => vec![
            ("j/k", "scroll"),
            ("[ ]", "body⇄headers"),
            ("g/G", "top/end"),
            ("w", "wrap"),
            ("y", "copy"),
            ("s", "send again"),
        ],
    }
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let insert = app.in_insert();
    let (mode_label, mode_style) = if insert {
        (
            " INSERT ",
            Style::new()
                .fg(theme::CHIP_FG)
                .bg(theme::GREEN)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            " NORMAL ",
            Style::new()
                .fg(theme::PALE)
                .bg(theme::BORDER)
                .add_modifier(Modifier::BOLD),
        )
    };

    let mut spans = vec![Span::raw(" "), Span::styled(mode_label, mode_style), Span::raw("  ")];
    for (i, (key, desc)) in hints(app).into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ·  ", Style::new().fg(theme::BORDER)));
        }
        spans.push(Span::styled(key, theme::key_hint()));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(desc, Style::new().fg(theme::DIM)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);

    // Right side: in-flight spinner or toast.
    let mut right: Vec<Span> = Vec::new();
    if app.loading {
        right.push(Span::styled(
            format!("{} sending… ", SPINNER[app.spin % SPINNER.len()]),
            Style::new().fg(theme::ACCENT),
        ));
        right.push(Span::styled("esc ", theme::key_hint()));
        right.push(Span::styled("cancel ", Style::new().fg(theme::DIM)));
    } else if let Some((msg, kind, _)) = &app.toast {
        let (icon, color) = match kind {
            ToastKind::Ok => ("", theme::GREEN),
            ToastKind::Err => ("✗ ", theme::RED),
            ToastKind::Info => ("", theme::ACCENT),
        };
        right.push(Span::styled(format!("{icon}{msg} "), Style::new().fg(color)));
    }
    if !right.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(right)).alignment(Alignment::Right),
            area,
        );
    }
}
