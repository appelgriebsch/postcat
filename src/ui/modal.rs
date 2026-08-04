//! Overlays: help, save/rename prompt, delete confirm, environment editor.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};
use ratatui::Frame;

use crate::app::{App, EditTarget, Overlay};
use crate::model::Method;
use crate::theme;

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn modal_block(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme::ACCENT))
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    match &app.overlay {
        Overlay::None => {}
        Overlay::Help { scroll } => draw_help(f, *scroll),
        Overlay::Prompt { title, ta, .. } => {
            let area = centered(f.area(), 52, 5);
            f.render_widget(Clear, area);
            let block = modal_block(title);
            let inner = block.inner(area);
            f.render_widget(block, area);
            let ta_rect = Rect { height: 1, ..inner };
            f.render_widget(ta, ta_rect);
            app.views.inline.sync(ta, ta_rect);
            app.edit_rect = ta_rect;
            let hint = Line::from(vec![
                Span::styled("⏎ ", theme::key_hint()),
                Span::styled("confirm", theme::hint_text()),
                Span::styled("   esc ", theme::key_hint()),
                Span::styled("cancel", theme::hint_text()),
            ]);
            f.render_widget(
                Paragraph::new(hint),
                Rect { y: inner.y + 2, height: 1, ..inner },
            );
        }
        Overlay::Confirm { msg, .. } => {
            let w = (msg.chars().count() as u16 + 6).clamp(30, 70);
            let area = centered(f.area(), w, 5);
            f.render_widget(Clear, area);
            let block = modal_block("Confirm");
            let inner = block.inner(area);
            f.render_widget(block, area);
            f.render_widget(
                Paragraph::new(Line::styled(msg.clone(), Style::new().fg(theme::FG))),
                Rect { height: 1, ..inner },
            );
            let hint = Line::from(vec![
                Span::styled("y ", theme::key_hint()),
                Span::styled("delete", Style::new().fg(theme::RED)),
                Span::styled("   n ", theme::key_hint()),
                Span::styled("keep it", theme::hint_text()),
            ]);
            f.render_widget(
                Paragraph::new(hint),
                Rect { y: inner.y + 2, height: 1, ..inner },
            );
        }
        Overlay::Env => draw_env(f, app),
        Overlay::Method { sel } => draw_method_menu(f, app, *sel),
    }
}

fn draw_method_menu(f: &mut Frame, app: &App, sel: usize) {
    let area = app.method_popup().intersection(f.area());
    if area.height < 3 || area.width < 6 {
        return;
    }
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme::ACCENT))
        .title(Span::styled(" Method ", Style::new().fg(theme::DIM)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    for (i, m) in Method::ALL.iter().enumerate() {
        let y = inner.y + i as u16;
        if y >= inner.y + inner.height {
            break;
        }
        let cursor = i == sel;
        let bg = if cursor { Some(theme::SEL_BG) } else { None };
        let sty = |mut s: Style| {
            if let Some(b) = bg {
                s = s.bg(b);
            }
            s
        };
        let marker = if cursor {
            Span::styled("▌", sty(Style::new().fg(theme::ACCENT)))
        } else {
            Span::styled(" ", sty(Style::new()))
        };
        let name = Span::styled(
            format!(" {:<7}", m.as_str()),
            sty(Style::new()
                .fg(theme::method_color(*m))
                .add_modifier(Modifier::BOLD)),
        );
        let current = if *m == app.draft.method {
            Span::styled(" ●", sty(Style::new().fg(theme::DIM)))
        } else {
            Span::styled("  ", sty(Style::new()))
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![marker, name, current])),
            Rect { x: inner.x, y, width: inner.width, height: 1 },
        );
    }
}

fn draw_env(f: &mut Frame, app: &mut App) {
    let area = centered(f.area(), 68, 18);
    f.render_widget(Clear, area);
    let block = modal_block("Environment · {{vars}}");
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 4 {
        return;
    }
    f.render_widget(
        Paragraph::new(Line::styled(
            "values replace {{name}} in url, params, headers, body & auth",
            Style::new().fg(theme::DIM),
        )),
        Rect { height: 1, ..inner },
    );
    let grid_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };
    let cell = if app.edit == EditTarget::Cell {
        app.cell.as_ref()
    } else {
        None
    };
    let editor = super::grid::draw_kv_grid(f, grid_area, &app.env, app.env_sel, true, cell);
    if let Some(r) = editor {
        app.edit_rect = r;
        app.sync_inline(r);
    }
}

fn draw_help(f: &mut Frame, scroll: u16) {
    let area = centered(f.area(), 74, 28);
    f.render_widget(Clear, area);
    let block = modal_block("Keys");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let section = |name: &'static str| {
        Line::from(Span::styled(
            name,
            Style::new().fg(theme::PURPLE).add_modifier(Modifier::BOLD),
        ))
    };
    let bind = |keys: &'static str, what: &'static str| {
        Line::from(vec![
            Span::styled(format!("  {keys:<14}"), theme::key_hint()),
            Span::styled(what, Style::new().fg(theme::PALE)),
        ])
    };

    let lines: Vec<Line> = vec![
        section("Panes"),
        bind("⇥ / ⇧⇥", "cycle panes"),
        bind("1 2 3 4", "library · url · request · response"),
        bind("^b", "show / hide the library"),
        Line::raw(""),
        section("Request"),
        bind("i  or  u", "edit the url (⏎ sends, esc keeps)"),
        bind("m / M", "cycle method · or click the chip"),
        bind("s  or  ⏎", "send   ·   esc cancels while sending"),
        bind("[ ]  h l", "switch Query / Headers / Body / Auth"),
        bind("⏎ / i", "edit selected row or field"),
        bind("a · d · ␣", "add · delete · toggle a row"),
        bind("t", "cycle body or auth type"),
        bind("f", "pretty-print the JSON body"),
        Line::raw(""),
        section("While editing"),
        bind("⇥", "jump key ⇄ value"),
        bind("⏎", "next field / grow the grid"),
        bind("^u", "clear the url"),
        bind("esc", "done"),
        Line::raw(""),
        section("Response"),
        bind("j k  ^d ^u", "scroll · half-page"),
        bind("g / G", "top / bottom"),
        bind("[ ]", "body ⇄ headers"),
        bind("w", "toggle line-wrap"),
        Line::raw(""),
        section("Selecting & copying"),
        bind("drag", "select in a field or the response"),
        bind("⇧ + ← →", "select while editing"),
        bind("y", "copy the selection · or the whole body"),
        bind("esc", "drop the selection"),
        Line::raw(""),
        section("Library & workspace"),
        bind("^s", "save request under a name"),
        bind("n", "new blank request"),
        bind("⏎", "open the selected entry"),
        bind("d · r", "delete · rename"),
        bind("e", "environment vars ({{name}})"),
        bind("q  ^c", "quit (workspace is saved)"),
    ];

    f.render_widget(
        Paragraph::new(lines).scroll((scroll, 0)),
        Rect { height: inner.height.saturating_sub(1), ..inner },
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("esc ", theme::key_hint()),
            Span::styled("close", theme::hint_text()),
        ]))
        .alignment(Alignment::Right),
        Rect { y: inner.y + inner.height - 1, height: 1, ..inner },
    );
}
