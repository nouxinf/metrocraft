//! Keyboard shortcuts, and what Preferences → Keys changes.
//!
//! Each command owns one key and a set of modifiers. The defaults are the ones
//! the menus advertise; any of them can be given a different key, and the set
//! is remembered between sessions.

use eframe::egui::{Context, Event, InputState, Key, KeyboardShortcut, Modifiers};
use serde::{Deserialize, Serialize};

/// A command with a key of its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Undo,
    Redo,
    Save,
    SaveAs,
    Open,
    New,
    Fit,
    Preferences,
}

/// Every command, in the order the Keys tab lists them.
pub const ACTIONS: [Action; 8] = [
    Action::Undo,
    Action::Redo,
    Action::Save,
    Action::SaveAs,
    Action::Open,
    Action::New,
    Action::Fit,
    Action::Preferences,
];

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Save => "Save",
            Self::SaveAs => "Save as",
            Self::Open => "Open",
            Self::New => "New",
            Self::Fit => "Fit to map",
            Self::Preferences => "Preferences",
        }
    }

    fn slot(self) -> usize {
        match self {
            Self::Undo => 0,
            Self::Redo => 1,
            Self::Save => 2,
            Self::SaveAs => 3,
            Self::Open => 4,
            Self::New => 5,
            Self::Fit => 6,
            Self::Preferences => 7,
        }
    }
}

/// Every command's key, in [`ACTIONS`] order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keys {
    binds: [Option<KeyboardShortcut>; ACTIONS.len()],
}

impl Default for Keys {
    fn default() -> Self {
        let mut keys = Self {
            binds: [None; ACTIONS.len()],
        };
        for (action, modifiers, key) in [
            (Action::Undo, Modifiers::COMMAND, Key::Z),
            (Action::Redo, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
            (Action::Save, Modifiers::COMMAND, Key::S),
            (
                Action::SaveAs,
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::S,
            ),
            (Action::Open, Modifiers::COMMAND, Key::O),
            (Action::New, Modifiers::COMMAND, Key::N),
            (Action::Fit, Modifiers::NONE, Key::F),
            (Action::Preferences, Modifiers::COMMAND, Key::Comma),
        ] {
            keys.binds[action.slot()] = Some(KeyboardShortcut::new(modifiers, key));
        }
        keys
    }
}

impl Keys {
    /// The key this command answers to, if it still has one.
    pub fn bind(&self, action: Action) -> Option<KeyboardShortcut> {
        self.binds[action.slot()]
    }

    /// Give `action` this key, taking it from whichever command held it: two
    /// commands on one key would fire together, so the older claim goes.
    pub fn rebind(&mut self, action: Action, shortcut: KeyboardShortcut) {
        for (slot, bind) in self.binds.iter_mut().enumerate() {
            if slot != action.slot() && *bind == Some(shortcut) {
                *bind = None;
            }
        }
        self.binds[action.slot()] = Some(shortcut);
    }

    /// Put every command back on the key it ships with.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether `action` is a command on a held modifier rather than a bare
    /// key, so a field with the keys still has to let it through.
    pub fn modified(&self, action: Action) -> bool {
        self.bind(action)
            .is_some_and(|shortcut| shortcut.modifiers.any())
    }

    /// Whether `action`'s key, and only its modifiers, went down this frame.
    pub fn pressed(&self, action: Action, input: &InputState) -> bool {
        self.bind(action).is_some_and(|shortcut| {
            input.key_pressed(shortcut.logical_key)
                && input.modifiers.command == shortcut.modifiers.command
                && input.modifiers.shift == shortcut.modifiers.shift
                && input.modifiers.alt == shortcut.modifiers.alt
        })
    }

    /// The key as the menus write it, `Ctrl+,` rather than `Ctrl+Comma`; empty
    /// while the command is unbound.
    pub fn label(&self, ctx: &Context, action: Action) -> String {
        let Some(shortcut) = self.bind(action) else {
            return String::new();
        };
        let key = shortcut.logical_key.symbol_or_name();
        let modifiers = ctx.format_modifiers(shortcut.modifiers);
        if modifiers.is_empty() {
            key.to_owned()
        } else {
            format!("{modifiers}+{key}")
        }
    }
}

/// The key that went down this frame, ready to become a binding. Modifier keys
/// on their own are not keys here, so the first real one wins.
pub fn captured(input: &InputState) -> Option<KeyboardShortcut> {
    input.events.iter().rev().find_map(|event| match event {
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => Some(KeyboardShortcut::new(held(*modifiers), *key)),
        _ => None,
    })
}

/// Only the modifiers a binding stores, so Ctrl on one platform and ⌘ on
/// another are both "command".
fn held(modifiers: Modifiers) -> Modifiers {
    let mut held = Modifiers::NONE;
    if modifiers.command {
        held |= Modifiers::COMMAND;
    }
    if modifiers.shift {
        held |= Modifiers::SHIFT;
    }
    if modifiers.alt {
        held |= Modifiers::ALT;
    }
    held
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_keys_the_menus_show() {
        let keys = Keys::default();
        for action in ACTIONS {
            assert!(keys.bind(action).is_some(), "{action:?} has no key");
        }
        assert_eq!(
            keys.bind(Action::Redo),
            Some(KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::Z
            ))
        );
        assert_eq!(
            keys.bind(Action::Preferences),
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma))
        );
        assert_eq!(
            keys.bind(Action::Fit),
            Some(KeyboardShortcut::new(Modifiers::NONE, Key::F))
        );
    }

    #[test]
    fn only_commands_on_a_modifier_survive_a_field_with_the_keys() {
        let keys = Keys::default();
        assert!(keys.modified(Action::Save));
        assert!(keys.modified(Action::Preferences));
        assert!(!keys.modified(Action::Fit));
    }

    #[test]
    fn rebinding_takes_the_key_from_the_command_that_had_it() {
        let mut keys = Keys::default();
        let taken = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        keys.rebind(Action::Preferences, taken);
        assert_eq!(keys.bind(Action::Preferences), Some(taken));
        assert_eq!(keys.bind(Action::Save), None);
        assert_eq!(keys.binds[Action::Save.slot()], None);

        // Its own key again is not a conflict with itself.
        keys.rebind(Action::Preferences, taken);
        assert_eq!(keys.bind(Action::Preferences), Some(taken));
    }

    #[test]
    fn the_command_that_lost_its_key_answers_to_nothing() {
        let mut keys = Keys::default();
        let redo = keys.bind(Action::Redo).unwrap();
        keys.rebind(Action::Undo, redo);
        assert_eq!(keys.bind(Action::Undo), Some(redo));
        assert!(keys.bind(Action::Redo).is_none());
    }

    #[test]
    fn reset_puts_every_command_back_on_its_default_key() {
        let defaults = Keys::default();
        let mut keys = defaults;
        keys.rebind(Action::Undo, KeyboardShortcut::new(Modifiers::ALT, Key::G));
        keys.rebind(Action::Save, KeyboardShortcut::new(Modifiers::NONE, Key::K));
        assert_ne!(keys, defaults);
        keys.reset();
        assert_eq!(keys, defaults);
    }

    #[test]
    fn bindings_survive_a_round_trip_through_json() {
        let mut keys = Keys::default();
        keys.rebind(
            Action::Fit,
            KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, Key::Num1),
        );
        let json = serde_json::to_string(&keys).unwrap();
        assert_eq!(serde_json::from_str::<Keys>(&json).unwrap(), keys);
    }
}
