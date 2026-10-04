//! tmux session manager dispatch (issue #116).
//!
//! Three of the four actions ride an exec channel multiplexed on the
//! pane's live SSH session (list, create, kill), so they never touch
//! the user's shell. Attach is the exception and has to be: it is the
//! command the user would type, so it is typed, into the pane the tab
//! sits beside, on their click.

use std::time::Duration;

use iced::Task;
use uuid::Uuid;

use crate::app::{Message, Oryxis, TmuxMessage};
use crate::tmux::model::TmuxStatus;
use crate::tmux::probe;

/// A listing is a single short command; a host that cannot answer in
/// this long is a host the tab should report on rather than wait for.
const TMUX_TIMEOUT: Duration = Duration::from_secs(8);


impl Oryxis {
    pub(crate) fn handle_tmux(&mut self, message: TmuxMessage) -> Task<Message> {
        match message {
            TmuxMessage::Refresh(pane_id) => self.tmux_list(pane_id),
            TmuxMessage::Listed(pane_id, result) => {
                self.tmux.end_probe(&pane_id);
                // The pane may have disconnected or closed while the
                // listing was in flight; `forget` dropped its entry, and
                // re-creating one here would paint sessions read over a
                // transport that no longer exists.
                if self.tmux.get(&pane_id).is_none() {
                    return Task::none();
                }
                // Has the typed attach actually registered? A tmux
                // client draws the ALTERNATE SCREEN, so the pane being
                // on it is the direct evidence, with no timing
                // assumption: read it before borrowing the entry.
                let pane_in_alt_screen = self.pane_alt_screen(pane_id);
                let entry = self.tmux.entry(pane_id);
                entry.status = match result {
                    Ok(payload) => match probe::parse_listing(&payload) {
                        probe::Listing::NoTmux => TmuxStatus::NoTmux,
                        probe::Listing::Sessions(sessions) => TmuxStatus::Ready(sessions),
                    },
                    Err(e) => TmuxStatus::Failed(e),
                };
                // The listing corrects the "attached here" hint: a
                // session that is gone, or that tmux reports with zero
                // clients, cannot be the one this pane is showing.
                if let Some(name) = entry.attached_to.clone()
                    && !crate::tmux::model::hint_survives_listing(
                        &entry.status,
                        &name,
                        pane_in_alt_screen,
                    )
                {
                    entry.attached_to = None;
                }
                Task::none()
            }
            TmuxMessage::Attach(tab_idx, pane_id, name) => self.tmux_attach(tab_idx, pane_id, name),
            TmuxMessage::SwitchClients(tab_idx, pane_id, name, result) => {
                self.tmux_switch_clients(tab_idx, pane_id, name, result)
            }
            TmuxMessage::SwitchDone(pane_id, name, result) => {
                if self.tmux.get(&pane_id).is_none() {
                    return Task::none();
                }
                match result {
                    Ok(()) => {
                        let entry = self.tmux.entry(pane_id);
                        entry.error = None;
                        // Move our one client inside the list already on
                        // screen, so the rows are right THIS frame instead
                        // of after a round trip (the ~1s of stale
                        // "attached" BabakAmini reported); the immediate
                        // re-list below confirms against the host.
                        let previous = entry.attached_to.replace(name.clone());
                        if let TmuxStatus::Ready(sessions) = &mut entry.status {
                            if let Some(prev) = previous
                                && let Some(s) = sessions.iter_mut().find(|s| s.name == prev)
                            {
                                s.attached = s.attached.saturating_sub(1);
                            }
                            if let Some(s) = sessions.iter_mut().find(|s| s.name == name) {
                                s.attached += 1;
                            }
                        }
                        self.tmux_list(pane_id)
                    }
                    Err(e) => {
                        // The switch may or may not have landed (a
                        // vanished session, a raced kill): surface the
                        // host's wording and re-read the truth.
                        self.tmux.entry(pane_id).error = Some(e);
                        self.tmux_list(pane_id)
                    }
                }
            }
            TmuxMessage::NewNameChanged(pane_id, value) => {
                self.tmux.entry(pane_id).new_name = value;
                Task::none()
            }
            // The name field's Enter (issue #160). The fork's
            // `text_input` fires `on_submit` on ANY Enter, focused or
            // not, so every Enter typed into the terminal lands here
            // while the tab is open; acting directly ran
            // `tmux new-session -d` on the host once per command the
            // user executed. Resolve which widget iced actually has
            // focused first, and only the name field's own Enter
            // creates.
            TmuxMessage::Submitted(pane_id) => {
                iced::widget::operation::find_focused().map(move |focused| {
                    Message::Tmux(TmuxMessage::SubmittedFocus(pane_id, focused))
                })
            }
            TmuxMessage::SubmittedFocus(pane_id, focused) => {
                if focused == Some(crate::tmux::new_name_input_id()) {
                    self.tmux_create(pane_id)
                } else {
                    Task::none()
                }
            }
            TmuxMessage::Create(pane_id) => self.tmux_create(pane_id),
            TmuxMessage::AskKill(pane_id, name) => {
                let entry = self.tmux.entry(pane_id);
                entry.confirm_kill = Some(name);
                entry.error = None;
                Task::none()
            }
            TmuxMessage::CancelKill(pane_id) => {
                self.tmux.entry(pane_id).confirm_kill = None;
                Task::none()
            }
            TmuxMessage::ConfirmKill(pane_id) => self.tmux_kill(pane_id),
            TmuxMessage::RowHovered(idx) => {
                self.nav.hover.tmux_row = Some(idx);
                Task::none()
            }
            TmuxMessage::RowExit(idx) => {
                // Guarded clear: see `HoverState::leave`.
                self.nav.hover.leave_tmux_row(idx);
                Task::none()
            }
            TmuxMessage::ActionDone(pane_id, result) => {
                if self.tmux.get(&pane_id).is_none() {
                    return Task::none();
                }
                match result {
                    // The host's answer is the listing, not our
                    // optimism: re-read rather than editing the list we
                    // are holding.
                    Ok(()) => {
                        self.tmux.entry(pane_id).error = None;
                        self.tmux_list(pane_id)
                    }
                    Err(e) => {
                        self.tmux.entry(pane_id).error = Some(e);
                        Task::none()
                    }
                }
            }
        }
    }

