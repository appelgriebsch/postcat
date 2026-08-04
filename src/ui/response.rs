//! Response pane: status line, body/headers tabs, scrolling.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::Frame;

use crate::app::{App, Focus, RespTab, TabTarget, SPINNER};
use crate::model::human_size;
use crate::theme;

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Response;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border(focused))
        .title(Span::styled(" Response ", theme::title(focused)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 3 || inner.width < 20 {
        return;
    }

    // Tab strip + meta line.
    let header_count = app.resp.as_ref().map(|r| r.data.headers.len()).unwrap_or(0);
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
    let mut tabs = vec![
        Span::raw(" "),
        tab("Body".into(), app.resp_tab == RespTab::Body),
        Span::raw("  "),
        tab("Headers".into(), app.resp_tab == RespTab::Headers),
    ];
    let mut headers_w: u16 = 7;
    if header_count > 0 {
        let badge = format!(" {header_count}");
        headers_w += badge.chars().count() as u16;
        tabs.push(Span::styled(badge, Style::new().fg(theme::BORDER)));
    }
    app.tab_hits.push((
        Rect { x: inner.x + 1, y: inner.y, width: 4, height: 1 },
        TabTarget::Resp(RespTab::Body),
    ));
    app.tab_hits.push((
        Rect { x: inner.x + 7, y: inner.y, width: headers_w, height: 1 },
        TabTarget::Resp(RespTab::Headers),
    ));
    let strip = Rect { height: 1, ..inner };
    f.render_widget(Paragraph::new(Line::from(tabs)), strip);
    f.render_widget(
        Paragraph::new(meta_line(app)).alignment(Alignment::Right),
        strip,
    );

    let content = Rect {
        x: inner.x + 1,
        y: inner.y + 2,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(2),
    };
    app.resp_rect = content;

    if app.loading {
        return draw_center(
            f,
            content,
            vec![
                Line::from(Span::styled(
                    format!("{}  sending request…", SPINNER[app.spin % SPINNER.len()]),
                    Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
                )),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("esc ", theme::key_hint()),
                    Span::styled("cancel", theme::hint_text()),
                ]),
            ],
        );
    }

    if let Some(err) = &app.resp_err {
        let lines = vec![
            Line::from(Span::styled(
                "✗ request failed",
                Style::new().fg(theme::RED).add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(Span::styled(err.clone(), Style::new().fg(theme::PALE))),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), content);
        return;
    }

    let Some(view) = &app.resp else {
        return draw_center(
            f,
            content,
            vec![
                Line::from(Span::styled("⚡", Style::new().fg(theme::BORDER))),
                Line::raw(""),
                Line::from(Span::styled("ready when you are", Style::new().fg(theme::DIM))),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("i", theme::key_hint()),
                    Span::styled(" type a url   ", theme::hint_text()),
                    Span::styled("⏎", theme::key_hint()),
                    Span::styled(" send   ", theme::hint_text()),
                    Span::styled("?", theme::key_hint()),
                    Span::styled(" all keys", theme::hint_text()),
                ]),
            ],
        );
    };

    let total_lines = match app.resp_tab {
        RespTab::Body => view.body_lines,
        RespTab::Headers => view.data.headers.len(),
    };
    let visible = content.height as usize;
    app.resp_scroll = app.resp_scroll.min(total_lines.saturating_sub(1));

    let text: Text<'static> = match app.resp_tab {
        RespTab::Body => view.body_text.clone(),
        RespTab::Headers => {
            let lines: Vec<Line> = view
                .data
                .headers
                .iter()
                .map(|(k, v)| {
                    Line::from(vec![
                        Span::styled(k.clone(), Style::new().fg(theme::CYAN)),
                        Span::styled(": ", Style::new().fg(theme::DIM)),
                        Span::styled(v.clone(), Style::new().fg(theme::FG)),
                    ])
                })
                .collect();
            Text::from(lines)
        }
    };

    let mut para = Paragraph::new(text).scroll((app.resp_scroll as u16, 0));
    if app.wrap {
        para = para.wrap(Wrap { trim: false });
    }
    f.render_widget(para, content);

    if total_lines > visible {
        let mut state = ScrollbarState::new(total_lines.saturating_sub(visible))
            .position(app.resp_scroll);
        let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .track_style(Style::new().fg(theme::BORDER))
            .thumb_style(Style::new().fg(theme::DIM));
        let sb_area = Rect {
            x: inner.x + inner.width - 1,
            y: content.y,
            width: 1,
            height: content.height,
        };
        f.render_stateful_widget(sb, sb_area, &mut state);
        // Remembered so the mouse can grab the thumb.
        app.resp_sb = sb_area;
    }
}

