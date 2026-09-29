//! The Shortcuts editor: capturing a chord, resetting one, resetting all.
//!
//! Capture is armed here and answered by the routers in `shortcuts`,
//! which is why a mouse press is a settings message: it can be a
//! binding being recorded rather than one being fired.

use super::*;

impl Oryxis {
    pub(super) fn handle_settings_hotkeys(
        &mut self,
        message: SettingsMessage,
    ) -> Result<Task<Message>, SettingsMessage> {
        match message {
            SettingsMessage::StartEditingHotkey(action, slot) => {
                self.editing_hotkey = Some((action, slot));
                // One live capture at a time: arming an action capture
                // cancels a pending host one, so a stale flag can't eat
                // the next chord for the wrong target.
                self.editing_host_hotkey = None;
            }
            SettingsMessage::StartEditingHostHotkey(id) => {
                self.editing_host_hotkey = Some(id);
                self.editing_hotkey = None;
            }
            SettingsMessage::ClearHostHotkey(id) => {
                self.editing_host_hotkey = None;
                self.set_host_hotkey(id, None);
            }
            SettingsMessage::MouseButtonPressed(button) => {
                return Ok(self.handle_mouse_button_press(button));
            }
            SettingsMessage::WheelCaptured(direction) => {
                return Ok(self.handle_hotkey_wheel_capture(direction));
            }
            SettingsMessage::ResetHotkey(action) => {
                let mut defaults = crate::hotkeys::default_bindings();
                match defaults.remove(&action) {
                    Some(d) => self.hotkey_bindings.insert(action, d),
                    None => self.hotkey_bindings.remove(&action),
                };
                // Empty value persists the absence of an override, so
                // future boots rehydrate to the default. Same
                // semantics as deleting the row, and distinct from the
                // UNBOUND token a deliberate unbind writes.
                self.persist_setting(&format!("hotkey_{}", action.id()), "");
            }
            SettingsMessage::ResetAllHotkeys => {
                self.hotkey_bindings = crate::hotkeys::default_bindings();
                for action in crate::hotkeys::HotkeyAction::all() {
                    self.persist_setting(&format!("hotkey_{}", action.id()), "");
                }
            }
            SettingsMessage::ToggleSecretVisibility(field) => {
                if !self.revealed_secrets.remove(&field) {
                    self.revealed_secrets.insert(field);
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }

    /// Set (or clear, with `None`) a host's connect shortcut by
    /// connection id, updating the in-memory list and persisting to the
    /// vault. The single write path shared by both edit surfaces
    /// (Settings → Shortcuts and the host editor tray), so they can
    /// never disagree. `chord` is the serialized `HotkeyBinding`.
    pub(crate) fn set_host_hotkey(&mut self, id: uuid::Uuid, chord: Option<String>) {
        let Some(idx) = self.connections.iter().position(|c| c.id == id) else {
            return;
        };
        self.connections[idx].hotkey = chord.clone();
        // Keep the open editor form in step so the tray reflects the
        // change immediately whichever surface made it.
        if self.editor_form.editing_id == Some(id) {
            self.editor_form.hotkey = chord;
        }
        if let Some(vault) = &self.vault {
            // `None` password preserves the encrypted column untouched.
            let _ = vault.save_connection(&self.connections[idx], None);
        }
    }
}
