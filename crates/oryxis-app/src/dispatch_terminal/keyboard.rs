//! `KeyboardEvent` handling, split out of `dispatch_terminal`: the
//! modifier / Alt-side tracking, the lock-screen and modal gates,
//! find-bar / player / pick-list / keynav / sidebar-ring routing,
//! the hotkey table hook, and the final `key_encode` dispatch into
//! the focused pane's PTY. Called from `handle_terminal`.

#![allow(clippy::result_large_err)]

use iced::keyboard;
use iced::Task;

use crate::app::{TabsMessage, TerminalMessage, NavigationMessage, VaultMessage, Message, Oryxis};

impl Oryxis {
    /// Handle the `KeyboardEvent` chord resolver + PTY key routing.
    /// Returns `Err(message)` for every other variant so
    /// `handle_terminal`'s chain falls through.
    pub(super) fn handle_terminal_keyboard(
        &mut self,
        message: TerminalMessage,
    ) -> Result<Task<Message>, TerminalMessage> {
        match message {
            TerminalMessage::KeyboardEvent(event) => {
                // Track modifier state for downstream consumers (SFTP
                // ctrl/shift-click selection). Always update first so
                // every later branch in this handler sees fresh state.
                if let keyboard::Event::ModifiersChanged(m) = &event {
                    self.modifiers = *m;
                }
                // Track which physical Alt (Option) side is held for the
                // macOS OptionAsMeta quirk: `Modifiers` can't tell the
                // sides apart, so watch the Alt key's own press/release.
                // Must run ahead of every gate below (early returns would
                // starve it); ModifiersChanged without Alt clears both so
                // a release swallowed by a modal can't wedge a side down.
                match &event {
                    keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Alt),
                        location,
                        ..
                    }
                    | keyboard::Event::KeyReleased {
                        key: keyboard::Key::Named(keyboard::key::Named::Alt),
                        location,
                        ..
                    } => {
                        let down = matches!(&event, keyboard::Event::KeyPressed { .. });
                        match location {
                            keyboard::Location::Left => self.alt_sides.left = down,
                            keyboard::Location::Right => self.alt_sides.right = down,
                            _ => {}
                        }
                    }
                    keyboard::Event::ModifiersChanged(m) if !m.alt() => {
                        self.alt_sides = crate::key_encode::OptionSides::default();
                    }
                    _ => {}
                }
                // Key-gate tracing (debug log): Ctrl+Shift chords are
                // always app hotkeys, so snapshot the gate-relevant
                // state on arrival. Field report 2026-07-03: the
                // FocusSidebarList chord worked on a local tab but not
                // on an SSH tab of the same build, and no static
                // difference between those paths exists; this line is
                // how the next report pinpoints the consuming gate.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && modifiers.shift()
                {
                    tracing::debug!(
                        ?key,
                        view = ?self.cur_view(),
                        tab = ?self.cur_active_tab(),
                        pick_open = self.keynav.pick_open,
                        modal = self.any_modal_blocks_input(),
                        panel = self.panels.host_panel,
                        capture = self.editing_hotkey.is_some(),
                        "key-gate: ctrl+shift chord"
                    );
                }
                // PrintScreen -> open the Windows snip overlay (region
                // capture), matching the OS default. winit delivers the
                // key to the focused window without forwarding it to
                // DefWindowProc, so Windows' own PrintScreen handler never
                // fires while Oryxis is focused; we remap it explicitly.
                // VK_SNAPSHOT classically emits only WM_KEYUP, so accept a
                // press or a release, debounced so a paired press+release
                // doesn't launch the overlay twice. Handled before the
                // modal / chat / PTY gates so a screenshot always works.
                #[cfg(target_os = "windows")]
                {
                    let is_printscreen = matches!(
                        &event,
                        keyboard::Event::KeyPressed {
                            key: keyboard::Key::Named(keyboard::key::Named::PrintScreen),
                            ..
                        } | keyboard::Event::KeyReleased {
                            key: keyboard::Key::Named(keyboard::key::Named::PrintScreen),
                            ..
                        }
                    );
                    if is_printscreen {
                        let now = std::time::Instant::now();
                        let recent = self
                            .last_printscreen
                            .map(|t| {
                                now.duration_since(t) < std::time::Duration::from_millis(400)
                            })
                            .unwrap_or(false);
                        if !recent {
                            self.last_printscreen = Some(now);
                            crate::util::open_screenshot_tool();
                        }
                        return Ok(Task::none());
                    }
                }
                // Lock screen, biometric-first layout: there is no
                // password input on screen, so Enter raises the OS
                // presence prompt (the primary action). The fallback
                // layout keeps Enter on the focused input's on_submit.
                if self.vault_ui.state == crate::state::VaultState::Locked
                    && self.biometric_unlock_offered()
                    && !self.vault_ui.password_fallback
                    && matches!(
                        &event,
                        keyboard::Event::KeyPressed {
                            key: keyboard::Key::Named(keyboard::key::Named::Enter),
                            ..
                        }
                    )
                {
                    return Ok(Task::done(Message::Vault(VaultMessage::BiometricUnlockRequested)));
                }
                // Lock / setup / onboarding screens own the keyboard
                // exclusively: their inputs receive keys through the
                // widget tree, and every consumer below (keynav routers,
                // the hotkey table, PTY routing) belongs to the unlocked
                // app. `LockVault` leaves `active_view = Dashboard`, so
                // without this gate those consumers ran against stale
                // vault-area state from behind the lock screen; a key
                // they claimed could steal iced focus from the unlock
                // input mid-typing (field report 2026-07-12). The one
                // exception: an app-level modal rendered over the lock
                // screen (update / plugin install / KBI / host key) keeps
                // its keyboard layer, so its Enter / Esc / arrows work.
                if self.vault_ui.state != crate::state::VaultState::Unlocked {
                    if self.any_modal_blocks_input()
                        && let Some(task) = self.handle_modal_nav_key(&event)
                    {
                        return Ok(task);
                    }
                    return Ok(Task::none());
                }
                // The gate above cannot catch the key press that UNLOCKED
                // the vault: the password input's on_submit processes first
                // in the same update batch, so by the time that Enter
                // arrives here the state is already Unlocked. Every
                // consumer below would treat it as an unlocked-app
                // keystroke; with a terminal tab restored by the soft lock
                // the newline lands on the shell prompt and would run
                // whatever was left typed there. Swallow key events for a
                // breath after an unlock (a human's first intentional
                // post-unlock key arrives much later; an unlock via the
                // mouse leaves no in-flight key at all).
                if self
                    .last_unlock
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(150))
                {
                    return Ok(Task::none());
                }
                // Scrollback find-bar (C1): while it's open on the active
                // pane it owns the keyboard, like a modal. Enter steps to the
                // next match, Shift+Enter to the previous, Esc closes; every
                // other key edits the find input through its focused
                // `text_input` and must NOT leak to the PTY running
                // underneath (the global subscription delivers keys here
                // regardless of widget focus).
                if self
                    .active_tab
                    .and_then(|i| self.tabs.get(i))
                    .map(|t| t.active().search_open)
                    .unwrap_or(false)
                {
                    if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event {
                        match key {
                            keyboard::Key::Named(keyboard::key::Named::Enter) => {
                                return Ok(Task::done(Message::Terminal(TerminalMessage::TerminalSearchStep(
                                    !modifiers.shift(),
                                ))));
                            }
                            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                                return Ok(Task::done(Message::Terminal(TerminalMessage::TerminalSearchClose)));
                            }
                            _ => {}
                        }
                    }
                    return Ok(Task::none());
                }
                // Session player (issue #71): while the player surface
                // is up on the History view it owns its transport keys
                // (Space, Left/Right, Home, Esc); everything else falls
                // through so hotkeys keep working. See
                // dispatch_player.rs.
                if let Some(task) = self.handle_player_key(&event) {
                    return Ok(task);
                }
                // An open pick_list dropdown owns the keyboard: the
                // widget itself handles Enter/Space (confirm), Up/Down
                // (move the hovered option) and Esc (close). The global
                // subscription still delivers those keys here, so
                // swallow them or Esc would ALSO close the panel/modal
                // behind the dropdown and Enter would double-dispatch.
                // Tab is left alone (the widget closes the menu without
                // capturing so the focus chain still runs).
                if self.keynav.pick_open
                    && let keyboard::Event::KeyPressed { key, .. } = &event
                    && (matches!(
                        key,
                        keyboard::Key::Named(
                            keyboard::key::Named::Enter
                                | keyboard::key::Named::Space
                                | keyboard::key::Named::Escape
                                | keyboard::key::Named::ArrowUp
                                | keyboard::key::Named::ArrowDown
                        )
                    ) || matches!(key, keyboard::Key::Character(c) if c.as_str() == " "))
                {
                    return Ok(Task::none());
                }
                // Host editor panel open -> Tab / Shift+Tab move focus
                // between form fields like a browser, instead of falling
                // through to the PTY (which would emit a literal \t) or a
                // hotkey binding. focus_next / focus_previous walk iced's
                // real focus chain, so click-then-Tab works too.
                if self.side_panel_open()
                    && !self.any_modal_blocks_input()
                    && let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab))
                    && !modifiers.control()
                {
                    // Tab walks the panel's recorded rows: inputs get
                    // real focus, selects/toggles/buttons get the ring
                    // (the raw focus chain skipped everything that
                    // isn't a text input). Panels that record nothing
                    // fall back to the old focus-chain walk. See
                    // dispatch_keynav_panel.rs.
                    //
                    // First resolve which widget iced actually has focused
                    // (a mouse click moves focus without touching our ring
                    // index), so the walk continues from the clicked field
                    // instead of a stale index. The walk itself happens in
                    // the `PanelNavTabResolved` handler.
                    let forward = !modifiers.shift();
                    if !self.keynav.panel_items.borrow().is_empty() {
                        return Ok(iced::widget::operation::find_focused().map(
                            move |focused| Message::Navigation(NavigationMessage::PanelNavTabResolved { forward, focused }),
                        ));
                    }
                    return Ok(if forward {
                        iced::widget::operation::focus_next()
                    } else {
                        iced::widget::operation::focus_previous()
                    });
                }
                // Settings -> Security password forms open -> Tab /
                // Shift+Tab walk the password fields like a browser form,
                // instead of leaking a literal \t. Same focus-chain
                // mechanism as the host editor above. Covers both the
                // set-password and change-password forms.
                if self.cur_view() == crate::state::View::Settings
                    && self.settings_section == crate::state::SettingsSection::Security
                    && (self.vault_ui.show_password_form || self.vault_ui.change_password_open)
                    && let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab))
                {
                    return Ok(if modifiers.shift() {
                        iced::widget::operation::focus_previous()
                    } else {
                        iced::widget::operation::focus_next()
                    });
                }
                // Ctrl+Tab / Ctrl+Shift+Tab: switch tabs by last use, like the
                // OS Alt+Tab. A single repeated press toggles the two most
                // recent tabs; holding Ctrl and pressing Tab several times
                // walks further back through the recency stack, committing the
                // choice when Ctrl is released. Covers open tabs only (Home
                // stays on Ctrl+1 / Alt+arrow). Handled here rather than via the
                // configurable hotkey table so it works from any surface.
                // Consumed unconditionally so the combo never leaks a literal
                // \t into the PTY. See `tab_cycle.rs` for the MRU mechanics;
                // the run is committed by `reconcile_tab_mru` (Ctrl-release) and
                // `WindowFocusChanged` (focus lost mid-hold).
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab))
                    && modifiers.control()
                    && !self.panels.host_panel
                    && !self.any_modal_blocks_input()
                {
                    return Ok(self.cycle_mru_step(!modifiers.shift()));
                }
                // Password-suggest popup (issue #117): must run BEFORE
                // the modal layer, whose catch-all would claim
                // Up/Down/Enter for a surface that is not a menu, and
                // before the binding table, which would take Down/Esc
                // from it. Non-modal by contract: it declines every key
                // it did not open for, so the PTY keeps the keyboard.
                // See dispatch_password_suggest.rs.
                if let Some(task) = self.handle_password_suggest_key(&event) {
                    return Ok(task);
                }
                // Modal / overlay-menu keyboard layer: while a
                // navigable dialog, dropdown or the burger menu is
                // open it owns movement + activation keys (Esc stays
                // with close_topmost_modal further down). See
                // dispatch_keynav_modal.rs.
                if let Some(task) = self.handle_modal_nav_key(&event) {
                    return Ok(task);
                }
                // Vault-area focus-zone navigation (Tab between zones,
                // arrows within, Enter activates, Esc back to idle).
                // Plain keys only, so Ctrl/Alt combos still reach the
                // hotkey table below; anything the router declines
                // falls through to it naturally. See dispatch_keynav.rs.
                if let Some(task) = self.handle_keynav_key(&event) {
                    return Ok(task);
                }
                // Terminal-sidebar navigation (all four tabs). Opt-in:
                // engaged by the FocusSidebarList hotkey, by Up/Down
                // while the cursor is over a list tab, or by Tab while
                // the cursor is over the sidebar; declines everything
                // else so the PTY keeps the keyboard (a plain Tab over
                // the terminal is still a literal \t). See
                // dispatch_keynav_sidebar.rs.
                if let Some(task) = self.handle_sidebar_nav_key(&event) {
                    return Ok(task);
                }
                // Hotkey dispatch + capture mode live in `shortcuts.rs`
                // (`handle_hotkey_keypress`). Returns a Task when the
                // event was consumed by a binding (or by the Settings
                // editor's capture mode), `None` to fall through to
                // the legacy PTY-routing block below.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && let Some(task) = self.handle_hotkey_keypress(key, modifiers)
                {
                    return Ok(task);
                }
                // When the sidebar is open and the cursor is over it, the
                // user is interacting with its widgets (chat textarea,
                // search fields, the Files browser), drop the event so it
                // doesn't double-dispatch into the terminal session running
                // underneath. `cursor_over_sidebar` honors the dock side
                // (issue #85); the old inline right-edge math leaked every
                // key into the PTY when the sidebar was docked left.
                if self.cursor_over_sidebar() {
                    return Ok(Task::none());
                }
                // A global picker / modal (new-tab picker, tab jump,
                // icon / theme / jump-host pickers, folder rename or
                // delete) owns the keyboard while open. Its own search
                // field consumes the keystroke via iced focus; without
                // this gate the same press also falls through to the
                // PTY below, so typing in the picker echoes into the
                // terminal. Esc still closes the modal because that's
                // handled earlier in `handle_hotkey_keypress`.
                if self.any_modal_blocks_input() {
                    return Ok(Task::none());
                }

                // The connect screen covers only the connecting tab (the
                // render side scopes it via `cp.tab_idx == active_tab` in
                // `view_content`); match that here. The old app-global
                // `connecting.is_none()` gate ate every keystroke in every
                // other tab as long as one tab sat on an in-flight or
                // failed-and-not-dismissed connect, so typing died until
                // the connecting tab was closed.
                let connecting_here = self
                    .connecting
                    .as_ref()
                    .is_some_and(|cp| Some(cp.tab_idx) == self.cur_active_tab());
                if let Some(tab_idx) = self.cur_active_tab()
                    && !connecting_here
                    && let keyboard::Event::KeyPressed {
                        key,
                        modified_key,
                        modifiers,
                        text: text_opt,
                        location,
                        ..
                    } = event
                    {
                        // Application-cursor-keys mode (DECCKM) of the active
                        // pane decides whether arrows / Home / End go out in
                        // SS3 (`ESC O A`) or CSI (`ESC [ A`) form. Read once
                        // here; the same flag is tracked for local PTY and SSH
                        // panes alike.
                        let app_cursor = self
                            .tabs
                            .get(tab_idx)
                            .and_then(|t| {
                                t.active().terminal.lock().ok().map(|s| s.application_cursor_keys())
                            })
                            .unwrap_or(false);
                        // C5: the focused pane's resolved legacy keyboard
                        // modes drive the named-key encoding below.
                        let pane_quirks = self
                            .tabs
                            .get(tab_idx)
                            .map(|t| t.active().quirks)
                            .unwrap_or(oryxis_core::models::terminal_quirks::DEFAULT_QUIRKS);
                        // Ctrl+D on a logged-out session (SSH / exec tab
                        // relabelled "(disconnected)") has no shell left to
                        // receive EOF, so it would be swallowed. Treat it as
                        // "close this dead tab" instead, matching the muscle
                        // memory of dismissing an exited shell. Only
                        // single-pane tabs carry the suffix, so siblings are
                        // never nuked. Ctrl WITHOUT Alt only: Ctrl+Alt may be
                        // AltGr composition (Windows), which is the encoder's
                        // call, never a close.
                        if modifiers.control()
                            && !modifiers.alt()
                            && !modifiers.shift()
                            && let keyboard::Key::Character(ref c) = key
                            && c.as_str().eq_ignore_ascii_case("d")
                            && self
                                .tabs
                                .get(tab_idx)
                                .is_some_and(|t| t.label.ends_with(" (disconnected)"))
                        {
                            return Ok(self.update(Message::Tabs(TabsMessage::CloseTab(tab_idx))));
                        }
                        // Everything else is the pure encoder's decision
                        // table (`key_encode::pty_bytes`): the macOS Cmd and
                        // Ctrl+Shift swallows, control bytes (Ctrl+C's
                        // interrupt included), AltGr / Option composition
                        // (issue #80), meta-sends-escape, the C5 named-key
                        // quirks and the numpad NumLock text.
                        let press = crate::key_encode::KeyPress {
                            key: &key,
                            modified_key: &modified_key,
                            modifiers,
                            text: text_opt.as_deref(),
                            location,
                        };
                        let bytes = crate::key_encode::pty_bytes(
                            &press,
                            crate::key_encode::Platform::current(),
                            app_cursor,
                            &pane_quirks,
                            self.alt_sides,
                        );
                        // Key-event tracing (debug log, opt-in): what winit
                        // delivered vs what the encoder wrote is exactly the
                        // evidence a layout report (bepo AltGr+Space, German
                        // AltGr, Option composition) needs. Restricted to
                        // chord/named presses so plain typing, passwords
                        // included, never lands in the log file.
                        if crate::logging::is_enabled()
                            && (modifiers.control()
                                || modifiers.alt()
                                || matches!(key, keyboard::Key::Named(_)))
                        {
                            tracing::debug!(
                                ?key,
                                ?modified_key,
                                ?modifiers,
                                ?location,
                                text = ?text_opt,
                                out = ?bytes,
                                alt_sides = ?self.alt_sides,
                                "key-encode"
                            );
                        }
                        if let Some(bytes) = bytes
                            && !bytes.is_empty()
                        {
                            self.write_input_to_tab(tab_idx, &bytes);
                        }
                    }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