/// Highlight the selection over whatever the pane rendered, and read the text
/// under it back out of the frame. Working on the finished cells is what makes
/// the highlight and the clipboard agree by construction — wrapped lines,
/// syntax colours, headers and all.
pub fn paint_selection(f: &mut Frame, app: &mut App) {
    if !app.resp_sel_live() {
        app.clear_resp_sel();
        return;
    }
    let Some(sel) = app.resp_sel else { return };
    let area = app.resp_rect;
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (last_x, last_y) = (area.right() - 1, area.bottom() - 1);
    let clamp = |(x, y): (u16, u16)| (x.clamp(area.x, last_x), y.clamp(area.y, last_y));
    let ((x0, y0), (x1, y1)) = {
        let (a, b) = sel.ordered();
        (clamp(a), clamp(b))
    };

    let buf = f.buffer_mut();
    let mut text = String::new();
    for y in y0..=y1 {
        let from = if y == y0 { x0 } else { area.x };
        let to = if y == y1 { x1 } else { last_x };
        let mut line = String::new();
        for x in from..=to {
            let cell = &mut buf[(x, y)];
            line.push_str(cell.symbol());
            cell.set_fg(theme::CHIP_FG).set_bg(theme::ACCENT);
        }
        text.push_str(line.trim_end());
        if y < y1 {
            text.push('\n');
        }
    }
    app.resp_sel_text = text;

    if app.copy_pending {
        app.copy_pending = false;
        let text = app.resp_sel_text.clone();
        app.copy(&text);
    }
}

fn meta_line(app: &App) -> Line<'static> {
    if app.loading {
        return Line::raw("");
    }
    if app.resp_err.is_some() {
        return Line::from(vec![
            Span::styled("✗ error ", Style::new().fg(theme::RED).add_modifier(Modifier::BOLD)),
        ]);
    }
    let Some(view) = &app.resp else {
        return Line::from(Span::styled("idle ", Style::new().fg(theme::BORDER)));
    };
    let d = &view.data;
    let color = theme::status_color(d.status);
    let mut spans = vec![
        Span::styled("● ", Style::new().fg(color)),
        Span::styled(
            format!("{} {}", d.status, d.reason),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        ),
    ];
    if app.streaming {
        spans.push(Span::styled(
            format!("  {} streaming", SPINNER[app.spin % SPINNER.len()]),
            Style::new().fg(theme::ACCENT),
        ));
        spans.push(Span::styled(
            format!("  {}", human_size(d.body.len())),
            Style::new().fg(theme::PALE),
        ));
        spans.push(Span::styled("  esc ", Style::new().fg(theme::ACCENT)));
        spans.push(Span::styled("stop ", Style::new().fg(theme::DIM)));
        return Line::from(spans);
    }
    spans.push(Span::styled(
        format!("  {} ms", d.elapsed_ms),
        Style::new().fg(theme::PALE),
    ));
    spans.push(Span::styled(
        format!("  {}", human_size(d.body.len())),
        Style::new().fg(theme::PALE),
    ));
    spans.push(Span::styled(format!("  {} ", d.version), Style::new().fg(theme::DIM)));
    if !app.wrap {
        spans.push(Span::styled("nowrap ", Style::new().fg(theme::YELLOW)));
    }
    Line::from(spans)
}

fn draw_center(f: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    let h = lines.len() as u16;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let rect = Rect { x: area.x, y, width: area.width, height: h.min(area.height) };
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        rect,
    );
}
