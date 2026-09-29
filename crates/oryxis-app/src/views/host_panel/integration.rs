//! Host editor: the SSH Integration rows (expose-to-MCP toggle,
//! remote-desktop block, environment variables, initial command).
use super::*;
use iced::widget::column;

impl Oryxis {
    pub(super) fn hp_row_mcp(&self, is_ssh: bool) -> Element<'_, Message> {
        // Expose to MCP / AI (SSH > Integration).
        let row_mcp: Element<'_, Message> = if is_ssh {
            self.panel_nav_slot(
            crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorToggleMcpEnabled)),
            8.0,
            container(
                dir_row(vec![
                    iced_fonts::lucide::plug().size(14).color(OryxisColors::t().text_muted).into(),
                    Space::new().width(10).into(),
                    text(t("expose_to_mcp")).size(13).color(OryxisColors::t().text_secondary).into(),
                    Space::new().width(Length::Fill).into(),
                    {
                        let on = self.editor_form.mcp_enabled;
                        let bg = if on { OryxisColors::t().success } else { OryxisColors::t().bg_hover };
                        let fg = crate::theme::contrast_text_for(bg);
                        button(text(if on { crate::i18n::t("toggle_on") } else { crate::i18n::t("toggle_off") }).size(12).color(fg))
                            .on_press(Message::Editor(EditorMessage::EditorToggleMcpEnabled))
                            .style(move |_theme, _status| button::Style {
                                background: Some(Background::Color(bg)),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                text_color: fg,
                                ..Default::default()
                            })
                            .into()
                    },
                ]).align_y(iced::Alignment::Center)
            )
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 }).into(),
            )
        } else {
            empty()
        };
        row_mcp
    }

    /// Directory a fresh SFTP mount of this host lands in (SSH >
    /// Integration). Empty placeholder spells out the default (the login
    /// directory) so "inherit" is visible rather than implied; a path that
    /// no longer resolves falls back to it at mount time.
    pub(super) fn hp_row_sftp_initial_path(&self, is_ssh: bool) -> Element<'_, Message> {
        if !is_ssh {
            return empty();
        }
        // Stacked, unlike the sibling toggle rows: a remote path is long and
        // an inline field would squeeze the label into two lines and still
        // clip the value.
        container(
            column![
                dir_row(vec![
                    iced_fonts::lucide::folder_open()
                        .size(14)
                        .color(OryxisColors::t().text_muted)
                        .into(),
                    Space::new().width(10).into(),
                    text(t("host_sftp_initial_path"))
                        .size(13)
                        .color(OryxisColors::t().text_secondary)
                        .into(),
                ])
                .align_y(iced::Alignment::Center),
                Space::new().height(2),
                text(t("host_sftp_initial_path_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(6),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-sftp-initial-path")),
                    10.0,
                    text_input(
                        t("host_sftp_initial_path_placeholder"),
                        &self.editor_form.sftp_initial_path,
                    )
                    .id(iced::widget::Id::new("editor-sftp-initial-path"))
                    .on_input(|v| {
                        Message::Editor(EditorMessage::EditorSftpInitialPathChanged(v))
                    })
                    .on_submit_maybe(self.hp_submit())
                    .padding(6)
                    .size(13)
                    .width(Length::Fill)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
                ),
            ]
            .width(Length::Fill),
        )
        .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 })
        .into()
    }

    /// Per-host drop-transport flag (SSH > Integration): drag-and-drop
    /// uploads ride ZMODEM (`rz` typed into the shell) instead of SFTP,
    /// for hosts whose interactive shell runs inside a container. SFTP
    /// always reaches the host filesystem as sshd sees it; the `rz` the
    /// app types runs where the shell runs and lands in the container's
    /// own working directory. Off keeps the standard drop routing.
    ///
    /// Shaped like the mosh block (`panel_option_row` + the note under
    /// it) rather than the sibling one-liners: the note runs to several
    /// lines, and nesting it INSIDE the row centers the icon and the
    /// pill against the whole block, which leaves the icon beside the
    /// middle of the text with no label next to it.
    pub(super) fn hp_row_zmodem_drops(&self, is_ssh: bool) -> Element<'_, Message> {
        if !is_ssh {
            return empty();
        }
        let msg = Message::Editor(EditorMessage::EditorToggleZmodemDrops);
        let row = self.panel_nav_slot(
            crate::keynav::RowAction::activate(msg.clone()),
            8.0,
            panel_option_row(
                iced_fonts::lucide::upload(),
                t("host_zmodem_drops"),
                super::basics::hp_toggle_button(self.editor_form.zmodem_drops, msg),
            ),
        );
        container(
            column![
                row,
                text(t("host_zmodem_drops_desc")).size(11).color(OryxisColors::t().text_muted),
            ]
            .width(Length::Fill),
        )
        // `panel_option_row` carries 4 of its own, so the row keeps the
        // 8px the sibling toggles sit on.
        .padding(Padding { top: 4.0, right: 0.0, bottom: 8.0, left: 0.0 })
        .into()
    }

    /// Per-host agentless monitoring opt-in (SSH > Integration, issue
    /// #83). Same shape as the MCP row: SSH-only, since the probe reads
    /// `/proc` over an exec channel.
    pub(super) fn hp_row_monitor(&self, is_ssh: bool) -> Element<'_, Message> {
        // The per-host opt-in only exists once the monitoring feature is
        // enabled in Features & Plugins.
        if !is_ssh || !self.prefs.host_monitoring {
            return empty();
        }
        // "Enable for all hosts" forces this host on: the toggle renders
        // locked (no press, muted "On", no keyboard slot) with a hint that
        // it is controlled globally, so the state is honest but the user
        // isn't led to click a control that does nothing.
        let locked = self.prefs.monitor_all_hosts;
        let on = locked || self.editor_form.monitor_enabled;

        let toggle: Element<'_, Message> = {
            let bg = if on {
                OryxisColors::t().success
            } else {
                OryxisColors::t().bg_hover
            };
            let fg = crate::theme::contrast_text_for(bg);
            let label = text(if on {
                crate::i18n::t("toggle_on")
            } else {
                crate::i18n::t("toggle_off")
            })
            .size(12)
            .color(if locked { crate::theme::mix(fg, bg, 0.35) } else { fg });
            let mut btn = button(label)
                .style(move |_theme, _status| {
                    // A locked toggle desaturates toward the panel so it
                    // reads as "not editable here".
                    let shown = if locked { Color { a: 0.5, ..bg } } else { bg };
                    button::Style {
                        background: Some(Background::Color(shown)),
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        text_color: fg,
                        ..Default::default()
                    }
                })
                .padding(Padding { top: 3.0, right: 10.0, bottom: 3.0, left: 10.0 });
            if !locked {
                btn = btn.on_press(Message::Editor(EditorMessage::EditorToggleMonitorEnabled));
            }
            btn.into()
        };

        let label_text = if locked {
            crate::i18n::t("monitor_enable_host_all")
        } else {
            crate::i18n::t("monitor_enable_host")
        };
        let row = container(
            dir_row(vec![
                iced_fonts::lucide::activity()
                    .size(14)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().width(10).into(),
                text(label_text)
                    .size(13)
                    .color(OryxisColors::t().text_secondary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                toggle,
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 });

        // A locked row is not a keyboard stop (nothing to activate);
        // otherwise it records as usual.
        if locked {
            row.into()
        } else {
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(
                    EditorMessage::EditorToggleMonitorEnabled,
                )),
                8.0,
                row.into(),
            )
        }
    }

    /// Which mounts the monitor reports for this host (issue #135):
    /// Auto (the probe's own rules keep one row per storage device) or
    /// Custom, a list of mount patterns.
    ///
    /// Only rendered while this host is actually monitored, so a host
    /// with the feature off doesn't carry a setting that does nothing.
    /// The selection is still SAVED when the row is hidden (unlike the
    /// SSH-only clamps on `mcp_enabled` / `monitor_enabled`, which are
    /// live flags): it is stored configuration, and turning monitoring
    /// off and on again must bring back the list the user typed rather
    /// than silently resetting the host to Auto.
    pub(super) fn hp_row_monitor_disks(&self, is_ssh: bool) -> Element<'_, Message> {
        let monitored = self.prefs.monitor_all_hosts || self.editor_form.monitor_enabled;
        if !is_ssh || !self.prefs.host_monitoring || !monitored {
            return empty();
        }
        let custom = self.editor_form.monitor_disks_custom;
        let custom_label = t("monitor_disks_custom").to_string();
        let selected = if custom { custom_label.clone() } else { t("monitor_disks_auto").to_string() };
        let options =
            vec![t("monitor_disks_auto").to_string(), t("monitor_disks_custom").to_string()];
        // The picker carries the CHOICE, not the label: comparing the
        // selection against the Custom string happens here, where the
        // string was built, so the message stays a bool and a language
        // switch can't reach the dispatcher.
        let picker = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-monitor-disks")),
            crate::widgets::INPUT_RADIUS,
            pick_list(Some(selected), options, |s: &String| s.clone())
                .on_select(move |v| {
                    Message::Editor(EditorMessage::EditorMonitorDisksCustom(v == custom_label))
                })
                .id(iced::widget::Id::new("editor-pick-monitor-disks"))
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(120)
                .padding(10)
                .text_size(13)
                .style(crate::widgets::rounded_pick_list_style)
                .into(),
        );
        // The "+" only exists under Custom: adding a row while the host
        // is on Auto would build a list nothing reads.
        let add: Element<'_, Message> = if custom {
            dir_row(vec![
                Space::new().width(8).into(),
                self.panel_nav_slot(
                    crate::keynav::RowAction::activate(Message::Editor(
                        EditorMessage::EditorAddMonitorDisk,
                    )),
                    4.0,
                    button(text("+").size(14).color(OryxisColors::t().text_primary))
                        .on_press(Message::Editor(EditorMessage::EditorAddMonitorDisk))
                        .style(|_, status| button::Style {
                            background: Some(Background::Color(match status {
                                button::Status::Hovered | button::Status::Pressed => {
                                    OryxisColors::t().bg_selected
                                }
                                _ => OryxisColors::t().bg_hover,
                            })),
                            border: Border { radius: Radius::from(4.0), ..Default::default() },
                            text_color: OryxisColors::t().text_primary,
                            ..Default::default()
                        })
                        .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
                        .into(),
                ),
            ])
            .into()
        } else {
            empty()
        };

        let mut block = column![
            dir_row(vec![
                iced_fonts::lucide::hard_drive()
                    .size(14)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().width(10).into(),
                column![
                    text(t("monitor_disks")).size(13).color(OryxisColors::t().text_secondary),
                    Space::new().height(2),
                    text(t("monitor_disks_desc")).size(11).color(OryxisColors::t().text_muted),
                ]
                .width(Length::Fill)
                .into(),
                Space::new().width(8).into(),
                picker,
                add,
            ])
            .align_y(iced::Alignment::Center),
        ];

        if custom {
            // Same static-id limitation as the env-var / port-forward
            // rows: the pattern inputs stay mouse-only and the remove
            // button is the row's keyboard stop.
            for (i, mount) in self.editor_form.monitor_disks.iter().enumerate() {
                let idx = i;
                block = block.push(Space::new().height(8));
                block = block.push(
                    dir_row(vec![
                        text_input("/data", mount)
                            .on_input(move |v| {
                                Message::Editor(EditorMessage::EditorMonitorDiskChanged(idx, v))
                            })
                            .padding(6)
                            .width(Length::Fill)
                            .size(13)
                            .style(crate::widgets::rounded_input_style)
                            .align_x(dir_align_x())
                            .into(),
                        self.panel_nav_slot(
                            crate::keynav::RowAction::activate(Message::Editor(
                                EditorMessage::EditorRemoveMonitorDisk(idx),
                            )),
                            4.0,
                            button(text("\u{00D7}").size(11).color(OryxisColors::t().error))
                                .on_press(Message::Editor(EditorMessage::EditorRemoveMonitorDisk(
                                    idx,
                                )))
                                .style(|_, status| button::Style {
                                    background: match status {
                                        button::Status::Hovered | button::Status::Pressed => {
                                            Some(Background::Color(OryxisColors::t().bg_hover))
                                        }
                                        _ => None,
                                    },
                                    border: Border {
                                        radius: Radius::from(4.0),
                                        ..Default::default()
                                    },
                                    text_color: OryxisColors::t().error,
                                    ..Default::default()
                                })
                                .padding(Padding { top: 2.0, right: 4.0, bottom: 2.0, left: 4.0 })
                                .into(),
                        ),
                    ])
                    .align_y(iced::Alignment::Center)
                    .spacing(4),
                );
            }
        }

        container(block)
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 })
            .into()
    }

    pub(super) fn hp_rd_block(&self, is_rd: bool) -> Element<'_, Message> {
        use oryxis_core::models::connection::ConnectionProtocol as Proto;
        // Remote-desktop rows (RemoteDesktop hosts only): the kind picker
        // (RDP/VNC) and the SSH gateway to tunnel through (or Direct). The
        // endpoint (host/port) and login (username/password) reuse the
        // shared fields above.
        let rd_block: Element<'_, Message> = if is_rd {
            use oryxis_core::models::remote_desktop::RemoteDesktopKind;
            let kind_row = panel_option_row(
                iced_fonts::lucide::monitor_smartphone(),
                t("remote_desktop_kind"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-rd-kind")),
                    crate::widgets::INPUT_RADIUS,
                    pick_list(
                        Some(self.editor_form.rd_kind),
                        vec![RemoteDesktopKind::Rdp, RemoteDesktopKind::Vnc],
                        |k: &RemoteDesktopKind| k.to_string(),
                    )
                    .on_select(|v| Message::Editor(EditorMessage::EditorRdKindChanged(v)))
                    .id(iced::widget::Id::new("editor-pick-rd-kind"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(120)
                    .padding(10)
                    .text_size(13)
                    .style(crate::widgets::rounded_pick_list_style)
                    .into(),
                ),
            );
            // Gateway: `None` = Direct, else an SSH host to tunnel through.
            let gw_options: Vec<Option<uuid::Uuid>> = std::iter::once(None)
                .chain(
                    self.connections
                        .iter()
                        .filter(|c| c.protocol == Proto::Ssh)
                        .map(|c| Some(c.id)),
                )
                .collect();
            let gw_labels: std::collections::HashMap<Option<uuid::Uuid>, String> =
                std::iter::once((None, t("remote_desktop_direct").to_string()))
                    .chain(
                        self.connections
                            .iter()
                            .filter(|c| c.protocol == Proto::Ssh)
                            .map(|c| (Some(c.id), c.label.clone())),
                    )
                    .collect();
            let gw_row = panel_option_row(
                iced_fonts::lucide::route(),
                t("remote_desktop_gateway"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-rd-gateway")),
                    crate::widgets::INPUT_RADIUS,
                    pick_list(
                        Some(self.editor_form.rd_gateway_id),
                        gw_options,
                        move |id: &Option<uuid::Uuid>| {
                            gw_labels.get(id).cloned().unwrap_or_default()
                        },
                    )
                    .on_select(|v| Message::Editor(EditorMessage::EditorRdGatewayChanged(v)))
                    .id(iced::widget::Id::new("editor-pick-rd-gateway"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(200)
                    .padding(10)
                    .text_size(13)
                    .style(crate::widgets::rounded_pick_list_style)
                    .into(),
                ),
            );
            // Wake-on-LAN MAC: waking the workstation before an RDP/VNC
            // connect is the classic use, so RD hosts get the row too
            // (built here so it keynav-records inside the RD block).
            let mac_row = self.hp_row_mac_address(true);
            column![
                kind_row,
                Space::new().height(ROW_GAP),
                gw_row,
                Space::new().height(ROW_GAP),
                mac_row
            ]
            .into()
        } else {
            empty()
        };
        rd_block
    }

    pub(super) fn hp_env_items(&self, is_ssh: bool) -> Element<'_, Message> {
        // ── Section: Environment Variables ──
        let env_items: Element<'_, Message> = if is_ssh {
        let mut env_items = column![
            dir_row(vec![
                iced_fonts::lucide::variable().size(14).color(OryxisColors::t().text_muted).into(),
                Space::new().width(10).into(),
                column![
                    text(t("env_vars")).size(13).color(OryxisColors::t().text_secondary),
                    Space::new().height(2),
                    text(t("env_vars_desc")).size(11).color(OryxisColors::t().text_muted),
                ].width(Length::Fill).into(),
                Space::new().width(8).into(),
                self.panel_nav_slot(
                    crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorAddEnvVar)),
                    4.0,
                    button(text("+").size(14).color(OryxisColors::t().text_primary))
                        .on_press(Message::Editor(EditorMessage::EditorAddEnvVar))
                        .style(|_, _| button::Style {
                            background: Some(Background::Color(OryxisColors::t().bg_hover)),
                            border: Border { radius: Radius::from(4.0), ..Default::default() },
                            text_color: OryxisColors::t().text_primary,
                            ..Default::default()
                        })
                        .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
                        .into(),
                ),
            ]).align_y(iced::Alignment::Center),
        ];

        for (i, e) in self.editor_form.env_vars.iter().enumerate() {
            let idx = i;
            env_items = env_items.push(Space::new().height(8));
            // Same static-id limitation as the port-forward rows: the
            // key/value inputs stay mouse-only, the remove button is
            // the keyboard row.
            env_items = env_items.push(
                dir_row(vec![
                    text_input("LC_EXAMPLE", &e.key)
                        .on_input(move |v| Message::Editor(EditorMessage::EditorEnvVarKeyChanged(idx, v)))
                        .padding(6)
                        .width(Length::FillPortion(2))
                        .size(13)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into(),
                    text("=").size(12).color(OryxisColors::t().text_muted).into(),
                    text_input(crate::i18n::t("env_value_placeholder"), &e.value)
                        .on_input(move |v| Message::Editor(EditorMessage::EditorEnvVarValueChanged(idx, v)))
                        .padding(6)
                        .width(Length::FillPortion(3))
                        .size(13)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into(),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorRemoveEnvVar(idx))),
                        4.0,
                        button(text("\u{00D7}").size(11).color(OryxisColors::t().error))
                            .on_press(Message::Editor(EditorMessage::EditorRemoveEnvVar(idx)))
                            .style(|_, _| button::Style {
                                background: None,
                                border: Border::default(),
                                text_color: OryxisColors::t().error,
                                ..Default::default()
                            })
                            .padding(Padding { top: 2.0, right: 4.0, bottom: 2.0, left: 4.0 })
                            .into(),
                    ),
                ]).align_y(iced::Alignment::Center).spacing(4),
            );
        }
        env_items.into()
        } else {
            empty()
        };
        env_items
    }

    pub(super) fn hp_startup_block(&self, is_ssh: bool) -> Element<'_, Message> {
        // Initial command / snippet (Terminal), sent to the shell right
        // after the session opens. Universal (keystrokes), so it lives in
        // the universal Terminal section, not the SSH block.
        // Forced-selection searchable combo: the None / Custom sentinels
        // and snippet labels (options built once in
        // `rebuild_editor_combos`). Picking commits via
        // EditorStartupChoiceChanged; typing only filters (no on_input,
        // so there is no free-text path). The current choice's label
        // seeds the selection (and doubles as the focused placeholder).
        let startup_block: Element<'_, Message> = if is_ssh {
        let startup_selected = self.editor_startup_label();
        // Keyboard row: Left/Right cycle None / Custom / snippet labels.
        let (startup_prev, startup_next) = crate::keynav::slots::cycle_pair(
            self.editor_startup_combo.options(),
            &startup_selected,
            |v| Message::Editor(EditorMessage::EditorStartupChoiceChanged(v)),
        );
        let startup_picker: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::picker(startup_prev, startup_next),
            10.0,
            iced::widget::combo_box(
                &self.editor_startup_combo,
                // Owned placeholder (frame-local String); see auth.rs.
                startup_selected.clone(),
                Some(&startup_selected),
                |v| Message::Editor(EditorMessage::EditorStartupChoiceChanged(v)),
            )
            .on_open(Message::Editor(EditorMessage::EditorStartupComboOpened))
            // Restores the committed pick on blur; see auth.rs.
            .on_close(Message::NoOp)
            .size(13)
            .padding(10)
            .input_style(crate::widgets::rounded_input_style)
            .menu_style(crate::widgets::combo_menu_style)
            .width(Length::Fill)
            .into(),
        );

        let mut startup_block = column![
            text(t("initial_command_label"))
                .size(12)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            startup_picker,
        ];
        if matches!(self.editor_startup_choice, crate::state::StartupChoice::Custom) {
            startup_block = startup_block.push(Space::new().height(8)).push(
                // Multi-line, auto-grows with content; container caps the
                // height (~8 lines) and then it scrolls internally. Supports
                // multi-command scripts (one command per line).
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-initial-command")),
                    10.0,
                    container(
                        text_editor(&self.editor_initial_command)
                            .id(iced::widget::Id::new("editor-initial-command"))
                            .placeholder(t("initial_command_ph"))
                            .on_action(|v| Message::Editor(EditorMessage::EditorInitialCommandChanged(v)))
                            .padding(10)
                            .height(Length::Shrink)
                            .style(crate::widgets::rounded_editor_style),
                    )
                    .height(Length::Shrink.max(200.0))
                    .into(),
                ),
            );
        }
        startup_block.into()
        } else {
            empty()
        };
        startup_block
    }
}
