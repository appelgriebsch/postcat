//! Request pane: URL bar, tab strip, and the per-tab editors.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};
use ratatui::Frame;

use crate::app::{App, EditTarget, Focus, Overlay, ReqTab, TabTarget};
use crate::model::{active_count, AuthType, BodyType};
use crate::theme;

pub fn draw_url_bar(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Url;
    let editing = app.edit == EditTarget::Url;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border(focused));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 20 {
        return;
    }

    let [method_a, url_a, hint_a] = Layout::horizontal([
        Constraint::Length(13),
        Constraint::Min(10),
        Constraint::Length(9),
    ])
    .areas(inner);

    // The " GET     ▾ " chip is clickable and opens the method dropdown.
    app.tab_hits.push((
        Rect { x: inner.x, y: inner.y, width: 11, height: 1 },
        TabTarget::Method,
    ));

    let m = app.draft.method;
    let method_line = Line::from(vec![
        Span::styled(
            format!(" {:<8}", m.as_str()),
            Style::new().fg(theme::method_color(m)).add_modifier(Modifier::BOLD),
        ),
        Span::styled("▾ ", Style::new().fg(theme::DIM)),
        Span::styled("│ ", Style::new().fg(theme::BORDER)),
    ]);
    f.render_widget(Paragraph::new(method_line), method_a);

    f.render_widget(&app.url_ta, url_a);
    app.url_rect = url_a;
    app.views.url.sync(&app.url_ta, url_a);
    if editing {
        app.edit_rect = url_a;
    }

    let hint = if editing {
        Line::from(vec![
            Span::styled("⏎ ", theme::key_hint()),
            Span::styled("send ", theme::hint_text()),
        ])
    } else if focused {
        Line::from(vec![
            Span::styled("i ", theme::key_hint()),
            Span::styled("edit ", theme::hint_text()),
        ])
    } else {
        Line::from(Span::styled("", theme::hint_text()))
    };
    f.render_widget(Paragraph::new(hint).alignment(Alignment::Right), hint_a);
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::ReqContent;
    let mut title_spans = vec![Span::styled(" Request ", theme::title(focused))];
    if !app.draft.name.is_empty() {
        title_spans.push(Span::styled(
            format!("· {} ", app.draft.name),
            Style::new().fg(theme::PURPLE),
        ));
    }
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border(focused))
        .title(Line::from(title_spans));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 3 || inner.width < 20 {
        return;
    }

    draw_tabs(f, app, Rect { height: 1, ..inner });

    let content = Rect {
        x: inner.x + 1,
        y: inner.y + 2,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(2),
    };

    match app.req_tab {
        ReqTab::Query => {
            let editor =
                super::grid::draw_kv_grid(f, content, &app.draft.params, app.params_sel, focused, pane_cell(app));
            if let Some(r) = editor {
                app.edit_rect = r;
                app.sync_inline(r);
            }
        }
        ReqTab::Headers => {
            let editor =
                super::grid::draw_kv_grid(f, content, &app.draft.headers, app.headers_sel, focused, pane_cell(app));
            if let Some(r) = editor {
                app.edit_rect = r;
                app.sync_inline(r);
            }
        }
        ReqTab::Body => draw_body(f, app, content, focused),
        ReqTab::Auth => draw_auth(f, app, content, focused),
    }
}

/// The inline cell editor belongs to this pane only when no overlay is open.
fn pane_cell(app: &App) -> Option<&crate::app::CellEdit> {
    if matches!(app.overlay, Overlay::None) {
        app.cell.as_ref()
    } else {
        None
    }
}