    /// Run the listing for a pane, if it still holds a live SSH session.
    fn tmux_list(&mut self, pane_id: Uuid) -> Task<Message> {
        let Some(session) = self.tmux_session_for_pane(pane_id) else {
            return Task::none();
        };
        // A slow host is skipped rather than queueing listings behind
        // each other: the previous one still holds a channel.
        if !self.tmux.begin_probe(pane_id) {
            return Task::none();
        }
        // Keep whatever is on screen while refreshing an existing list,
        // so the rows don't blink away on every action; only a first
        // load shows the spinner.
        let entry = self.tmux.entry(pane_id);
        if matches!(entry.status, TmuxStatus::Idle) {
            entry.status = TmuxStatus::Loading;
        }
        let command = probe::list_sessions_command();
        Task::perform(
            async move {
                match session.probe(&command, TMUX_TIMEOUT).await {
                    Some(payload) => Ok(payload),
                    None => Err(crate::i18n::t("tmux_probe_failed").to_string()),
                }
            },
            move |result| Message::Tmux(TmuxMessage::Listed(pane_id, result)),
        )
    }

    /// Route an attach click: typed into the pane when the pane sits at
    /// its own shell, switched out-of-band when it is already inside
    /// another session.
    ///
    /// The split exists because the two states need opposite tools
    /// (issue #159 follow-up). Outside tmux, attaching MUST go through
    /// the foreground shell: it is the shell that hands its tty over to
    /// the tmux client. Inside tmux, a typed line lands in whatever the
    /// current session is running and waits for it (`sleep 10` delayed
    /// the switch until it finished), while `switch-client` on an exec
    /// channel moves the client instantly, busy or not.
    fn tmux_attach(&mut self, tab_idx: usize, pane_id: Uuid, name: String) -> Task<Message> {
        // A modal on screen owns the keyboard, and the fork's
        // `text_input` returns its `on_submit` binding for Enter before
        // the focus gate, so an Enter meant for the modal can reach
        // this handler.
        if self.any_modal_blocks_input() {
            return Task::none();
        }
        // The session this pane already shows is inert (issue #159):
        // its row renders unclickable, and this guard covers the
        // remaining paths (a stale frame, the keynav ring), because the
        // typed line would land INSIDE that very session, i.e. in
        // whatever program runs there.
        if self
            .tmux
            .get(&pane_id)
            .is_some_and(|e| e.attached_to.as_deref() == Some(name.as_str()))
        {
            return Task::none();
        }
        if let Some(current) = self
            .tmux
            .get(&pane_id)
            .and_then(|e| e.attached_to.clone())
        {
            return self.tmux_switch_begin(tab_idx, pane_id, current, name);
        }
        self.tmux_attach_typed(tab_idx, pane_id, name)
    }

