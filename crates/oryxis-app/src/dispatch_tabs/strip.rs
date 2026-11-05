//! The tab strip: select, close, pin, duplicate, reorder, and the
//! per-tab menus.
//!
//! Closing is the arm with teeth: a grouped tab tears down every pane in
//! it, so it confirms first, a live single-session tab confirms when the
//! opt-in guard is on, and "close others" / "close all" confirm when any
//! closed tab is live. The gates live in `handle_close_tab` and
//! `handle_close_other_tabs` / `handle_close_all_tabs` rather than at
//! each call site (strip X, context menu, Ctrl+W and the terminal's own
//! path all reach them).

use super::*;

impl Oryxis {
    pub(super) fn handle_tabs_strip(&mut self, message: TabsMessage) -> Task<Message> {
        match message {
            TabsMessage::SelectTab(idx) => return self.handle_select_tab(idx),
            TabsMessage::TabChipPressed(idx, at) => {
                // Off: no press is half of anything, and no stamp is left
                // for the moment the setting goes on.
                let Some(id) = self.tabs.get(idx).map(|t| t._id).filter(|_| {
                    self.vaults.cur().prefs.tab_double_click != "off"
                }) else {
                    self.nav.tab_click.reset();
                    self.nav.tab_press_verdict = None;
                    return Task::none();
                };
                // A modified press is never half of a double click, and must
                // not turn the next plain press into one.
                if self.modifiers.control()
                    || self.modifiers.command()
                    || self.modifiers.shift()
                    || self.modifiers.alt()
                {
                    self.nav.tab_click.reset();
                    self.nav.tab_press_verdict = None;
                    return Task::none();
                }
                let rule = crate::double_click::DoubleClickRule::os();
                let report = self.nav.tab_click.press(id, at, std::time::Instant::now(), &rule);
                self.nav.tab_press_verdict = Some((id, report));
            }
            TabsMessage::TabChipClicked(idx) => {
                // Taken whatever happens below: a verdict belongs to the one
                // release its press began.
                let id = self.tabs.get(idx).map(|t| t._id);
                let double = self
                    .nav
                    .tab_press_verdict
                    .take()
                    .filter(|(key, _)| Some(*key) == id)
                    .is_some_and(|(_, report)| report.double);
                // The first click of the pair already selected the tab, so
                // the second one only acts.
                if double {
                    match self.vaults.cur().prefs.tab_double_click.as_str() {
                        "duplicate" => return self.handle_duplicate_tab(idx),
                        "rename" => {
                            return Task::done(Message::Tabs(TabsMessage::StartRenameTab(idx)));
                        }
                        _ => {}
                    }
                }
                return self.handle_select_tab(idx);
            }
            TabsMessage::CloseTab(idx) => return self.handle_close_tab(idx),
            // The strip's X, which is the only close the mouse performs
            // in a row: stamp the streak so the chip that slides under
            // the cursor next shows its own X on arrival instead of
            // asking for another dwell (issue #186).
            TabsMessage::CloseTabFromStrip(idx) => {
                self.nav.hover.tab_close_click_at = Some(std::time::Instant::now());
                return self.handle_close_tab(idx);
            }
            // The Confirm* arms resolve the dialog's tab id to an index
            // NOW, at execution: the modal can sit open across async
            // updates (an auto-reconnect's remove + rebuild shifts
            // indices), so the id is the only reference that survives.
            // An id that no longer resolves is a tab that closed itself
            // while the question was on screen: nothing to confirm.
            TabsMessage::ConfirmCloseGroupedTab(id) => {
                return match self.tab_index_by_id(id) {
                    Some(idx) => self.close_tab_now(idx),
                    None => Task::none(),
                };
            }
            TabsMessage::ConfirmCloseLiveTab(id) => {
                return match self.tab_index_by_id(id) {
                    Some(idx) => self.close_tab_now(idx),
                    None => Task::none(),
                };
            }
            TabsMessage::ReopenClosedTab => return self.handle_reopen_closed_tab(),
            TabsMessage::CloseOtherTabs(idx) => return self.handle_close_other_tabs(idx),
            TabsMessage::ConfirmCloseOtherTabs(id) => {
                return match self.tab_index_by_id(id) {
                    Some(idx) => self.close_other_tabs_now(idx),
                    None => Task::none(),
                };
            }
            TabsMessage::CloseAllTabs => return self.handle_close_all_tabs(),
            TabsMessage::ConfirmCloseAllTabs => return self.close_all_tabs_now(),
            TabsMessage::ClosePanelTab(kind) => {
                return self.close_panel_tab(kind);
            }
            TabsMessage::ToggleTabPin(idx) => {
                self.overlay = None;
                if let Some(tab) = self.tabs.get_mut(idx) {
                    tab.pinned = !tab.pinned;
                    // An explicit re-pin is the one gesture that retires an
                    // inherited SFTP pin (H5): from here on this tab is
                    // pinned as the terminal it now is, not as the SFTP tab
                    // it absorbed.
                    tab.inherited_pin = None;
                }
                self.persist_pinned_tabs();
            }
            TabsMessage::ReconnectTab(idx) => return self.handle_reconnect_tab(idx),
            TabsMessage::DuplicateTab(idx) => return self.handle_duplicate_tab(idx),
            TabsMessage::DuplicateInNewWindow(idx) => {
                return self.duplicate_in_new_window(idx);
            }
            TabsMessage::TabDragOverChip(over, at) => self.slide_dragged_tab_over(over, at),
            TabsMessage::TabDragToEnd => {
                // The `+` ends the strip. Inline, the slide already put the
                // tab last once the cursor passed the last chip's middle;
                // docked at the edge of an overflowing strip, the last chip
                // may be scrolled out of reach, and this is the way there.
                if let Some(drag) = self.tab_drag.filter(|d| d.active) {
                    self.slide_tab_to_partition_end(drag.from_id);
                }
            }
            TabsMessage::TabBarWheel(dy) => {
                // Vertical wheel over the tab bar scrolls horizontally
                // iced's horizontal-only scrollable ignores y deltas, so
                // we translate them via scroll_by here. Sign flip so
                // wheel-down brings later tabs into view (matches the
                // direction Chrome/VS Code use).
                return iced::widget::operation::scrollable::scroll_by(
                    iced::widget::Id::new("tab-scroll"),
                    iced::widget::scrollable::AbsoluteOffset { x: -dy, y: 0.0 },
                    iced::widget::operation::Animation::Instant,
                );
            }
            TabsMessage::ShowTabMenu(idx) => {
                // The anchor is a one-shot: take it before the guard
                // below, or a keyboard-placed anchor would stay armed
                // and land the NEXT menu where this one was asked for.
                let anchor = self.keynav_take_menu_anchor();
                // Store the tab's id, not the index: the menu outlives
                // the frame that opened it, and the auto-reconnect tick
                // can remove + re-append a tab while it is up (see
                // `OverlayContent::TabActions`).
                // DIAG (issue #276): temporary, remove with the tab-menu fix.
                tracing::info!(
                    "diag: ShowTabMenu idx={idx} tabs={} anchor=({:.1}, {:.1}) cursor=({:.1}, {:.1}) window=({:.1}, {:.1})",
                    self.tabs.len(),
                    anchor.0,
                    anchor.1,
                    self.cur_mouse().x,
                    self.cur_mouse().y,
                    self.cur_window_size().width,
                    self.cur_window_size().height,
                );
                let Some(tab_id) = self.tabs.get(idx).map(|t| t._id) else {
                    return Task::none();
                };
                self.overlay = Some(OverlayState {
                    content: OverlayContent::TabActions(tab_id),
                    x: anchor.0,
                    y: anchor.1,
                });
            }
            TabsMessage::ToggleSidebarCollapsed => {
                // Flip the side strip between its full-width form and the
                // narrow icon-only rail, mirroring the vault nav rail's
                // expand/collapse. Persisted so the choice sticks.
                self.vaults.cur_mut().prefs.sidebar_collapsed =
                    !self.vaults.cur().prefs.sidebar_collapsed;
                self.persist_setting(
                    "sidebar_collapsed",
                    if self.vaults.cur().prefs.sidebar_collapsed { "true" } else { "false" },
                );
            }
            TabsMessage::ShowTabBarMenu => {
                // The strip's own menu, at the cursor. A chip's right
                // press is captured by the chip, so reaching here means
                // the click landed on empty strip (issue #186).
                let anchor = self.keynav_take_menu_anchor();
                self.overlay = Some(OverlayState {
                    content: OverlayContent::TabBarActions,
                    x: anchor.0,
                    y: anchor.1,
                });
            }
            TabsMessage::ShowSplitMenu => {
                // Hover popover under `+`, anchored under the button.
                // It opens when it has something to offer beyond what
                // clicking `+` already does: a terminal tab to split, or
                // a closed tab to bring back (issue #186, which is also
                // why the second half matters: after closing the last
                // tab there is no tab to split, and that is exactly the
                // moment the reopen is being looked for).
                // An open tab is the whole split test: the `active_view`
                // half this used to also require is never assigned by the
                // ordinary path (opening a host from the Dashboard pushes
                // a tab and leaves the view where it was), so the popover
                // was dead exactly where splitting is most wanted.
                if (self.cur_active_tab().is_some() || !self.vaults.cur().closed_tabs.is_empty())
                    && !matches!(
                        self.overlay.as_ref().map(|o| &o.content),
                        Some(OverlayContent::SplitMenu)
                    )
                {
                    // Anchor under the `+` button at a fixed position (its
                    // reported bounds), not the cursor, so the popover lines
                    // up cleanly with the button: from its leading edge,
                    // so under RTL it hangs under the button too.
                    let b = self.plus_btn_bounds.get();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SplitMenu,
                        x: crate::widgets::leading_edge_x(b),
                        y: b.y + b.height,
                    });
                }
            }
            TabsMessage::SplitMenuEnter => {
                self.split_menu_hovered = true;
            }
            TabsMessage::SplitMenuLeave => {
                // Left the `+` button or the popover. Defer the close briefly
                // so moving from the button INTO the menu (which re-enters
                // via `SplitMenuEnter`) doesn't flap it shut.
                self.split_menu_hovered = false;
                return Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(180)).await;
                    },
                    |_| Message::Tabs(TabsMessage::SplitMenuCloseIfIdle),
                );
            }
            TabsMessage::SplitMenuCloseIfIdle => {
                if !self.split_menu_hovered
                    && matches!(
                        self.overlay.as_ref().map(|o| &o.content),
                        Some(OverlayContent::SplitMenu)
                    )
                {
                    self.overlay = None;
                }
            }
            TabsMessage::ActivateStripSlot(slot) => {
                if let Some(msg) = self.strip_slot_target(slot) {
                    return Task::done(msg);
                }
            }
            TabsMessage::CopyTabAddress(idx) => {
                self.overlay = None;
                // Resolve through the focused pane's origin, not the tab
                // label: a split tab can hold panes on different hosts and
                // the label may be renamed. `CopyToClipboard` owns the write
                // (one clipboard access per process) and toasts only once the
                // runtime confirms it landed.
                let address = self
                    .tabs
                    .get(idx)
                    .map(|t| t.active().id)
                    .and_then(|pane_id| self.pane_origin_connection(pane_id))
                    .map(|c| c.hostname.clone());
                if let Some(address) = address {
                    return self.update(Message::CopyToClipboard(address));
                }
            }
            TabsMessage::ToggleTabFilesMode(idx) => return self.handle_toggle_tab_files_mode(idx),
            TabsMessage::ShowTabSurface(idx, surface) => {
                // The chip and the segments live on the strip, so the
                // overlay a right-click left open goes with them.
                self.overlay = None;
                return self.show_tab_surface(idx, surface);
            }
            TabsMessage::DetachTabSftp(idx) => return self.handle_detach_tab_sftp(idx),
            TabsMessage::CloseTabSftpSession(idx) => return self.handle_close_tab_sftp_session(idx),
            TabsMessage::OpenTerminalForSftpTab(idx) => return self.handle_open_terminal_for_sftp_tab(idx),
            TabsMessage::SsmKeepaliveTick => {
                // Toggle each SSM/ECS terminal between `base` and
                // `base - 1` rows. Every tick is therefore a genuine size
                // change, which fires a SIGWINCH the plugin forwards to
                // SSM as a resize event, and resize events reset the
                // server's idle timer. No base means we're focused (the
                // ticker shouldn't be mounted then), so it's a no-op.
                if let Some((base_cols, base_rows)) = self.ssm_keepalive_base {
                    let shrunk = base_rows.saturating_sub(1).max(2);
                    for pane in self.plugin_panes() {
                        if let Ok(mut state) = pane.terminal.lock() {
                            let target = if state.rows() == base_rows {
                                shrunk
                            } else {
                                base_rows
                            };
                            state.resize(base_cols, target);
                        }
                    }
                }
            }
            TabsMessage::BusyAnimTick => {
                // The increment IS the re-render: the strip derives the
                // marching-dots frame from this counter (issue #146).
                self.busy_anim_tick = self.busy_anim_tick.wrapping_add(1);
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
