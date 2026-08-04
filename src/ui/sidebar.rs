//! Library sidebar: saved requests + history.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};
use ratatui::Frame;

use crate::app::{App, Focus, SideTab, TabTarget};
use crate::theme;

fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        s.to_string()
    } else if width <= 1 {
        "…".into()
    } else {
        let mut out: String = s.chars().take(width - 1).collect();
        out.push('…');
        out
    }
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Sidebar;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border(focused))
        .title(Span::styled(" Library ", theme::title(focused)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 3 || inner.width < 10 {
        return;
    }

    // Tab row.
    let tab = |label: String, active: bool| {
        if active {
            Span::styled(
                label,
                Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
        } else {
            Span::styled(label, Style::new().fg(theme::DIM))
        }
    };
    let saved_label = format!("Saved {}", app.saved.len());
    let history_label = format!("History {}", app.history.len());
    let saved_w = saved_label.chars().count() as u16;
    let history_w = history_label.chars().count() as u16;
    let tabs = Line::from(vec![
        Span::raw(" "),
        tab(saved_label, app.side_tab == SideTab::Saved),
        Span::styled("  │  ", Style::new().fg(theme::BORDER)),
        tab(history_label, app.side_tab == SideTab::History),
    ]);
    app.tab_hits.push((
        Rect { x: inner.x + 1, y: inner.y, width: saved_w, height: 1 },
        TabTarget::Side(SideTab::Saved),
    ));
    app.tab_hits.push((
        Rect { x: inner.x + 1 + saved_w + 5, y: inner.y, width: history_w, height: 1 },
        TabTarget::Side(SideTab::History),
    ));
    f.render_widget(Paragraph::new(tabs), Rect { height: 1, ..inner });

    let list = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };

    let len = app.side_len();
    if len == 0 {
        let msg: Vec<Line> = match app.side_tab {
            SideTab::Saved => vec![
                Line::styled("nothing saved yet", Style::new().fg(theme::DIM)),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("^s", theme::key_hint()),
                    Span::styled(" saves the open request", Style::new().fg(theme::DIM)),
                ]),
            ],
            SideTab::History => vec![
                Line::styled("no requests sent yet", Style::new().fg(theme::DIM)),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("⏎", theme::key_hint()),
                    Span::styled(" in the url bar sends", Style::new().fg(theme::DIM)),
                ]),
            ],
        };
        let pad = Rect { x: list.x + 2, width: list.width.saturating_sub(2), ..list };
        f.render_widget(Paragraph::new(msg), pad);
        app.side_off = 0;
        return;
    }

    let visible = list.height as usize;
    let sel = app.side_sel.min(len - 1);
    app.side_sel = sel;
    let off = sel.saturating_sub(visible.saturating_sub(1));
    app.side_off = off;

    for (vis, idx) in (off..len.min(off + visible)).enumerate() {
        let y = list.y + vis as u16;
        let selected = idx == sel;
        let row_rect = Rect { x: list.x, y, width: list.width, height: 1 };
        let bg = if selected && focused { Some(theme::SEL_BG) } else { None };

        let (method, name, status): (_, String, Option<u16>) = match app.side_tab {
            SideTab::Saved => {
                let r = &app.saved[idx];
                (r.method, r.display_name().to_string(), None)
            }
            SideTab::History => {
                let h = &app.history[idx];
                (h.request.method, h.request.display_name().to_string(), h.status)
            }
        };

        let with_bg = |mut s: Style| {
            if let Some(b) = bg {
                s = s.bg(b);
            }
            s
        };

        let marker = if selected {
            Span::styled("▌", with_bg(Style::new().fg(theme::ACCENT)))
        } else {
            Span::styled(" ", with_bg(Style::new()))
        };
        let method_span = Span::styled(
            format!("{} ", method.short()),
            with_bg(Style::new().fg(theme::method_color(method)).add_modifier(Modifier::BOLD)),
        );

        let status_w = if app.side_tab == SideTab::History { 4 } else { 0 };
        let name_w = (list.width as usize).saturating_sub(6 + status_w + 1);
        let name_style = if selected {
            with_bg(Style::new().fg(theme::FG))
        } else {
            with_bg(Style::new().fg(theme::PALE))
        };
        let name_span = Span::styled(
            format!("{:<w$}", truncate(&name, name_w), w = name_w),
            name_style,
        );

        let mut spans = vec![marker, method_span, name_span];
        if app.side_tab == SideTab::History {
            let (txt, color) = match status {
                Some(code) => (format!("{code:>4}"), theme::status_color(code)),
                None => ("   ✗".to_string(), theme::RED),
            };
            spans.push(Span::styled(txt, with_bg(Style::new().fg(color))));
        }
        spans.push(Span::styled(" ", with_bg(Style::new())));
        f.render_widget(Paragraph::new(Line::from(spans)), row_rect);
    }
}