    /// First half of the out-of-band switch: read which clients the
    /// CURRENT session holds, so the switch can name OUR client. Naming
    /// it is what keeps this deterministic: a bare `switch-client` run
    /// outside any client resolves "current client" to the most
    /// recently used one anywhere and moves it (the issue #157 hijack).
    fn tmux_switch_begin(
        &mut self,
        tab_idx: usize,
        pane_id: Uuid,
        current: String,
        name: String,
    ) -> Task<Message> {
        let Some(session) = self.tmux_session_for_pane(pane_id) else {
            return Task::none();
        };
        let command = match probe::list_clients_command(&current) {
            Ok(cmd) => cmd,
            Err(e) => {
                self.tmux.entry(pane_id).error = Some(e.to_string());
                return Task::none();
            }
        };
        self.tmux.entry(pane_id).error = None;
        Task::perform(
            async move {
                session
                    .probe(&command, TMUX_TIMEOUT)
                    .await
                    .ok_or_else(|| crate::i18n::t("tmux_probe_failed").to_string())
            },
            move |result| {
                Message::Tmux(TmuxMessage::SwitchClients(
                    tab_idx,
                    pane_id,
                    name.clone(),
                    result,
                ))
            },
        )
    }

    /// Second half: the `list-clients` payload arrived. A client tty
    /// means the pane really is inside tmux, so move that client on an
    /// exec channel. No client at all means the "attached here" hint
    /// was stale (the user detached by hand between listings): retire
    /// it and attach the ordinary typed way, because the pane's shell
    /// is back outside tmux where the typed line is the right tool.
    fn tmux_switch_clients(
        &mut self,
        tab_idx: usize,
        pane_id: Uuid,
        name: String,
        result: Result<String, String>,
    ) -> Task<Message> {
        if self.tmux.get(&pane_id).is_none() {
            return Task::none();
        }
        let payload = match result {
            Ok(payload) => payload,
            Err(e) => {
                self.tmux.entry(pane_id).error = Some(e);
                return Task::none();
            }
        };
        let Some(tty) = probe::parse_clients(&payload) else {
            self.tmux.entry(pane_id).attached_to = None;
            return self.tmux_attach_typed(tab_idx, pane_id, name);
        };
        let Some(session) = self.tmux_session_for_pane(pane_id) else {
            return Task::none();
        };
        let command = match probe::switch_client_command(&tty, &name) {
            Ok(cmd) => cmd,
            Err(e) => {
                self.tmux.entry(pane_id).error = Some(e.to_string());
                return Task::none();
            }
        };
        Task::perform(
            async move {
                // `exec_capture`, not `probe`: tmux reports "can't find
                // session" on stderr, which `probe` never collects, and
                // the exit status is the only honest success signal.
                match session.exec_capture(&command, None, TMUX_TIMEOUT).await {
                    Some(res) if res.exit_code == 0 => Ok(()),
                    Some(res) => {
                        let reason = res.stderr.trim();
                        let reason = if reason.is_empty() {
                            res.stdout.trim()
                        } else {
                            reason
                        };
                        Err(if reason.is_empty() {
                            crate::i18n::t("tmux_action_failed").to_string()
                        } else {
                            reason.to_string()
                        })
                    }
                    None => Err(crate::i18n::t("tmux_action_failed").to_string()),
                }
            },
            move |result| Message::Tmux(TmuxMessage::SwitchDone(pane_id, name.clone(), result)),
        )
    }

