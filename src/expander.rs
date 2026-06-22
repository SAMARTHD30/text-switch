use arboard::Clipboard;
use rdev::{simulate, EventType, Key};
use std::{thread, time::Duration};

/// Small delay between simulated key events so target apps keep up.
const KEY_DELAY: Duration = Duration::from_millis(2);
/// Delay around clipboard set/paste so the OS clipboard settles.
const CLIPBOARD_DELAY: Duration = Duration::from_millis(30);
const CLIPBOARD_ATTEMPTS: usize = 8;
const CLIPBOARD_RETRY_DELAY: Duration = Duration::from_millis(15);

fn send(event: &EventType) {
    let _ = simulate(event);
    thread::sleep(KEY_DELAY);
}

fn tap(key: Key) {
    send(&EventType::KeyPress(key));
    send(&EventType::KeyRelease(key));
}

fn clipboard_with_text(text: &str) -> Option<(Clipboard, Option<String>)> {
    for _ in 0..CLIPBOARD_ATTEMPTS {
        if let Ok(mut clipboard) = Clipboard::new() {
            let saved = clipboard.get_text().ok();
            if clipboard.set_text(text.to_string()).is_ok() {
                return Some((clipboard, saved));
            }
        }
        thread::sleep(CLIPBOARD_RETRY_DELAY);
    }
    None
}

/// Replace a just-typed trigger with `text`:
/// 1. delete `trigger_len` characters with Backspace,
/// 2. save the current clipboard,
/// 3. put `text` on the clipboard and send Ctrl+V,
/// 4. restore the previous clipboard.
///
/// Best-effort: clipboard failures never panic; the clipboard is always
/// restored if it was successfully read.
pub fn expand(trigger_len: usize, text: &str, paste_modifier: Key) {
    let Some((mut clipboard, saved)) = clipboard_with_text(text) else {
        return;
    };

    thread::sleep(CLIPBOARD_DELAY);

    for _ in 0..trigger_len {
        tap(Key::Backspace);
    }

    send(&EventType::KeyPress(paste_modifier));
    tap(Key::KeyV);
    send(&EventType::KeyRelease(paste_modifier));
    thread::sleep(CLIPBOARD_DELAY);

    if let Some(prev) = saved {
        let _ = clipboard.set_text(prev);
    }
}
