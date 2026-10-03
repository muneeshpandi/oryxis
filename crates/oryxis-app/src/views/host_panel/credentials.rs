//! Host editor: the Credentials body (username, identity suggestions,
//! password / identity banner, TOTP) and the reduced Serial line block.
use super::*;
use iced::widget::column;

impl Oryxis {
    pub(super) fn hp_cred_items(&self, is_serial: bool, is_ssh: bool) -> Element<'_, Message> {

        // Credentials column: username, then identity suggestions, then
        // either the "managed by identity" banner is hoisted to the SSH
        // Authentication group, or the password row is appended below.
        // Gated off for Serial, which has no auth (and whose param block
        // replaces it); building it there would record dead Tab targets.
        let cred_items: Element<'_, Message> = if is_serial {
            empty()
        } else {
        // Group inheritance (D4): an empty username says what the
        // host's group chain will supply, and only while it IS empty,
        // because once the user types the inherited value is no longer
        // what connects.
        let inherited = self.editor_inherited();
        let username_hint = self
            .editor_form
            .username
            .trim()
            .is_empty()
            .then_some(inherited.username.clone())
            .flatten();
        let mut cred_items = column![
            crate::widgets::panel_field_inherited(
                "",
                dir_row(vec![
                    iced_fonts::lucide::user().size(13).color(OryxisColors::t().text_muted).boxed(),
                    Space::new().width(10).boxed(),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::input(iced::widget::Id::new("editor-username")),
                        10.0,
                        text_input(t("username"), &self.editor_form.username)
                            .id(iced::widget::Id::new("editor-username"))
                            .on_input(|v| Message::Editor(EditorMessage::EditorUsernameChanged(v)))
                            .on_submit_maybe(self.hp_submit())
                            .padding(10)
                            .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).boxed(),
                    ),
                ]).align_y(iced::Alignment::Center).boxed(),
                username_hint,
            )
        ];

        // Identity suggestion dropdown (only when username field is
        // focused). Identities are an SSH-auth concept, so the reduced
        // Telnet form never offers them.
        //
        // The rows live in ONE column that is always pushed, empty when
        // nothing is offered: the list comes and goes with typing (and
        // with the first keystroke in the password field, which is what
        // clears `username_focused`), and pushing its rows straight into
        // `cred_items` moved every row below it, so the password input
        // lost its state, focus included, on that keystroke.
        let mut suggestions = iced::widget::Column::<iced::Element<'_, _>>::new();
        if is_ssh && self.editor_form.username_focused && self.editor_form.selected_identity.is_none() && !self.identities.is_empty() {
            let search = self.editor_form.username.to_lowercase();
            let matching: Vec<&Identity> = if search.is_empty() {
                self.identities.iter().collect()
            } else {
                self.identities.iter()
                    .filter(|i| i.label.to_lowercase().contains(&search)
                        || i.username.as_deref().unwrap_or("").to_lowercase().contains(&search))
                    .collect()
            };
            if !matching.is_empty() {
                for identity in matching.iter().take(3) {
                    let label = identity.label.clone();
                    let subtitle = format!(
                        "{}{}",
                        identity.username.as_deref().unwrap_or(""),
                        if identity.key_id.is_some() {
                            let key_name = identity.key_id.and_then(|kid| {
                                self.keys.iter().find(|k| k.id == kid).map(|k| k.label.as_str())
                            }).unwrap_or("key");
                            format!(", {}", key_name)
                        } else { String::new() },
                    );
                    let ident_label = identity.label.clone();
                    suggestions = suggestions.push(self.panel_nav_slot(
                        crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorIdentityChanged(
                            ident_label.clone(),
                        ))),
                        6.0,
                        button(
                            container(
                                dir_row(vec![
                                    iced_fonts::lucide::user().size(12).color(OryxisColors::t().accent).boxed(),
                                    Space::new().width(8).boxed(),
                                    column![
                                        text(label.clone()).size(12).color(OryxisColors::t().text_primary),
                                        text(subtitle.clone()).size(10).color(OryxisColors::t().text_muted),
                                    ].boxed(),
                                ]).align_y(iced::Alignment::Center),
                            )
                            .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
                            .width(Length::Fill)
                            .style(|_| container::Style {
                                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                                border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
                                ..Default::default()
                            }),
                        )
                        .on_press(Message::Editor(EditorMessage::EditorIdentityChanged(ident_label)))
                        .width(Length::Fill)
                        .style(|_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                _ => Color::TRANSPARENT,
                            };
                            button::Style {
                                background: Some(Background::Color(bg)),
                                ..Default::default()
                            }
                        })
                        .boxed(),
                    ));
                    suggestions = suggestions.push(Space::new().height(2).boxed());
                }
            }
        }
        cred_items = cred_items.push(suggestions.boxed());

        // Identity selected -> the "managed by identity" banner replaces
        // both the password (Credentials) and the key (SSH Authentication).
        // We compute the banner / key / password as separate optionals so
        // each lands in its own section below.
        let ssh_identity_banner: Option<Element<'_, Message>> = self
            .editor_form
            .selected_identity
            .as_ref()
            .filter(|_| is_ssh)
            .map(|ident_label| {
                container(
                    dir_row(vec![
                        iced_fonts::lucide::user().size(14).color(OryxisColors::t().accent).boxed(),
                        Space::new().width(8).boxed(),
                        column![
                            text(format!("{}: {}", t("identity"), ident_label)).size(12).color(OryxisColors::t().text_primary),
                            text(t("managed_by_identity")).size(10).color(OryxisColors::t().text_muted),
                        ].boxed(),
                        Space::new().width(Length::Fill).boxed(),
                        button(text("\u{00D7}").size(11).color(OryxisColors::t().text_muted))
                            .on_press(Message::Editor(EditorMessage::EditorIdentityChanged("(none)".into())))
                            .padding(4)
                            .style(|_, _| button::Style::default()).boxed(),
                    ]).align_y(iced::Alignment::Center),
                )
                .padding(10)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color { a: 0.15, ..OryxisColors::t().accent })),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::t().accent, width: 1.0 },
                    ..Default::default()
                })
                .boxed()
            });

        // No identity on the host, but a group supplies one (D4): say so
        // where the banner would be. Muted rather than accent-filled,
        // because this is what WILL be used, not what the user chose,
        // and the two must not read the same.
        let inherited_identity_note: Option<Element<'_, Message>> = self
            .editor_form
            .selected_identity
            .is_none()
            .then_some(inherited.identity.clone())
            .flatten()
            .filter(|_| is_ssh)
            .map(|(label, group)| {
                container(
                    text(
                        crate::i18n::t("inherited_from")
                            .replace("{value}", &label)
                            .replace("{group}", &group),
                    )
                    .size(10)
                    .color(OryxisColors::t().accent),
                )
                .padding(Padding { top: 2.0, right: 0.0, bottom: 0.0, left: 0.0 })
                .boxed()
            });
        // Always one slot, empty when there is no note: the note leaves
        // with the first character typed into the password below it
        // (a typed password answers the credential family, see
        // `editor_form_answers_credentials`), and a child that came
        // and went above the password row cost that row its focus.
        cred_items = cred_items.push((match inherited_identity_note {
            Some(note) => column![note],
            None => column![],
        }).boxed());

        // Credentials body: password row when no identity, else the
        // "managed by identity" banner (both belong with the login).
        cred_items = cred_items.push(Space::new().height(8).boxed());
        if let Some(banner) = ssh_identity_banner {
            // Keyboard row: Enter/Space clears the identity (the
            // banner's only verb, same as its × button).
            cred_items = cred_items.push(self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorIdentityChanged("(none)".into()))),
                8.0,
                banner,
            ));
        } else if is_ssh && self.editor_form.auth_method == AuthMethod::PasswordPrompt {
            // "Ask every time": no password is stored, so there is no field
            // to fill. A one-line note explains the prompt-on-connect flow.
            // SSH-only (the auth-method picker that sets it is hidden on
            // Telnet, which always shows a stored-password field).
            cred_items = cred_items.push(
                dir_row(vec![
                    iced_fonts::lucide::keyboard().size(13).color(OryxisColors::t().text_muted).boxed(),
                    Space::new().width(10).boxed(),
                    text(t("auth_password_prompt_note"))
                        .size(12)
                        .color(OryxisColors::t().text_muted)
                        .boxed(),
                ]).align_y(iced::Alignment::Center).boxed()
            );
        } else {
            let pw_placeholder: &'static str = if self.editor_form.has_existing_password
                && !self.editor_form.password.touched()
            {
                "••••••••"
            } else {
                t("password")
            };
            // Keyboard rows: Tab focuses the inner input via its id;
            // the reveal eye is the next stop (recorded inside the
            // widget, hence the field row is recorded first).
            self.panel_nav_record(crate::keynav::RowAction::input(
                iced::widget::Id::new("editor-password"),
            ));
            cred_items = cred_items.push(
                dir_row(vec![
                    iced_fonts::lucide::keyboard().size(13).color(OryxisColors::t().text_muted).boxed(),
                    Space::new().width(10).boxed(),
                    crate::widgets::password_input_with_eye_nav(
                        pw_placeholder,
                        self.editor_form.password.as_str(),
                        |v| Message::Editor(EditorMessage::EditorPasswordChanged(v.into())),
                        self.hp_submit(),
                        self.editor_form.password_visible,
                        Message::Editor(EditorMessage::EditorTogglePasswordVisibility),
                        10.0,
                        Some(iced::widget::Id::new("editor-password")),
                        |eye| self.panel_nav_slot(
                            crate::keynav::RowAction::activate(
                                Message::Editor(EditorMessage::EditorTogglePasswordVisibility),
                            ),
                            6.0,
                            eye,
                        ),
                    ),
                ]).align_y(iced::Alignment::Center).boxed(),
            );
        }

        cred_items.boxed()
        };
        cred_items
    }

    /// TOTP 2FA autofill, behind a "Use TOTP" disclosure so the secret
    /// field doesn't clutter hosts without 2FA. Offered for every auth
    /// method: a keyboard-interactive second factor can follow any
    /// first factor (password, key, agent). Lives in the collapsed
    /// Authentication section (it is a second factor, not the login);
    /// Telnet has no keyboard-interactive second factor, so the reduced
    /// form never builds it. The toggle row's own vertical padding
    /// provides the same 8px rhythm as the username/password gap.
    pub(super) fn hp_totp_block(&self, is_ssh: bool) -> Element<'_, Message> {
        if !is_ssh {
            return empty();
        }
        let mut cred_items = iced::widget::Column::<iced::Element<'_, _>>::new();
        cred_items = cred_items.push(self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorUseTotpToggled)),
                8.0,
                container(
                    dir_row(vec![
                        iced_fonts::lucide::shield_check().size(13).color(OryxisColors::t().text_muted).boxed(),
                        Space::new().width(10).boxed(),
                        text(t("use_totp")).size(13).color(OryxisColors::t().text_secondary).boxed(),
                        Space::new().width(Length::Fill).boxed(),
                        {
                            let on = self.editor_form.use_totp;
                            let bg = if on { OryxisColors::t().success } else { OryxisColors::t().bg_hover };
                            let fg = crate::theme::contrast_text_for(bg);
                            button(text(if on { crate::i18n::t("toggle_on") } else { crate::i18n::t("toggle_off") }).size(12).color(fg))
                                .on_press(Message::Editor(EditorMessage::EditorUseTotpToggled))
                                .style(move |_theme, _status| button::Style {
                                    background: Some(Background::Color(bg)),
                                    border: Border { radius: Radius::from(4.0), ..Default::default() },
                                    text_color: fg,
                                    ..Default::default()
                                })
                                .boxed()
                        },
                    ]).align_y(iced::Alignment::Center)
                )
                .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 }).boxed(),
            ));
        if self.editor_form.use_totp {
            let totp_placeholder: &'static str = if self.editor_form.has_existing_totp
                && !self.editor_form.totp_secret.touched()
            {
                "••••••••"
            } else {
                t("totp_secret")
            };
            // Keyboard rows: Tab focuses the inner input via its id;
            // the reveal eye is the next stop.
            self.panel_nav_record(crate::keynav::RowAction::input(
                iced::widget::Id::new("editor-totp"),
            ));
            cred_items = cred_items.push(
                dir_row(vec![
                    Space::new().width(23).boxed(),
                    crate::widgets::password_input_with_eye_nav(
                        totp_placeholder,
                        self.editor_form.totp_secret.as_str(),
                        |v| Message::Editor(EditorMessage::EditorTotpChanged(v.into())),
                        self.hp_submit(),
                        self.editor_form.totp_visible,
                        Message::Editor(EditorMessage::EditorToggleTotpVisibility),
                        10.0,
                        Some(iced::widget::Id::new("editor-totp")),
                        |eye| self.panel_nav_slot(
                            crate::keynav::RowAction::activate(
                                Message::Editor(EditorMessage::EditorToggleTotpVisibility),
                            ),
                            6.0,
                            eye,
                        ),
                    ),
                ]).align_y(iced::Alignment::Center).boxed(),
            );
        }
        cred_items.boxed()
    }

    pub(super) fn hp_serial_params_block(&self, is_serial: bool) -> Element<'_, Message> {
        // Serial line parameters (reduced Serial form). Built here, after
        // the credentials region and before the terminal card, so the
        // keynav walk records them right after the protocol picker.
        // Gated off (empty) for non-serial protocols so they never
        // record invisible Tab targets.
        let serial_params_block: Element<'_, Message> = if is_serial {
            use oryxis_core::models::serial::{
                SerialFlowControl, SerialLineEnding, SerialParity, SerialStopBits,
                COMMON_BAUD_RATES,
            };
            let p = self.editor_form.serial.unwrap_or_default();
            let radius = crate::widgets::INPUT_RADIUS;
            // Baud: common rates (the model still accepts any u32).
            let baud_row = panel_option_row(
                iced_fonts::lucide::gauge(),
                t("serial_baud"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-serial-baud")),
                    radius,
                    pick_list(Some(p.baud), COMMON_BAUD_RATES.to_vec(), |b: &u32| b.to_string())
                        .on_select(|v| Message::Editor(EditorMessage::EditorSerialBaudChanged(v)))
                        .id(iced::widget::Id::new("editor-serial-baud"))
                        .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                        .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                        .width(120)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .boxed(),
                ),
            );
            let data_bits_row = panel_option_row(
                iced_fonts::lucide::binary(),
                t("serial_data_bits"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-serial-data")),
                    radius,
                    pick_list(Some(p.data_bits), vec![8u8, 7, 6, 5], |n: &u8| n.to_string())
                        .on_select(|v| Message::Editor(EditorMessage::EditorSerialDataBitsChanged(v)))
                        .id(iced::widget::Id::new("editor-serial-data"))
                        .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                        .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                        .width(120)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .boxed(),
                ),
            );
            let parity_row = panel_option_row(
                iced_fonts::lucide::sigma(),
                t("serial_parity"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-serial-parity")),
                    radius,
                    pick_list(
                        Some(p.parity),
                        vec![SerialParity::None, SerialParity::Odd, SerialParity::Even],
                        |v: &SerialParity| v.to_string(),
                    )
                    .on_select(|v| Message::Editor(EditorMessage::EditorSerialParityChanged(v)))
                    .id(iced::widget::Id::new("editor-serial-parity"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(120)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .boxed(),
                ),
            );
            let stop_row = panel_option_row(
                iced_fonts::lucide::octagon_pause(),
                t("serial_stop_bits"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-serial-stop")),
                    radius,
                    pick_list(
                        Some(p.stop_bits),
                        vec![SerialStopBits::One, SerialStopBits::Two],
                        |v: &SerialStopBits| v.to_string(),
                    )
                    .on_select(|v| Message::Editor(EditorMessage::EditorSerialStopBitsChanged(v)))
                    .id(iced::widget::Id::new("editor-serial-stop"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(120)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .boxed(),
                ),
            );
            let flow_row = panel_option_row(
                iced_fonts::lucide::waves(),
                t("serial_flow_control"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-serial-flow")),
                    radius,
                    pick_list(
                        Some(p.flow_control),
                        vec![
                            SerialFlowControl::None,
                            SerialFlowControl::Software,
                            SerialFlowControl::Hardware,
                        ],
                        |v: &SerialFlowControl| v.to_string(),
                    )
                    .on_select(|v| Message::Editor(EditorMessage::EditorSerialFlowChanged(v)))
                    .id(iced::widget::Id::new("editor-serial-flow"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(140)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .boxed(),
                ),
            );
            let line_ending_row = panel_option_row(
                iced_fonts::lucide::corner_down_left(),
                t("serial_line_ending"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-serial-eol")),
                    radius,
                    pick_list(
                        Some(p.line_ending),
                        vec![
                            SerialLineEnding::Cr,
                            SerialLineEnding::Lf,
                            SerialLineEnding::CrLf,
                        ],
                        |v: &SerialLineEnding| v.to_string(),
                    )
                    .on_select(|v| Message::Editor(EditorMessage::EditorSerialLineEndingChanged(v)))
                    .id(iced::widget::Id::new("editor-serial-eol"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(120)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .boxed(),
                ),
            );
            // Local echo: ON/OFF toggle (raw serial has no ECHO
            // negotiation, so a non-echoing device shows nothing typed
            // until this is on).
            let echo_row = self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorSerialLocalEchoToggled)),
                8.0,
                container(
                    dir_row(vec![
                        iced_fonts::lucide::eye().size(13).color(OryxisColors::t().text_muted).boxed(),
                        Space::new().width(10).boxed(),
                        text(t("serial_local_echo")).size(13).color(OryxisColors::t().text_secondary).boxed(),
                        Space::new().width(Length::Fill).boxed(),
                        {
                            let on = p.local_echo;
                            let bg = if on { OryxisColors::t().success } else { OryxisColors::t().bg_hover };
                            let fg = crate::theme::contrast_text_for(bg);
                            button(text(if on { crate::i18n::t("toggle_on") } else { crate::i18n::t("toggle_off") }).size(12).color(fg))
                                .on_press(Message::Editor(EditorMessage::EditorSerialLocalEchoToggled))
                                .style(move |_theme, _status| button::Style {
                                    background: Some(Background::Color(bg)),
                                    border: Border { radius: Radius::from(4.0), ..Default::default() },
                                    text_color: fg,
                                    ..Default::default()
                                })
                                .boxed()
                        },
                    ]).align_y(iced::Alignment::Center)
                )
                .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 }).boxed(),
            );
            column![
                baud_row,
                Space::new().height(ROW_GAP),
                data_bits_row,
                Space::new().height(ROW_GAP),
                parity_row,
                Space::new().height(ROW_GAP),
                stop_row,
                Space::new().height(ROW_GAP),
                flow_row,
                Space::new().height(ROW_GAP),
                line_ending_row,
                Space::new().height(ROW_GAP),
                echo_row,
            ].boxed()
        } else {
            empty()
        };
        serial_params_block
    }
}
