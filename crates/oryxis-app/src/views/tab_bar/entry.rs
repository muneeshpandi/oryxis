//! Tab bar: per-entry element builder, shared by the horizontal strips
//! (top / bottom, `tab_strip_bar`) and the side-docked vertical strip
//! (`view_side_tab_strip`, issue #87). Split out of views/tab_bar/mod.rs
//! so the orientation assemblers can't drift on the per-tab derivation
//! (privacy labels, accents, status dots, hybrid glyphs, drag gaps).

use super::*;

/// Per-frame inputs shared by every strip entry, computed once by the
/// strip assembler and threaded into `strip_tab_element` per
/// `strip_order` entry.
pub(crate) struct StripCtx {
    /// Privacy Mode terms, one pass for the whole strip (issue #78).
    pub(crate) privacy_terms: Vec<String>,
    pub(crate) close_on_right: bool,
    /// Whether the chip under the cursor has earned its close X: the
    /// reveal waits for a hover dwell (issue #186). One flag for the
    /// whole strip, because one chip holds the hover at a time.
    pub(crate) close_armed: bool,
    pub(crate) compact_pins: bool,
    pub(crate) solid_fill: bool,
    /// Uniform chip width for the vertical strip; `None` = horizontal
    /// allocation (active natural / content-hugged / precomputed).
    pub(crate) uniform_w: Option<f32>,
    /// Horizontal per-tab widths, indexed by terminal storage index
    /// (active natural, inactives content-hugged and possibly shrunk
    /// under overflow). Empty in vertical mode.
    pub(crate) session_widths: Vec<f32>,
    /// Width every chip reserves for the tab-number prefix
    /// (`Oryxis::tab_number_px`), 0 when numbering is off or drawn in
    /// the badge slot.
    pub(crate) number_px: f32,
}