fn draw_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let counts = [
        active_count(&app.draft.params),
        active_count(&app.draft.headers),
        0,
        0,
    ];
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    let mut x = area.x + 1;
    for (i, tab) in ReqTab::ALL.iter().enumerate() {
        let active = *tab == app.req_tab;
        let label = match tab {
            ReqTab::Query => "Query",
            ReqTab::Headers => "Headers",
            ReqTab::Body => "Body",
            ReqTab::Auth => "Auth",
        };
        if i > 0 {
            spans.push(Span::raw("  "));
            x += 2;
        }
        let style = if active {
            Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new().fg(theme::DIM)
        };
        spans.push(Span::styled(label, style));

        // Badges: counts / body type / auth type.
        let badge: Option<String> = match tab {
            ReqTab::Query | ReqTab::Headers => {
                (counts[i] > 0).then(|| format!(" {}", counts[i]))
            }
            ReqTab::Body => (app.draft.body_type != BodyType::None)
                .then(|| format!(" {}", app.draft.body_type.as_str())),
            ReqTab::Auth => (app.draft.auth.typ != AuthType::None)
                .then(|| format!(" {}", app.draft.auth.typ.as_str())),
        };
        let mut width = label.chars().count() as u16;
        if let Some(b) = badge {
            let badge_style = if active {
                Style::new().fg(theme::TEAL)
            } else {
                Style::new().fg(theme::BORDER)
            };
            width += b.chars().count() as u16;
            spans.push(Span::styled(b, badge_style));
        }
        app.tab_hits.push((
            Rect { x, y: area.y, width, height: 1 },
            TabTarget::Req(*tab),
        ));
        x += width;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_body(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    if area.height < 2 {
        return;
    }
    // Type selector line.
    let bt = app.draft.body_type;
    let mut spans = vec![
        Span::styled("type ", Style::new().fg(theme::DIM)),
        Span::styled("‹ ", Style::new().fg(theme::BORDER)),
        Span::styled(
            bt.as_str(),
            Style::new().fg(theme::TEAL).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ›", Style::new().fg(theme::BORDER)),
        Span::styled("   t ", theme::key_hint()),
        Span::styled("change", theme::hint_text()),
    ];
    if bt == BodyType::Json {
        spans.push(Span::styled("   f ", theme::key_hint()));
        spans.push(Span::styled("format", theme::hint_text()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), Rect { height: 1, ..area });

    let content = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: area.height.saturating_sub(2),
    };
    match bt {
        BodyType::None => {
            let msg = Line::from(vec![
                Span::styled("no body — press ", Style::new().fg(theme::DIM)),
                Span::styled("t", theme::key_hint()),
                Span::styled(" to pick a type, ", Style::new().fg(theme::DIM)),
                Span::styled("⏎", theme::key_hint()),
                Span::styled(" to start typing JSON", Style::new().fg(theme::DIM)),
            ]);
            f.render_widget(Paragraph::new(msg), content);
        }
        BodyType::Form => {
            let editor =
                super::grid::draw_kv_grid(f, content, &app.draft.form, app.form_sel, focused, pane_cell(app));
            if let Some(r) = editor {
                app.edit_rect = r;
                app.sync_inline(r);
            }
        }
        BodyType::Json | BodyType::Text => {
            f.render_widget(&app.body_ta, content);
            app.body_rect = content;
            app.views.body.sync(&app.body_ta, content);
            if app.edit == EditTarget::Body {
                app.edit_rect = content;
            }
        }
    }
}

fn draw_auth(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let t = app.draft.auth.typ;
    let spans = vec![
        Span::styled("type ", Style::new().fg(theme::DIM)),
        Span::styled("‹ ", Style::new().fg(theme::BORDER)),
        Span::styled(
            t.as_str(),
            Style::new().fg(theme::TEAL).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ›", Style::new().fg(theme::BORDER)),
        Span::styled("   t ", theme::key_hint()),
        Span::styled("change", theme::hint_text()),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), Rect { height: 1, ..area });

    let fields: Vec<(&str, String)> = match t {
        AuthType::None => vec![],
        AuthType::Bearer => vec![("token", app.draft.auth.token.clone())],
        AuthType::Basic => vec![
            ("username", app.draft.auth.user.clone()),
            ("password", "•".repeat(app.draft.auth.pass.chars().count())),
        ],
    };

    if fields.is_empty() {
        let msg = Line::styled(
            "requests are sent without authentication",
            Style::new().fg(theme::DIM),
        );
        f.render_widget(
            Paragraph::new(msg),
            Rect { x: area.x, y: area.y + 2, width: area.width, height: 1 },
        );
        return;
    }

    let label_w: usize = 10;
    for (i, (label, value)) in fields.iter().enumerate() {
        let y = area.y + 2 + i as u16 * 2;
        if y >= area.y + area.height {
            break;
        }
        let selected = focused && app.auth_sel == i;
        let row_rect = Rect { x: area.x, y, width: area.width, height: 1 };
        let bg = if selected { Some(theme::SEL_BG) } else { None };
        let sty = |mut s: Style| {
            if let Some(b) = bg {
                s = s.bg(b);
            }
            s
        };
        let marker = if selected {
            Span::styled("▌", sty(Style::new().fg(theme::ACCENT)))
        } else {
            Span::styled(" ", sty(Style::new()))
        };
        let label_span = Span::styled(
            format!("{label:>label_w$}  "),
            sty(Style::new().fg(theme::DIM)),
        );
        let shown = if value.is_empty() { "·".to_string() } else { value.clone() };
        let val_style = if value.is_empty() {
            sty(Style::new().fg(theme::BORDER))
        } else {
            sty(Style::new().fg(theme::FG))
        };
        let val_w = (area.width as usize).saturating_sub(label_w + 3);
        let value_span = Span::styled(format!("{:<val_w$}", shown), val_style);
        f.render_widget(
            Paragraph::new(Line::from(vec![marker, label_span, value_span])),
            row_rect,
        );

        // Inline editor over the value area.
        if selected && app.edit == EditTarget::Auth
            && let Some(ta) = &app.auth_ta {
                let rect = Rect {
                    x: area.x + label_w as u16 + 3,
                    y,
                    width: area.width.saturating_sub(label_w as u16 + 3),
                    height: 1,
                };
                f.render_widget(ta, rect);
                app.views.inline.sync(ta, rect);
                app.edit_rect = rect;
            }
    }
}
