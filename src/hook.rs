use crate::config::{ActivationKey, Config};
use crate::expander;
use crate::matcher::Matcher;
use rdev::{grab, simulate, Event, EventType, Key};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Buffer cap: generous upper bound on trigger length. Exact-equality matching
/// means a large cap never causes false matches; this only bounds memory.
const BUFFER_CAP: usize = 100;
const RELEASE_SETTLE_DELAY: Duration = Duration::from_millis(25);
const REPLAY_KEY_DELAY: Duration = Duration::from_millis(2);

/// True while the expander is replaying simulated keystrokes, so the hook
/// ignores its own synthetic input.
static EXPANDING: AtomicBool = AtomicBool::new(false);

#[derive(Default, Clone, Copy)]
struct ModifierState {
    alt: bool,
    alt_gr: bool,
    ctrl_left: bool,
    ctrl_right: bool,
    shift_left: bool,
    shift_right: bool,
    meta_left: bool,
    meta_right: bool,
}

impl ModifierState {
    fn set(&mut self, key: Key, down: bool) {
        match key {
            Key::Alt => self.alt = down,
            Key::AltGr => self.alt_gr = down,
            Key::ControlLeft => self.ctrl_left = down,
            Key::ControlRight => self.ctrl_right = down,
            Key::ShiftLeft => self.shift_left = down,
            Key::ShiftRight => self.shift_right = down,
            Key::MetaLeft => self.meta_left = down,
            Key::MetaRight => self.meta_right = down,
            _ => {}
        }
    }

    fn any_excluding(self, key: Key) -> bool {
        (key != Key::Alt && self.alt)
            || (key != Key::AltGr && self.alt_gr)
            || (key != Key::ControlLeft && self.ctrl_left)
            || (key != Key::ControlRight && self.ctrl_right)
            || (key != Key::ShiftLeft && self.shift_left)
            || (key != Key::ShiftRight && self.shift_right)
            || (key != Key::MetaLeft && self.meta_left)
            || (key != Key::MetaRight && self.meta_right)
    }

    fn shortcut_modifier_excluding(self, key: Key) -> bool {
        (key != Key::Alt && self.alt)
            || (key != Key::AltGr && self.alt_gr)
            || (key != Key::ControlLeft && self.ctrl_left)
            || (key != Key::ControlRight && self.ctrl_right)
            || (key != Key::MetaLeft && self.meta_left)
            || (key != Key::MetaRight && self.meta_right)
    }

    fn shift_excluding(self, key: Key) -> bool {
        (key != Key::ShiftLeft && self.shift_left) || (key != Key::ShiftRight && self.shift_right)
    }
}

/// Shared, hot-reloadable configuration.
pub type SharedConfig = Arc<Mutex<Config>>;

/// The concrete `rdev::Key` for a preset activation key (`None` for `Custom`).
fn preset_key(a: &ActivationKey) -> Option<Key> {
    Some(match a {
        ActivationKey::RightCtrl => Key::ControlRight,
        ActivationKey::RightShift => Key::ShiftRight,
        ActivationKey::Insert => Key::Insert,
        ActivationKey::ScrollLock => Key::ScrollLock,
        ActivationKey::Pause => Key::Pause,
        ActivationKey::F9 => Key::F9,
        ActivationKey::Custom(_) => return None,
    })
}

/// Does `key` match the configured activation key? Presets compare against a
/// fixed `rdev::Key`; custom keys compare against a normalized captured name so
/// saved/manual names tolerate spaces, underscores, and case differences.
fn is_activation(a: &ActivationKey, key: Key) -> bool {
    match a {
        ActivationKey::Custom(name) => custom_key_matches(name, key),
        preset => preset_key(preset) == Some(key),
    }
}

fn custom_key_matches(name: &str, key: Key) -> bool {
    let stored = normalize_key_name(name);
    stored == normalize_key_name(&format!("{:?}", key))
        || modifier_alias_matches(&stored, key)
        || key_to_char(key).is_some_and(|c| stored == c.to_string())
}