    /// Type `tmux attach -t <name>` into the pane the tab belongs to.
    ///
    /// This is the one action that reaches the user's own shell. The
    /// name came off the host, so it is quoted (and a name carrying a
    /// line break is refused rather than quoted, which would otherwise
    /// let the host run a second command here).
    fn tmux_attach_typed(&mut self, tab_idx: usize, pane_id: Uuid, name: String) -> Task<Message> {
        let command = match probe::attach_command(&name) {
            Ok(cmd) => cmd,
            Err(e) => {
                self.tmux.entry(pane_id).error = Some(e.to_string());
                return Task::none();
            }
        };
        let entry = self.tmux.entry(pane_id);
        entry.error = None;
        // Believed, not proven: the listing and the alternate-screen
        // edge correct it if the command lands somewhere unexpected.
        entry.attached_to = Some(name);
        self.write_ring_injection_to_tab(tab_idx, format!("{command}\n").as_bytes());
        // Re-read after the shell has had a moment to run the line: a
        // switch between two sessions never leaves the alternate
        // screen, so the flip-edge refresh cannot see it, and the
        // attached counts on screen would go stale (issue #158).
        Task::perform(
            async {
                tokio::time::sleep(Duration::from_millis(1200)).await;
            },
            move |()| Message::Tmux(TmuxMessage::Refresh(pane_id)),
        )
    }

    /// Create a DETACHED session over the exec channel. Detached so it
    /// never fights the pane's own PTY; the user attaches afterwards
    /// with the same gesture any other session takes.
    fn tmux_create(&mut self, pane_id: Uuid) -> Task<Message> {
        // Same fork Enter hazard as `tmux_attach`: the new-name field
        // carries `on_submit`, so an Enter meant for a modal on screen
        // would otherwise create a session on the remote host.
        if self.any_modal_blocks_input() {
            return Task::none();
        }
        let Some(session) = self.tmux_session_for_pane(pane_id) else {
            return Task::none();
        };
        let name = self.tmux.entry(pane_id).new_name.trim().to_string();
        // An empty name is not an error: tmux numbers the session
        // itself, which is what `tmux new -d` does on the command line.
        let command = if name.is_empty() {
            "tmux new-session -d".to_string()
        } else {
            match probe::new_session_command(&name) {
                Ok(cmd) => cmd,
                Err(e) => {
                    self.tmux.entry(pane_id).error = Some(e.to_string());
                    return Task::none();
                }
            }
        };
        self.tmux.entry(pane_id).new_name.clear();
        self.tmux_run_action(pane_id, session, command)
    }

    /// Kill the session parked by `AskKill`.
    fn tmux_kill(&mut self, pane_id: Uuid) -> Task<Message> {
        let Some(name) = self.tmux.entry(pane_id).confirm_kill.take() else {
            return Task::none();
        };
        let Some(session) = self.tmux_session_for_pane(pane_id) else {
            return Task::none();
        };
        let command = match probe::kill_session_command(&name) {
            Ok(cmd) => cmd,
            Err(e) => {
                self.tmux.entry(pane_id).error = Some(e.to_string());
                return Task::none();
            }
        };
        self.tmux_run_action(pane_id, session, command)
    }

    /// Shared tail of create / kill: run the command on an exec channel
    /// and report through `ActionDone`, which re-lists on success.
    fn tmux_run_action(
        &mut self,
        pane_id: Uuid,
        session: std::sync::Arc<oryxis_ssh::SshSession>,
        command: String,
    ) -> Task<Message> {
        Task::perform(
            async move {
                match session.probe(&command, TMUX_TIMEOUT).await {
                    // tmux prints nothing on success and a reason on
                    // failure, so a non-empty answer IS the error. It
                    // is the host's own wording, which beats a generic
                    // string that hides "duplicate session: work".
                    Some(output) if output.trim().is_empty() => Ok(()),
                    Some(output) => Err(output.trim().to_string()),
                    None => Err(crate::i18n::t("tmux_action_failed").to_string()),
                }
            },
            move |result| Message::Tmux(TmuxMessage::ActionDone(pane_id, result)),
        )
    }

