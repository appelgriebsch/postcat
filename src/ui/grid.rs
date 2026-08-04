//! Shared key/value grid renderer (query params, headers, form body, env vars).

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::CellEdit;
use crate::model::KV;
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

/// Returns the rect of the inline cell editor, when one was rendered.
pub fn draw_kv_grid(
    f: &mut Frame,
    area: Rect,
    rows: &[KV],
    sel: usize,
    focused: bool,
    cell: Option<&CellEdit>,
) -> Option<Rect> {
    if area.height < 2 || area.width < 16 {
        return None;
    }

    let marker_w: u16 = 2; // "▌ "
    let toggle_w: u16 = 2; // "● "
    let key_w = ((area.width - marker_w - toggle_w) as f32 * 0.38) as u16;
    let key_w = key_w.clamp(8, area.width / 2);
    let val_x = area.x + marker_w + toggle_w + key_w + 1;
    let val_w = (area.x + area.width).saturating_sub(val_x);

    // Column header.
    let header = Line::from(vec![
        Span::raw(" ".repeat((marker_w + toggle_w) as usize)),
        Span::styled(
            format!("{:<w$} ", "KEY", w = key_w as usize),
            Style::new().fg(theme::DIM).add_modifier(Modifier::BOLD),
        ),
        Span::styled("VALUE", Style::new().fg(theme::DIM).add_modifier(Modifier::BOLD)),
    ]);
    f.render_widget(Paragraph::new(header), Rect { height: 1, ..area });

    let visible = (area.height - 1) as usize;
    let sel = sel.min(rows.len().saturating_sub(1));
    let off = sel.saturating_sub(visible.saturating_sub(1));
    let mut editor_rect = None;

    for (vis, (idx, row)) in rows.iter().enumerate().skip(off).take(visible).enumerate() {
        let y = area.y + 1 + vis as u16;
        let row_rect = Rect { x: area.x, y, width: area.width, height: 1 };
        let selected = idx == sel && focused;

        let base_bg = if selected { Some(theme::SEL_BG) } else { None };
        let mut key_style = Style::new().fg(theme::FG);
        let mut val_style = Style::new().fg(theme::PALE);
        if !row.enabled {
            key_style = Style::new().fg(theme::DIM).add_modifier(Modifier::CROSSED_OUT);
            val_style = key_style;
        }
        if let Some(bg) = base_bg {
            key_style = key_style.bg(bg);
            val_style = val_style.bg(bg);
        }

        let marker = if selected {
            Span::styled("▌ ", Style::new().fg(theme::ACCENT).bg(theme::SEL_BG))
        } else {
            Span::raw("  ")
        };
        let toggle = if row.enabled {
            let mut s = Style::new().fg(theme::TEAL);
            if let Some(bg) = base_bg {
                s = s.bg(bg);
            }
            Span::styled("● ", s)
        } else {
            let mut s = Style::new().fg(theme::DIM);
            if let Some(bg) = base_bg {
                s = s.bg(bg);
            }
            Span::styled("○ ", s)
        };

        let key_text = if row.key.is_empty() && !selected {
            Span::styled(
                format!("{:<w$}", "·", w = key_w as usize),
                Style::new().fg(theme::BORDER),
            )
        } else {
            Span::styled(
                format!("{:<w$}", truncate(&row.key, key_w as usize), w = key_w as usize),
                key_style,
            )
        };
        let gap = if let Some(bg) = base_bg {
            Span::styled(" ", Style::new().bg(bg))
        } else {
            Span::raw(" ")
        };
        let val_text = Span::styled(
            format!("{:<w$}", truncate(&row.value, val_w as usize), w = val_w as usize),
            val_style,
        );

        let line = Line::from(vec![marker, toggle, key_text, gap, val_text]);
        f.render_widget(Paragraph::new(line), row_rect);

        // Active inline editor sits on top of its cell.
        if selected
            && let Some(edit) = cell {
                let rect = if edit.col == 0 {
                    Rect { x: area.x + marker_w + toggle_w, y, width: key_w, height: 1 }
                } else {
                    Rect { x: val_x, y, width: val_w, height: 1 }
                };
                f.render_widget(&edit.ta, rect);
                editor_rect = Some(rect);
            }
    }
    editor_rect
}
