use std::io;
use std::time::Duration;

use anyhow::Result;
use postcat::{app, keys, ui};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyEventKind,
};
use crossterm::execute;

fn main() -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;

    // ratatui::init chains this hook after its own terminal restore, so a panic
    // still turns mouse capture / bracketed paste off.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
        prev(info);
    }));

    let mut terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableBracketedPaste, EnableMouseCapture);
    set_pointer(POINTER_ARROW);

    let mut app = app::App::new(rt.handle().clone());
    let result = run(&mut terminal, &mut app);

    let _ = io::Write::write_all(&mut io::stdout(), POINTER_RESET);
    let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    result
}

// OSC 22 (xterm pointer shape): the app is UI, so the pointer is an arrow
// everywhere — the I-beam appears only over the field being edited.
// Terminals without support ignore these sequences.
const POINTER_ARROW: &[u8] = b"\x1b]22;default\x07";
const POINTER_TEXT: &[u8] = b"\x1b]22;text\x07";
const POINTER_RESET: &[u8] = b"\x1b]22;\x07";

fn set_pointer(shape: &[u8]) {
    use std::io::Write;
    let mut out = io::stdout();
    let _ = out.write_all(shape);
    let _ = out.flush();
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut app::App) -> Result<()> {
    let mut pointer_text = false;
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => keys::on_key(app, k),
                Event::Paste(s) => keys::on_paste(app, s),
                Event::Mouse(m) => keys::on_mouse(app, m),
                _ => {}
            }
        }
        // Drain worker events; chunk arrivals coalesce into one rebuild per frame.
        app.pump();
        app.tick();

        if app.hover_text != pointer_text {
            pointer_text = app.hover_text;
            set_pointer(if pointer_text { POINTER_TEXT } else { POINTER_ARROW });
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