    /// The live SSH session behind a pane, when the feature is on and
    /// the transport is still up. `None` for local / serial / telnet
    /// panes and for dead sessions.
    /// Whether the pane's emulator sat on the alternate screen after
    /// the last output batch. A tmux client draws there, so this is the
    /// evidence that a typed attach registered (`Listed` reads it
    /// before trusting a zero-client listing). A pane that is gone
    /// answers `false`: it is showing no tmux client either.
    fn pane_alt_screen(&self, pane_id: Uuid) -> bool {
        self.tabs
            .iter()
            .find_map(|tab| tab.pane_grid.panes.values().find(|p| p.id == pane_id))
            .is_some_and(|pane| pane.alt_screen)
    }

    pub(crate) fn tmux_session_for_pane(
        &self,
        pane_id: Uuid,
    ) -> Option<std::sync::Arc<oryxis_ssh::SshSession>> {
        if !self.prefs.tmux_manager {
            return None;
        }
        let pane = self
            .tabs
            .iter()
            .find_map(|tab| tab.pane_grid.panes.values().find(|p| p.id == pane_id))?;
        let ssh = pane.session.as_ref().and_then(|s| s.ssh())?;
        ssh.is_alive().then(|| ssh.clone())
    }

    /// True while the tmux tab is the visible sidebar tab, which is what
    /// arms the on-open listing: the manager never probes a screen
    /// nobody is looking at.
    pub(crate) fn tmux_tab_visible(&self) -> bool {
        self.sidebar_tab_shown(crate::state::TerminalSidebarTab::Tmux)
    }

    /// Idempotent "the tmux tab is on screen for this pane" sync, called
    /// from every entry point that can reveal it (tab selected, pane
    /// focused, sidebar opened, session (re)connected). Every reveal
    /// re-reads the host (issue #158: the list changes behind the tab's
    /// back, so a cached listing shown as current was a lie); the rows
    /// already on screen stay put while the probe runs, only a first
    /// load shows the spinner, and the in-flight guard collapses
    /// repeated reveals into one probe.
    pub(crate) fn tmux_sync(&mut self) -> Task<Message> {
        if !self.tmux_tab_visible() {
            return Task::none();
        }
        let Some(pane_id) = self.active_pane_mut().map(|p| p.id) else {
            return Task::none();
        };
        if self.tmux_session_for_pane(pane_id).is_none() {
            return Task::none();
        }
        // Materialize the entry first: `Listed` drops results for
        // panes it cannot find, and `tmux_list` only creates one
        // when it gets past the session check.
        self.tmux.entry(pane_id);
        self.tmux_list(pane_id)
    }

    /// Alternate-screen edge for a pane, reported by the `PtyOutput`
    /// funnel. The flip is the attach/detach signal a tmux client
    /// leaves in the byte stream (attaching draws the alternate screen,
    /// detaching restores the primary), so it drives the auto-refresh
    /// the tab cannot get any other way (issue #158). vim and htop flip
    /// the same switch; the extra listing they cause is bounded by the
    /// tab having to be on screen and the in-flight guard.
    pub(crate) fn tmux_alt_screen_edge(&mut self, pane_id: Uuid, entered: bool) -> Task<Message> {
        // Leaving the alternate screen means whatever the pane was
        // showing full-screen is gone, the believed attach included
        // (issue #159). The hint is retired even with the tab hidden;
        // a wrong "attached here" must not survive until the next
        // reveal.
        if !entered
            && let Some(entry) = self.tmux.get(&pane_id)
            && entry.attached_to.is_some()
        {
            self.tmux.entry(pane_id).attached_to = None;
        }
        if !self.tmux_tab_visible() {
            return Task::none();
        }
        // Only the pane the tab is reading (the active tab's focused
        // pane): a background pane's vim session is not this tab's
        // business.
        if self.active_pane_mut().map(|p| p.id) != Some(pane_id) {
            return Task::none();
        }
        self.tmux_list(pane_id)
    }

    /// Drop a pane's listing on disconnect / close. A list that outlived
    /// its transport would offer attaches that cannot happen.
    pub(crate) fn tmux_reset_pane(&mut self, pane_id: &Uuid) {
        self.tmux.forget(pane_id);
    }

    /// Drop every listing (feature turned off, vault locked).
    pub(crate) fn tmux_reset_all(&mut self) {
        self.tmux.clear();
    }
}