impl Oryxis {
    /// Icon-only Home area tab: the route back to the vault surface
    /// (the vault identity / switcher lives on the contextual sub-nav).
    /// It stays selected across every vault sub-section, mirroring the
    /// `in_vault_area` family used in `layout.rs`. Rendered by the
    /// combined top bar and, in side-dock mode, by the slim chrome bar
    /// (the vertical strip carries session tabs only). Settings stays
    /// out on purpose - it lives in the burger menu so it doesn't take
    /// a permanent slot.
    ///
    /// With the switcher in the tab bar (`vault_switcher_in_tabs`), the
    /// Home tab carries the window's vault: its name, and a chevron that
    /// opens the switcher. `compact` is the side-docked strip's narrow
    /// header, where the name moves into a tooltip.
    pub(crate) fn home_area_tab(&self, solid_fill: bool, compact: bool) -> Element<'_, Message> {
        let nav_active = self.cur_active_tab().is_none();
        let in_vault_area = matches!(
            self.cur_view(),
            View::Dashboard
                | View::Keys
                | View::Snippets
                | View::PortForwarding
                | View::Cloud
                | View::Proxies
                | View::KnownHosts
                | View::History
        );
        if self.vault_switcher_on_home_tab() {
            let menu_open = self
                .cur_overlay()
                .is_some_and(|o| matches!(o.content, crate::state::OverlayContent::VaultSwitcher));
            return vault_home_tab(
                self.vault_name(self.vaults.cur().id),
                nav_active && in_vault_area,
                solid_fill,
                compact,
                menu_open,
                self.vault_tab_bounds.clone(),
            );
        }
        area_tab(
            "",
            iced_fonts::lucide::house(),
            Message::Navigation(NavigationMessage::GoHome),
            nav_active && in_vault_area,
            solid_fill,
        )
    }

    /// The Home tab's width in the horizontal strip's budget.
    pub(crate) fn home_area_tab_width(&self) -> f32 {
        if self.vault_switcher_on_home_tab() {
            vault_home_tab_width(&self.vault_name(self.vaults.cur().id), false)
        } else {
            TAB_ROW_HEIGHT
        }
    }

    /// Whether the active tab drag (if any) picked up a pinned tab. In
    /// side-dock mode with `pinned_tabs_top_bar` on, this decides which
    /// surface draws the floating ghost: the chrome bar (pinned) or the
    /// vertical strip (unpinned).
    pub(crate) fn dragged_tab_pinned(&self) -> bool {
        let Some(dragged) = self.strip_drag_id() else {
            return false;
        };
        self.tabs
            .iter()
            .find(|t| t._id == dragged)
            .map(|t| t.pinned)
            .or_else(|| {
                self.sftp_tabs
                    .iter()
                    .find(|t| t.id == dragged)
                    .map(|t| t.pinned)
            })
            .unwrap_or(false)
    }

    /// Whether a `strip_order` entry is pinned (used by the vertical
    /// strip to pack consecutive compact chips into rows).
    pub(crate) fn strip_entry_pinned(&self, entry: StripEntry) -> bool {
        match entry {
            StripEntry::Sftp(idx) => self.sftp_tabs[idx].pinned,
            StripEntry::Terminal(idx) => self.tabs[idx].pinned,
            // Transient by design, so pinning it would promise a
            // persistence it does not have.
            StripEntry::Panel(_) => false,
        }
    }

    /// Build the strip element for one `strip_order` entry: the session
    /// or SFTP tab, its compact pinned chip, or the same-width gap the
    /// other tabs slide around while that entry is being dragged.
    ///
    /// `slot` is the entry's 0-based position in the FULL strip order,
    /// which is what the tab number renders. Callers that skip entries
    /// (the chrome bar takes only the pinned ones, the side strip only
    /// the unpinned) must still count from the unfiltered order, or the
    /// numbering would restart per surface.
    pub(crate) fn strip_tab_element(
        &self,
        ctx: &StripCtx,
        entry: StripEntry,
        slot: usize,
    ) -> Element<'_, Message> {
        let id = match entry {
            StripEntry::Terminal(idx) => self.tabs[idx]._id,
            StripEntry::Sftp(idx) => self.sftp_tabs[idx].id,
            StripEntry::Panel(kind) => kind.tab_id(),
        };
        // During a reorder drag begun in this window every other chip
        // reports the cursor's moves over it, in its own coordinates:
        // the live slide (`slide_dragged_tab_over`). The wrapper is
        // always there (widget state is matched by position) and carries
        // `on_move` alone: an `on_enter` would take the entering move.
        let mut chip = MouseArea::new(self.strip_tab_chip(ctx, entry, slot));
        if self
            .tab_drag
            .is_some_and(|d| d.active && d.from_id != id && self.floats_here())
        {
            chip = chip.on_move(move |at| Message::Tabs(TabsMessage::TabDragOverChip(id, at)));
        }
        // Every chip reports where it was drawn: a tab carried over
        // this strip from another window takes its slot from them.
        crate::widgets::bounds_reporter(chip.boxed(), self.strip_chip_cell(id))
    }

    fn strip_tab_chip(
        &self,
        ctx: &StripCtx,
        entry: StripEntry,
        slot: usize,
    ) -> Element<'_, Message> {
        let active_idx = self.cur_active_tab();
        let number = self.tab_number_at(slot);
        // Terminal and SFTP tabs share one strip; SFTP tabs are active
        // only while the SFTP surface itself is up.
        let sftp_surface = self.cur_active_tab().is_none() && self.cur_view() == View::Sftp;
        if let StripEntry::Panel(kind) = entry {
            // Same active rule as the SFTP tabs: it owns the strip slot
            // only while its own surface is the one showing.
            let is_active = self.cur_active_tab().is_none() && self.cur_view() == kind.view();
            let label = self.panel_tab_label(kind);
            let width = ctx.uniform_w.unwrap_or_else(|| {
                if is_active {
                    TAB_NATURAL_WIDTH
                } else {
                    panel_tab_width(&label, ctx.number_px)
                }
            });
            let is_dragging = self.strip_drag_id() == Some(kind.tab_id());
            if is_dragging {
                return Space::new().width(width).height(TAB_ROW_HEIGHT).boxed();
            }
            return panel_tab(
                kind,
                &label,
                is_active,
                self.cur_nav().hover.panel_tab == Some(kind) && ctx.close_armed,
                width,
                ctx.close_on_right,
                ctx.solid_fill,
                number,
            );
        }
        let idx = match entry {
            StripEntry::Terminal(i) | StripEntry::Sftp(i) => i,
            StripEntry::Panel(_) => unreachable!("handled above"),
        };
        if entry == StripEntry::Sftp(idx) {
            let tab = &self.sftp_tabs[idx];
            let is_active = sftp_surface && self.cur_active_sftp() == Some(idx);
            // The mounted host (matched by the tab label = host name) drives
            // the badge icon + color, same as the terminal tabs.
            let detected_os = self.tab_detected_os(&tab.label);
            // Active-tab accent: the host's custom color if set, else the
            // OS brand color (so an Ubuntu tab "breathes" orange), else the
            // global accent for an empty (no-host) tab.
            // `tab_accent_color = "app"` pins the accent to the global
            // one: skip the whole per-host derivation (None falls back
            // to the app accent downstream).
            // The host brand colour (custom or OS), derived ONCE and
            // ungated: the folder badge always shows it (host
            // identity is the badge's job, like the terminal OS
            // badge). `tab_accent_color = "app"` only pins the gated
            // accent that drives text / wash / border.
            let brand = self.vaults.cur().connections
                .iter()
                .find(|c| c.label == tab.label)
                .and_then(|c| c.custom_color.as_deref().or(c.color.as_deref()))
                .and_then(crate::widgets::parse_hex_color)
                .or_else(|| {
                    detected_os.as_deref().map(|os| {
                        crate::os_icon::resolve_icon(Some(os), OryxisColors::t().accent).1
                    })
                });
            let badge_accent = brand.unwrap_or_else(|| OryxisColors::t().accent);
            let host_accent = self.host_accent_enabled().then_some(brand).flatten();
            // Privacy Mode redacts the rendered label (issue #78);
            // hovering the tab reveals it, mirroring the card
            // address. The width is computed from the same string
            // that renders so truncation stays consistent.
            let display_label =
                if self.cur_nav().hover.sftp_tab == Some(idx) {
                    tab.display_label().to_string()
                } else {
                    self.privacy_display_label(
                        &tab.label,
                        tab.display_label(),
                        &ctx.privacy_terms,
                    )
                };
            // Width mirrors the terminal model: NATURAL when active,
            // content-hugged otherwise; the vertical strip pins every
            // row to its uniform width.
            let width = ctx.uniform_w.unwrap_or_else(|| {
                if is_active {
                    TAB_NATURAL_WIDTH
                } else {
                    tab_content_width(&display_label, ctx.close_on_right, false, ctx.number_px)
                }
            });
            // The dragged tab floats as a ghost; leave a same-width
            // gap here that the other tabs slide around, like terminal tabs.
            let is_dragging = self.strip_drag_id() == Some(tab.id);
            if is_dragging {
                let gap_w = if ctx.compact_pins && tab.pinned { CHIP_W } else { width };
                return Space::new().width(gap_w).height(TAB_ROW_HEIGHT).boxed();
            } else if ctx.compact_pins && tab.pinned {
                return sftp_pinned_chip(idx, is_active, badge_accent, host_accent, ctx.solid_fill, number);
            }
            return sftp_session_tab(
                idx,
                &display_label,
                is_active,
                width,
                badge_accent,
                host_accent,
                self.vaults.cur().prefs.tab_accent_text,
                tab.pinned,
                ctx.solid_fill,
                number,
                Self::transfer_border(self.sftp_tab_slot(idx)),
            );
        }
        let tab = &self.tabs[idx];
        let is_active = active_idx == Some(idx);
        let is_hovered = self.cur_nav().hover.tab == Some(idx);
        // Reorder drag: the dragged tab gets the accent outline so the
        // user sees which one they picked up.
        let is_dragging = self.strip_drag_id() == Some(tab._id);
        // A split tab shows the focused pane's label + icon; a single
        // pane shows the tab's own label. Lookups (accent, OS badge)
        // key on the automatic label so a custom rename stays
        // display-only. Privacy Mode redacts the rendered label
        // (issue #78), keyed on the automatic label so a rename
        // keeps the per-host override; hovering the tab reveals it,
        // mirroring the card address.
        let display_label = if is_hovered {
            tab.display_label(self.tab_auto_title(tab)).to_string()
        } else {
            self.privacy_display_label(
                tab.auto_label(self.tab_auto_title(tab)),
                tab.display_label(self.tab_auto_title(tab)),
                &ctx.privacy_terms,
            )
        };
        let base_label = tab
            .auto_label(self.tab_auto_title(tab))
            .trim_end_matches(" (disconnected)");
        let detected_os = self.tab_detected_os(base_label);
        // During a drag every chip keeps its own allocated width: the
        // strip only permutes, so the total never changes and a swap
        // can't bounce the geometry (Chrome keeps chip sizes through a
        // reorder too). `unwrap_or_else`, not `unwrap_or`: the
        // horizontal fallback indexes `session_widths`, which is empty
        // in vertical mode and must not be evaluated eagerly.
        let width = ctx.uniform_w.unwrap_or_else(|| ctx.session_widths[idx]);
        // Per-host accent override: when this tab points at a
        // saved connection that has a custom `color`, tint the
        // active-tab fill and the tab text with that color
        // (JetBrains-style "respiração"). Otherwise the active
        // tab keeps the global accent.
        let host_accent: Option<Color> = if !self.host_accent_enabled() {
            // `tab_accent_color = "app"`: no per-host accent anywhere
            // in the strip (fill, text, borders fall back to the app
            // accent); the badge below keeps its brand colour.
            None
        } else { self.vaults.cur().connections.iter()
            .find(|c| c.label == base_label)
            // `custom_color` is what the icon picker writes (the
            // user-chosen accent). The legacy `color` field is a
            // dead column today but stays as a fallback so any
            // future code path that fills it still works.
            .and_then(|c| c.custom_color.as_deref().or(c.color.as_deref()))
            .and_then(crate::widgets::parse_hex_color)
            // Auto mode (no custom color): fall back to the detected
            // OS brand color so an Ubuntu tab "breathes" orange,
            // matching the OS badge glyph and the dashboard card.
            // Mirrors the SFTP tab above.
            .or_else(|| {
                detected_os.as_deref().map(|os| {
                    crate::os_icon::resolve_icon(Some(os), OryxisColors::t().accent).1
                })
            })
            // Cloud-transport tabs (`ECS · ...`, `SSM · ...`,
            // `K8s · ...`) don't match any saved Connection by
            // label, so the per-host color lookup above returns
            // None and the active-tab gradient falls back to the
            // global accent. Derive a brand-coloured accent from
            // the tab label prefix instead so the tab "breathes"
            // the parent dynamic-group color (AWS orange / K8s
            // blue / etc.) the same way a per-host accent does.
            .or_else(|| {
                crate::os_icon::tab_label_cloud_brand(base_label).map(|brand| {
                    crate::os_icon::provider_icon(brand, OryxisColors::t().accent).1
                })
            }) };
        // Tabs always render the badge as a rounded square,
        // independent of the per-host override and the global
        // `default_host_icon` setting. Circular badges read as
        // pills inside the narrow tab strip and the variable
        // shape disrupts the row's vertical rhythm; locking the
        // tab shape keeps the strip uniform while leaving the
        // dashboard card free to honour the user's choice.
        let host_icon_style = crate::widgets::HostIconStyle::Rounded;
        // Connection-state dot color, from the same `tab_conn_state`
        // the status bar reads, so the dot and the bar can never
        // disagree about one tab. Local-shell tabs and dormant pinned
        // placeholders get no dot: the OS badge already says what they
        // are. This used to test `session.is_some()`, which left a
        // split tab's dead focused pane green (only single-pane tabs
        // get relabeled "(disconnected)") and left a live cloud tab
        // dotless (its transport is a plugin process, not a handle).
        // A mosh session out of touch shares the amber of a dial in
        // flight, and means the same thing to the person looking: it is
        // working on it, and it is not there yet. Amber rather than red
        // because the session is not gone, and rather than green because
        // it is what the strip has to say at the one moment mosh earns
        // its keep. The mapping itself is `TabConnState::dot_color`,
        // shared with the per-pane header so the two cannot drift.
        let status_dot: Option<Color> = self
            .vaults.cur().prefs
            .show_tab_status_dot
            .then(|| self.tab_conn_state(idx).dot_color())
            .flatten();
        // Smart-tabs attention dot (top-right corner of the badge):
        // the highest-priority cause across the tab's panes. Viewing
        // the tab clears the state, so an active watched tab never
        // carries one. A background tab left armed for broadcast (C2)
        // takes the slot first (warning-tinted) so its armed state is
        // visible from the strip, not just once you switch back to it.
        let attention_dot: Option<Color> = if tab.broadcast && !is_active {
            Some(OryxisColors::t().warning)
        } else if self.vaults.cur().prefs.smart_tabs {
            tab.pane_grid
                .panes
                .values()
                .filter_map(|p| p.attention)
                .max()
                .map(|a| match a {
                    crate::smart_tabs::TabAttention::Activity => {
                        OryxisColors::t().accent
                    }
                    crate::smart_tabs::TabAttention::FinishedOk => {
                        OryxisColors::t().success
                    }
                    crate::smart_tabs::TabAttention::FinishedFail => {
                        OryxisColors::t().error
                    }
                })
        } else {
            None
        };
        // Running-command indicator (issue #146): some pane of this tab
        // has a command running PAST the smart-tabs long-command
        // threshold, so the strip says the host is busy while it still
        // is (the attention dot only speaks after the fact). Same gates
        // as the finished-command notification (toggle + threshold);
        // the frame advances on `BusyAnimTick`, whose subscription only
        // exists while a command is in flight.
        let busy_frame: Option<u8> = (self.vaults.cur().prefs.smart_tabs
            && self.vaults.cur().prefs.smart_long_secs > 0
            && tab.pane_grid.panes.values().any(|p| {
                p.running_cmd.as_ref().is_some_and(|run| {
                    run.started.elapsed().as_secs()
                        >= u64::from(self.vaults.cur().prefs.smart_long_secs)
                })
            }))
        .then_some((self.busy_anim_tick % 3) as u8);
        // Session-group tabs carry the group's own icon + color.
        let session_group = tab
            .session_group_id
            .and_then(|id| self.vaults.cur().session_groups.iter().find(|g| g.id == id));
        let sg_custom_color = session_group
            .and_then(|g| g.color.as_deref())
            .and_then(crate::widgets::parse_hex_color);
        let sg_custom_icon = session_group
            .and_then(|g| g.icon_style.as_deref())
            .filter(|s| !s.is_empty());
        // Local-terminal appearance override: a curated entry (matched
        // by label) can carry an explicit icon / color chosen in the
        // Settings card, which wins over the OS hint so the tab chip
        // reflects what the user picked. Session-group icon/color still
        // take precedence (a grouped tab is the group's identity).
        let is_local_pane =
            matches!(tab.active().origin, crate::state::PaneOrigin::Local(_));
        let lt_entry = if is_local_pane {
            self.local_terminals
                .as_deref()
                .and_then(|list| list.iter().find(|e| e.label == base_label))
        } else {
            None
        };
        // Second line of the tab: the connection address, in the SAME
        // form the host cards use (`host_address_label`: default port
        // omitted, serial lines as `port @ baud`) and behind the same
        // off-by-default setting, so addresses stay out of screenshots
        // unless the user asks for them.
        //
        // Local shells and ephemeral cloud tabs have no saved
        // connection, so they simply have no address; a renamed tab
        // keeps its rename alone (the user replaced the identity).
        // Resolved through the pane's ORIGIN (id-based), so an OSC
        // title change cannot repoint it at another host.
        //
        // Privacy Mode masks it in blocks and reveals on hover, exactly
        // like the card address, rather than redacting it as a label.
        let tab_address: Option<String> = if self.vaults.cur().prefs.show_tab_host_address
            && tab.custom_name.is_none()
        {
            self.pane_origin_connection(tab.active().id).map(|c| {
                let address = self.host_address_label(c);
                if self.privacy_active(c) && !is_hovered {
                    crate::widgets::mask_blocks(&address)
                } else {
                    address
                }
            })
        } else {
            None
        };
        let lt_icon = lt_entry.and_then(|e| e.icon.as_deref());
        let lt_color = lt_entry
            .and_then(|e| e.color.as_deref())
            .and_then(crate::widgets::parse_hex_color);
        let tab_icon = sg_custom_icon.or(lt_icon);
        let tab_badge_color = sg_custom_color.or(lt_color);
        let tab_accent = self
            .host_accent_enabled()
            .then(|| sg_custom_color.or(lt_color).or(host_accent))
            .flatten();
        // Mode glyph (issue #61): the chip (>_ terminal / console /
        // folder files) only exists once the tab has a SECOND surface
        // to switch to (owner QA 2026-07-05: a plain SSH tab shows no
        // glyph; the tab menu's "Open SFTP session" creates one and the
        // switch appears, and so does opening an SFTP console). An
        // in-Files-mode tab always keeps it (the way back), even after
        // a disconnect or feature toggle.
        let mode = self
            .tab_next_surface(idx)
            .map(|next| (self.tab_surface(idx), next));
        if is_dragging {
            // The dragged tab floats as a ghost following the cursor;
            // leave a gap the size of its chip here that the other tabs
            // slide around as the reorder happens. The chip renders at
            // `TAB_ROW_HEIGHT` (its button's padding included): a gap of
            // the content box made every row under it in a side strip
            // jump 10 px up on pickup.
            let gap_w = if tab.pinned && ctx.compact_pins {
                pinned_chip_width(mode)
            } else {
                width
            };
            Space::new()
                .width(gap_w)
                .height(TAB_ROW_HEIGHT)
                .boxed()
        } else if tab.pinned && ctx.compact_pins {
            // Chrome-style: icon-only chip, fixed width, stuck left.
            // A hybrid chip widens to carry the mode glyph (owner QA:
            // the pinned form must not lose the toggle).
            pinned_tab_chip(
                idx,
                detected_os.as_deref(),
                is_active,
                tab_accent,
                host_icon_style,
                tab_icon,
                tab_badge_color,
                status_dot,
                attention_dot,
                busy_frame,
                self.vaults.cur().prefs.tab_accent_text,
                ctx.solid_fill,
                mode,
                number,
            )
        } else {
            // An in-flight ZMODEM transfer (any pane of the tab)
            // borrows the OSC 9;4 progress border, so a transfer
            // in a background tab or unfocused split stays visible
            // from the strip; the overlay only covers the active
            // pane. The divert suspends OSC progress anyway, so
            // the transfer owning the slot loses nothing.
            let zmodem_progress = tab
                .pane_grid
                .panes
                .values()
                .filter_map(|p| p.zmodem.as_ref())
                .filter_map(|zm| {
                    zm.total.filter(|t| *t > 0).map(|total| {
                        let pct = (zm.transferred as f64 / total as f64) * 100.0;
                        oryxis_terminal::Progress {
                            state: 1,
                            value: pct.clamp(0.0, 100.0) as u8,
                        }
                    })
                })
                .next();
            // A Files-mode transfer borrows the same border, for the same
            // reason: a 3 GB download the user walked away from has to be
            // visible from the strip, not only from the tab running it.
            // A sidebar browser's transfer counts too, and it is scanned
            // per pane, because that is where its slot lives.
            let sftp_progress = Self::transfer_border(self.hybrid_tab_slot(tab)).or_else(|| {
                tab.pane_grid
                    .panes
                    .values()
                    .find_map(|p| Self::transfer_border(&p.files.transfer))
            });
            session_tab(
                idx,
                &display_label,
                tab.pane_count(),
                is_active,
                is_hovered && ctx.close_armed,
                detected_os.as_deref(),
                width,
                ctx.close_on_right,
                status_dot,
                attention_dot,
                busy_frame,
                tab_accent,
                self.vaults.cur().prefs.tab_accent_text,
                host_icon_style,
                tab_icon,
                tab_badge_color,
                tab.pinned,
                ctx.solid_fill,
                // A transfer the user started wins over the shell's own
                // OSC 9;4 report: it is the thing they are waiting on.
                zmodem_progress.or(sftp_progress).or(tab.active().progress),
                mode,
                tab_address,
                number,
            )
        }
    }

    /// One entry of the COLLAPSED side strip: an icon-only rail button
    /// (the tab's host/OS badge, or the panel/SFTP glyph) wrapped in a
    /// tooltip naming it, carrying the same activation message the full
    /// chip uses. The full-chip derivation (privacy labels, drag gaps,
    /// close X, status dots, accents) is deliberately NOT reused here:
    /// the rail is the minimal glance surface, so it shows just the
    /// badge + an active wash, mirroring the vault nav rail's collapsed
    /// form. Width is fixed at `SIDEBAR_WIDTH_COLLAPSED` so the rail
    /// lines up with the nav rail.
    pub(crate) fn side_rail_item(&self, entry: StripEntry) -> Element<'_, Message> {
        let colors = OryxisColors::t();
        // Resolve (badge color, glyph element, tooltip label, active,
        // on-press) per entry kind, reusing the same icon derivation the
        // full chips use so a host's brand icon/colour is identical in
        // both forms.
        let (badge_color, glyph, label, is_active, on_press): (
            Color,
            Element<'_, Message>,
            String,
            bool,
            Message,
        ) = match entry {
            StripEntry::Panel(kind) => {
                let is_active =
                    self.cur_active_tab().is_none() && self.cur_view() == kind.view();
                (
                    colors.accent,
                    panel_icon(kind).size(12).color(Color::WHITE).boxed(),
                    self.panel_tab_label(kind),
                    is_active,
                    Message::Navigation(NavigationMessage::ChangeView(kind.view())),
                )
            }
            StripEntry::Sftp(idx) => {
                let tab = &self.sftp_tabs[idx];
                let is_active = self.cur_active_tab().is_none()
                    && self.cur_view() == View::Sftp
                    && self.cur_active_sftp() == Some(idx);
                let brand = self
                    .vaults
                    .cur()
                    .connections
                    .iter()
                    .find(|c| c.label == tab.label)
                    .and_then(|c| c.custom_color.as_deref().or(c.color.as_deref()))
                    .and_then(crate::widgets::parse_hex_color)
                    .unwrap_or(colors.accent);
                (
                    brand,
                    iced_fonts::lucide::folder_tree().size(12).color(Color::WHITE).boxed(),
                    tab.display_label().to_string(),
                    is_active,
                    Message::Sftp(SftpMessage::SelectSftpTab(idx)),
                )
            }
            StripEntry::Terminal(idx) => {
                let tab = &self.tabs[idx];
                let is_active = self.cur_active_tab() == Some(idx);
                let base_label = tab
                    .auto_label(self.tab_auto_title(tab))
                    .trim_end_matches(" (disconnected)");
                let detected_os = self.tab_detected_os(base_label);
                let (brand, brand_color) =
                    crate::os_icon::resolve_icon(detected_os.as_deref(), colors.accent);
                (
                    brand_color,
                    brand.view(12.0, Color::WHITE),
                    tab.display_label(self.tab_auto_title(tab)).to_string(),
                    is_active,
                    Message::Tabs(TabsMessage::TabChipClicked(idx)),
                )
            }
        };

        let badge = crate::widgets::host_icon(
            crate::widgets::HostIconStyle::Rounded,
            badge_color,
            &label,
            Some(glyph),
            TAB_ICON_SLOT,
        );
        let btn = button(container(badge).center(Length::Fixed(40.0)))
            .on_press(on_press)
            .padding(0)
            .width(Length::Fixed(40.0))
            .style(move |_, status| {
                let accent = OryxisColors::t().accent;
                let bg = match status {
                    BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                    BtnStatus::Pressed => Color { a: 0.25, ..accent },
                    _ if is_active => Color { a: 0.15, ..accent },
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }
            });
        let centered = container(btn)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 });
        // The frame-local label can't be borrowed, so the owned tooltip
        // variant is the one that outlives this build.
        crate::views::terminal::icon_tooltip_owned(centered.boxed(), label)
    }

    /// Floating ghost of the tab being dragged (terminal or SFTP),
    /// plus its width. `None` when no drag is active. The caller
    /// positions it over its strip: the horizontal bars track the
    /// cursor's x, the vertical strip its y.
    ///
    /// The ghost is as wide as the gap the tab leaves in this strip,
    /// read from where that gap was last drawn, so one never differs
    /// from the other (a shrunk overflowing strip, the side strip's
    /// rows, a hybrid pin). Before the gap has been drawn once (a tab
    /// carried in from another window), `slot_w` (the side strip's row
    /// width) or the tab's own allocation stands in.
    pub(crate) fn strip_drag_ghost_el(
        &self,
        slot_w: Option<f32>,
        compact_pins: bool,
        privacy_terms: &[String],
    ) -> Option<(Element<'_, Message>, f32)> {
        let close_on_right = self.vaults.cur().prefs.tab_close_button_side == "right";
        let number_px = self.tab_number_px();
        let dragged = self.strip_drag_id()?;
        let drawn_w = self
            .strip_chip_rect(self.cur_window(), dragged)
            .map(|rect| rect.width);
        let width_or = |compact: bool, own: &dyn Fn() -> f32| -> f32 {
            drawn_w.unwrap_or_else(|| {
                if compact {
                    CHIP_W
                } else {
                    slot_w.unwrap_or_else(own)
                }
            })
        };
        if let Some(tab) = self.tabs.iter().find(|t| t._id == dragged) {
            let idx = self.tabs.iter().position(|t| t._id == dragged);
            let lookup_label = tab
                .auto_label(self.tab_auto_title(tab))
                .trim_end_matches(" (disconnected)")
                .to_string();
            // The ghost renders the same redacted label as the tab it
            // mirrors (issue #78); mid-drag there is no hover reveal.
            let base_label = self.privacy_display_label(
                &lookup_label,
                tab.display_label(self.tab_auto_title(tab))
                    .trim_end_matches(" (disconnected)"),
                privacy_terms,
            );
            let detected_os = self.tab_detected_os(&lookup_label);
            let compact = tab.pinned && compact_pins;
            let session_group = tab
                .session_group_id
                .and_then(|id| self.vaults.cur().session_groups.iter().find(|g| g.id == id));
            let sg_color = session_group
                .and_then(|g| g.color.as_deref())
                .and_then(crate::widgets::parse_hex_color);
            let sg_icon = session_group
                .and_then(|g| g.icon_style.as_deref())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let accent = sg_color
                .filter(|_| self.host_accent_enabled())
                .unwrap_or_else(|| OryxisColors::t().accent);
            let ghost_w = width_or(compact, &|| {
                if self.cur_active_tab() == idx {
                    TAB_NATURAL_WIDTH
                } else {
                    tab_content_width(&base_label, close_on_right, tab.pane_count() > 1, number_px)
                }
            });
            Some((
                drag_ghost(base_label, detected_os, compact, ghost_w, accent, self.vaults.cur().prefs.tab_accent_text, sg_icon, sg_color),
                ghost_w,
            ))
        } else if let Some(sftp_tab) = self.sftp_tabs.iter().find(|t| t.id == dragged) {
            let idx = self.sftp_tabs.iter().position(|t| t.id == dragged);
            let detected_os = self.tab_detected_os(&sftp_tab.label);
            let brand = self.vaults.cur().connections
                .iter()
                .find(|c| c.label == sftp_tab.label)
                .and_then(|c| c.custom_color.as_deref().or(c.color.as_deref()))
                .and_then(crate::widgets::parse_hex_color)
                .or_else(|| {
                    detected_os.as_deref().map(|os| {
                        crate::os_icon::resolve_icon(Some(os), OryxisColors::t().accent).1
                    })
                });
            // Badge keeps the brand ungated; the label/outline accent
            // honours `tab_accent_color`.
            let badge_accent = brand.unwrap_or_else(|| OryxisColors::t().accent);
            let accent = self
                .host_accent_enabled()
                .then_some(brand)
                .flatten()
                .unwrap_or_else(|| OryxisColors::t().accent);
            let compact = sftp_tab.pinned && compact_pins;
            let is_active = self.cur_active_tab().is_none()
                && self.cur_view() == View::Sftp
                && self.cur_active_sftp() == idx;
            let ghost_w = width_or(compact, &|| {
                if is_active {
                    TAB_NATURAL_WIDTH
                } else {
                    tab_content_width(
                        &self.privacy_display_label(
                            &sftp_tab.label,
                            sftp_tab.display_label(),
                            privacy_terms,
                        ),
                        close_on_right,
                        false,
                        number_px,
                    )
                }
            });
            Some((
                sftp_drag_ghost(
                    self.privacy_display_label(
                        &sftp_tab.label,
                        sftp_tab.display_label(),
                        privacy_terms,
                    ),
                    compact,
                    ghost_w,
                    badge_accent,
                    accent,
                    self.vaults.cur().prefs.tab_accent_text,
                ),
                ghost_w,
            ))
        } else {
            // Its own ghost rather than `drag_ghost`: that one derives an
            // OS badge from a host label, and a panel has no host. The
            // panel glyph + app accent is the same vocabulary as the chip
            // being dragged.
            crate::state::PanelKind::ALL
                .into_iter()
                .find(|k| k.tab_id() == dragged)
                .map(|kind| {
                    let label = self.panel_tab_label(kind);
                    let is_active =
                        self.cur_active_tab().is_none() && self.cur_view() == kind.view();
                    let ghost_w = width_or(false, &|| {
                        if is_active {
                            TAB_NATURAL_WIDTH
                        } else {
                            panel_tab_width(&label, number_px)
                        }
                    });
                    (panel_drag_ghost(kind, &label, ghost_w), ghost_w)
                })
        }
    }
}