fn modifier_alias_matches(stored: &str, key: Key) -> bool {
    matches!(
        (stored, key),
        ("control" | "ctrl", Key::ControlLeft | Key::ControlRight)
            | ("shift", Key::ShiftLeft | Key::ShiftRight)
            | ("alt", Key::Alt | Key::AltGr)
            | ("meta" | "win" | "windows", Key::MetaLeft | Key::MetaRight)
    )
}

fn normalize_key_name(name: &str) -> String {
    let trimmed = name.trim();
    let unprefixed = if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("key::") {
        &trimmed[5..]
    } else {
        trimmed
    };

    unprefixed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn is_modifier_key(key: Key) -> bool {
    matches!(
        key,
        Key::Alt
            | Key::AltGr
            | Key::ControlLeft
            | Key::ControlRight
            | Key::ShiftLeft
            | Key::ShiftRight
            | Key::MetaLeft
            | Key::MetaRight
    )
}

fn activation_press_passthrough(key: Key, modifiers: ModifierState) -> bool {
    is_modifier_key(key) || modifiers.any_excluding(key)
}

fn paste_modifier_for_activation(a: &ActivationKey) -> Key {
    if is_activation(a, Key::ControlLeft) {
        Key::ControlRight
    } else {
        Key::ControlLeft
    }
}

fn is_letter_key(key: Key) -> bool {
    matches!(
        key,
        Key::KeyA
            | Key::KeyB
            | Key::KeyC
            | Key::KeyD
            | Key::KeyE
            | Key::KeyF
            | Key::KeyG
            | Key::KeyH
            | Key::KeyI
            | Key::KeyJ
            | Key::KeyK
            | Key::KeyL
            | Key::KeyM
            | Key::KeyN
            | Key::KeyO
            | Key::KeyP
            | Key::KeyQ
            | Key::KeyR
            | Key::KeyS
            | Key::KeyT
            | Key::KeyU
            | Key::KeyV
            | Key::KeyW
            | Key::KeyX
            | Key::KeyY
            | Key::KeyZ
    )
}

fn trigger_char_for_key(key: Key, modifiers: ModifierState) -> Option<char> {
    if modifiers.shortcut_modifier_excluding(key) {
        return None;
    }
    if modifiers.shift_excluding(key) && !is_letter_key(key) {
        return None;
    }
    key_to_char(key)
}

fn replay_key(key: Key) {
    thread::spawn(move || {
        EXPANDING.store(true, Ordering::SeqCst);
        thread::sleep(REPLAY_KEY_DELAY);
        let _ = simulate(&EventType::KeyPress(key));
        thread::sleep(REPLAY_KEY_DELAY);
        let _ = simulate(&EventType::KeyRelease(key));
        thread::sleep(REPLAY_KEY_DELAY);
        EXPANDING.store(false, Ordering::SeqCst);
    });
}

/// Map an `rdev::Key` to a lowercase trigger character.
/// v1 supports lowercase ASCII letters and digits (see plan constraints).
fn key_to_char(key: Key) -> Option<char> {
    use Key::*;
    Some(match key {
        KeyA => 'a',
        KeyB => 'b',
        KeyC => 'c',
        KeyD => 'd',
        KeyE => 'e',
        KeyF => 'f',
        KeyG => 'g',
        KeyH => 'h',
        KeyI => 'i',
        KeyJ => 'j',
        KeyK => 'k',
        KeyL => 'l',
        KeyM => 'm',
        KeyN => 'n',
        KeyO => 'o',
        KeyP => 'p',
        KeyQ => 'q',
        KeyR => 'r',
        KeyS => 's',
        KeyT => 't',
        KeyU => 'u',
        KeyV => 'v',
        KeyW => 'w',
        KeyX => 'x',
        KeyY => 'y',
        KeyZ => 'z',
        Num0 => '0',
        Num1 => '1',
        Num2 => '2',
        Num3 => '3',
        Num4 => '4',
        Num5 => '5',
        Num6 => '6',
        Num7 => '7',
        Num8 => '8',
        Num9 => '9',
        _ => return None,
    })
}

/// Install the global keyboard hook and run its message loop.
/// This blocks the calling thread; run it on a dedicated thread.
/// Exits the process if the hook cannot be installed (the app is useless without it).
pub fn run(config: SharedConfig) {
    // `grab` requires an `Fn` callback, so we use a RefCell for interior
    // mutability of the matcher. The callback always runs on this one thread.
    let matcher = RefCell::new(Matcher::new(BUFFER_CAP));
    let modifiers = RefCell::new(ModifierState::default());
    let activation_down = RefCell::new(false);
    let activation_chorded = RefCell::new(false);
    let activation_passthrough_down = RefCell::new(true);

    let callback = move |event: Event| -> Option<Event> {
        // Pass synthetic keystrokes from the expander straight through.
        if EXPANDING.load(Ordering::SeqCst) {
            return Some(event);
        }

        match event.event_type {
            EventType::KeyPress(key) => modifiers.borrow_mut().set(key, true),
            EventType::KeyRelease(key) => modifiers.borrow_mut().set(key, false),
            _ => {}
        }

        // The activation key is configurable and hot-reloadable, so read it
        // fresh for each event (an uncontended lock is cheap, and key events
        // are infrequent compared to the lock cost).
        let activation = config.lock().unwrap().activation.clone();
        let mut activation_release_was_chorded = false;
        let mut activation_release_passthrough = true;

        match event.event_type {
            EventType::KeyPress(key) if is_activation(&activation, key) => {
                *activation_down.borrow_mut() = true;
                *activation_chorded.borrow_mut() = modifiers.borrow().any_excluding(key);
                *activation_passthrough_down.borrow_mut() =
                    activation_press_passthrough(key, *modifiers.borrow());
            }
            EventType::KeyRelease(key) if is_activation(&activation, key) => {
                activation_release_was_chorded =
                    *activation_chorded.borrow() || modifiers.borrow().any_excluding(key);
                activation_release_passthrough = *activation_passthrough_down.borrow();
                *activation_down.borrow_mut() = false;
                *activation_chorded.borrow_mut() = false;
                *activation_passthrough_down.borrow_mut() = true;
            }
            EventType::KeyPress(_) | EventType::KeyRelease(_) if *activation_down.borrow() => {
                *activation_chorded.borrow_mut() = true;
            }
            _ => {}
        }

        match event.event_type {
            // Activation: tap the configured key to expand the just-typed keyword.
            //
            // We act on *release*, not press, so a modifier activation key (e.g.
            // Right Ctrl / Right Shift) is no longer held when the expander
            // replays Backspace / Ctrl+V. A held modifier would turn those
            // Backspaces into Ctrl/Shift+Backspace and corrupt the deletion.
            // Non-modifier activation taps are deferred: if there is no match,
            // we replay the key so normal F-key/Insert/etc. behavior survives.
            EventType::KeyRelease(key) if is_activation(&activation, key) => {
                if activation_release_was_chorded {
                    return activation_release_passthrough.then_some(event);
                }

                let matched = {
                    let cfg = config.lock().unwrap();
                    let m = matcher.borrow();
                    m.check(&cfg.map).map(|trigger| {
                        let text = cfg.map.get(&trigger).cloned().unwrap_or_default();
                        (trigger.chars().count(), text)
                    })
                };
                matcher.borrow_mut().reset();

                if let Some((trigger_len, text)) = matched {
                    let paste_modifier = paste_modifier_for_activation(&activation);
                    EXPANDING.store(true, Ordering::SeqCst);
                    thread::spawn(move || {
                        thread::sleep(RELEASE_SETTLE_DELAY);
                        expander::expand(trigger_len, &text, paste_modifier);
                        EXPANDING.store(false, Ordering::SeqCst);
                    });
                } else if !activation_release_passthrough {
                    replay_key(key);
                }
                activation_release_passthrough.then_some(event)
            }

            // Activation key press: leave the buffer intact (expansion happens on
            // release). Modifier activations pass through so Ctrl+A/Ctrl+Z/etc.
            // remain real shortcuts. Non-modifier activations are deferred and
            // replayed on release only when no expansion happens.
            EventType::KeyPress(key) if is_activation(&activation, key) => {
                (*activation_passthrough_down.borrow()).then_some(event)
            }

            EventType::KeyPress(Key::Backspace) => {
                if modifiers.borrow().any_excluding(Key::Backspace) {
                    matcher.borrow_mut().reset();
                } else {
                    matcher.borrow_mut().backspace();
                }
                Some(event)
            }

            EventType::KeyPress(key) => {
                match trigger_char_for_key(key, *modifiers.borrow()) {
                    Some(c) => matcher.borrow_mut().push_char(c),
                    None => matcher.borrow_mut().reset(),
                }
                Some(event)
            }

            _ => Some(event),
        }
    };

    if let Err(e) = grab(callback) {
        eprintln!("FATAL: could not install keyboard hook: {:?}", e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_activation_matches_captured_name() {
        assert!(is_activation(
            &ActivationKey::Custom("F10".into()),
            Key::F10
        ));
        assert!(is_activation(
            &ActivationKey::Custom("ControlRight".into()),
            Key::ControlRight
        ));
    }

    #[test]
    fn custom_activation_tolerates_manual_name_variants() {
        assert!(is_activation(
            &ActivationKey::Custom("Control Right".into()),
            Key::ControlRight
        ));
        assert!(is_activation(
            &ActivationKey::Custom("Control".into()),
            Key::ControlLeft
        ));
        assert!(is_activation(
            &ActivationKey::Custom("Ctrl".into()),
            Key::ControlRight
        ));
        assert!(is_activation(
            &ActivationKey::Custom("key::f10".into()),
            Key::F10
        ));
        assert!(is_activation(&ActivationKey::Custom("a".into()), Key::KeyA));
        assert!(is_activation(&ActivationKey::Custom("1".into()), Key::Num1));
    }

    #[test]
    fn non_modifier_activation_is_deferred_when_pressed_alone() {
        assert!(!activation_press_passthrough(
            Key::F10,
            ModifierState::default()
        ));
        assert!(!activation_press_passthrough(
            Key::Insert,
            ModifierState::default()
        ));
    }

    #[test]
    fn modifier_activation_passes_through_for_shortcuts() {
        assert!(activation_press_passthrough(
            Key::ControlLeft,
            ModifierState::default()
        ));
        assert!(activation_press_passthrough(
            Key::ShiftLeft,
            ModifierState::default()
        ));
    }

    #[test]
    fn non_modifier_activation_passes_through_when_chorded() {
        let mut modifiers = ModifierState::default();
        modifiers.set(Key::ControlLeft, true);
        assert!(activation_press_passthrough(Key::F9, modifiers));
    }

    #[test]
    fn shifted_letters_still_record_trigger_chars() {
        let mut modifiers = ModifierState::default();
        modifiers.set(Key::ShiftLeft, true);
        assert_eq!(trigger_char_for_key(Key::KeyC, modifiers), Some('c'));
    }

    #[test]
    fn shortcut_chords_do_not_record_trigger_chars() {
        let mut modifiers = ModifierState::default();
        modifiers.set(Key::ControlLeft, true);
        assert_eq!(trigger_char_for_key(Key::KeyC, modifiers), None);
    }

    #[test]
    fn shifted_digits_do_not_record_as_digits() {
        let mut modifiers = ModifierState::default();
        modifiers.set(Key::ShiftLeft, true);
        assert_eq!(trigger_char_for_key(Key::Num1, modifiers), None);
    }

    #[test]
    fn left_ctrl_activation_pastes_with_right_ctrl() {
        assert_eq!(
            paste_modifier_for_activation(&ActivationKey::Custom("ControlLeft".into())),
            Key::ControlRight
        );
        assert_eq!(
            paste_modifier_for_activation(&ActivationKey::RightCtrl),
            Key::ControlLeft
        );
    }
}
