//! System clipboard writes.
//!
//! OSC 52 hands the text to the terminal emulator, which puts it on the real
//! clipboard — no pasteboard/X11/Wayland dependency, and it keeps working over
//! ssh. Terminals that don't implement it ignore the sequence.

use std::io::{self, Write};

use base64::Engine as _;

pub fn set(text: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(osc52(text).as_bytes());
    let _ = out.flush();
}

fn osc52(text: &str) -> String {
    let payload = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    format!("\x1b]52;c;{payload}\x07")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_targets_the_clipboard_with_base64_text() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
        // Newlines and non-ASCII survive the encoding rather than truncating it.
        assert_eq!(osc52("a\né"), "\x1b]52;c;YQrDqQ==\x07");
    }
}
