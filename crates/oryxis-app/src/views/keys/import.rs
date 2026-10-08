//! Key import side panel (private/public/cert paste + browse). Split out of views/keys.rs.

use iced::Widget as _;
use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_key_import_panel(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order (row mode: Up/Down from any input).
        self.panel_nav_reset();
        let has_content = !self.keys_ui.import_form.pem.is_empty();
        // Public-only rows (B3 security keys) save with no private
        // material at all, so a filled public line also arms Save.
        let can_save =
            has_content || !self.keys_ui.import_form.public_key.trim().is_empty();
        let panel_title = if self.keys_ui.import_form.editing_id.is_some() { t("edit_key") } else { t("add_key") };

        // Panel header
        let panel_header = container(
            dir_row(vec![
                text(panel_title).size(18).color(OryxisColors::t().text_primary).boxed(),
                Space::new().width(Length::Fill).boxed(),
                crate::widgets::panel_close_button(Message::Keys(KeysMessage::HideKeyPanel)).boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Name field
        let name_field = column![
            text(t("name")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("panel-key-import-name")),
                10.0,
                text_input("my-server-key", &self.keys_ui.import_form.label)
                    .id(iced::widget::Id::new("panel-key-import-name"))
                    .on_input(|v| Message::Keys(KeysMessage::KeyImportLabelChanged(v)))
                    .padding(10)
                    .size(13)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .boxed(),
            ),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // File selector: the same small "Browse..." affordance the
        // Certificate section header uses, so all three sections share
        // one visual pattern (label leading, Browse trailing).
        let browse_btn = self.panel_nav_slot(
            crate::keynav::RowAction::activate(Message::Keys(KeysMessage::BrowseKeyFile)),
            6.0,
            button(text(t("cert_browse")).size(12).color(OryxisColors::t().accent))
                .on_press(Message::Keys(KeysMessage::BrowseKeyFile))
                .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => Color { a: 0.1, ..OryxisColors::t().accent },
                        BtnStatus::Pressed => Color { a: 0.18, ..OryxisColors::t().accent },
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }
                })
                .boxed(),
        );

        // Status indicator
        let file_status: Element<'_, Message> = if has_content {
            container(
                dir_row(vec![
                    iced_fonts::lucide::circle_check()
                        .size(13)
                        .color(OryxisColors::t().success)
                        .boxed(),
                    Space::new().width(6).boxed(),
                    text(
                        t("loaded_bytes")
                            .replacen("{bytes}", &self.keys_ui.import_form.pem.len().to_string(), 1),
                    )
                    .size(12).color(OryxisColors::t().success).boxed(),
                ]).align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
            .boxed()
        } else {
            Space::new().boxed()
        };

        // Editable key content (text_editor = multi-line)
        let editor = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("panel-key-import-content")),
            10.0,
            text_editor(&self.keys_ui.import_content)
                .id(iced::widget::Id::new("panel-key-import-content"))
                .on_action(|v| Message::Keys(KeysMessage::KeyContentAction(v)))
                .padding(10)
                .height(180)
                .font(iced::Font::MONOSPACE)
                .size(11)
                .style(crate::widgets::rounded_editor_style)
                .boxed(),
        );

        // Passphrase prompt, shown only after import_key signals the key
        // is encrypted. The hint explains the one-time-decrypt model so
        // users understand we're not storing the passphrase anywhere.
        // Recorded only when rendered, so the keyboard row appears in
        // place between the content editor and the Save button.
        let passphrase_section: Element<'_, Message> = if self.keys_ui.import_form.passphrase_required {
            // Keyboard rows: the field, then its reveal eye.
            self.panel_nav_record(crate::keynav::RowAction::input(
                iced::widget::Id::new("panel-key-import-passphrase"),
            ));
            column![
                Space::new().height(12),
                text(t("key_passphrase_label")).size(12).color(OryxisColors::t().text_secondary),
                Space::new().height(6),
                dir_row(vec![
                    iced_fonts::lucide::lock().size(13).color(OryxisColors::t().text_muted).boxed(),
                    Space::new().width(10).boxed(),
                    crate::widgets::password_input_with_eye_nav(
                        t("key_passphrase_placeholder"),
                        &self.keys_ui.import_form.passphrase,
                        |v| Message::Keys(KeysMessage::KeyImportPassphraseChanged(v.into())),
                        Some(Message::Keys(KeysMessage::ImportKey)),
                        self.keys_ui.import_form.passphrase_visible,
                        Message::Keys(KeysMessage::KeyImportPassphraseToggleVisibility),
                        10.0,
                        Some(iced::widget::Id::new("panel-key-import-passphrase")),
                        |eye| self.panel_nav_slot(
                            crate::keynav::RowAction::activate(
                                Message::Keys(KeysMessage::KeyImportPassphraseToggleVisibility),
                            ),
                            6.0,
                            eye,
                        ),
                    ),
                ]).align_y(iced::Alignment::Center),
                Space::new().height(6),
                text(t("key_passphrase_hint")).size(11).color(OryxisColors::t().text_muted),
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .boxed()
        } else {
            Space::new().boxed()
        };

        // Editable public-key line (B2.1, Termius parity): empty derives
        // from the private key on save; a pasted / edited line must match
        // the private key (the comment may differ, that is the point,
        // it is what the ssh-agent serves). Prefilled from `<key>.pub`
        // on browse and from the stored key on edit. A wrapping textarea
        // rather than a one-line input: OpenSSH public lines are far
        // wider than the panel.
        let public_section = column![
            Space::new().height(16),
            text(t("public_key")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("panel-key-import-public")),
                10.0,
                text_editor(&self.keys_ui.import_public_content)
                    .id(iced::widget::Id::new("panel-key-import-public"))
                    .on_action(|v| Message::Keys(KeysMessage::KeyImportPublicAction(v)))
                    .placeholder("ssh-ed25519 AAAA...")
                    .padding(10)
                    .height(72)
                    .font(iced::Font::MONOSPACE)
                    .size(11)
                    .style(crate::widgets::rounded_editor_style)
                    .boxed(),
            ),
            Space::new().height(4),
            text(t("public_key_auto_hint")).size(11).color(OryxisColors::t().text_muted),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Attached-certificate section (B2): a paste field + Browse
        // button for a signed `-cert.pub` user certificate. Optional; the
        // auto-probe on file pick prefills it and raises the hint below.
        // Keyboard rows record in build order: Browse, then the field.
        let cert_browse_btn = self.panel_nav_slot(
            crate::keynav::RowAction::activate(Message::Keys(KeysMessage::BrowseCertFile)),
            6.0,
            button(text(t("cert_browse")).size(12).color(OryxisColors::t().accent))
                .on_press(Message::Keys(KeysMessage::BrowseCertFile))
                .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => Color { a: 0.1, ..OryxisColors::t().accent },
                        BtnStatus::Pressed => Color { a: 0.18, ..OryxisColors::t().accent },
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }
                })
                .boxed(),
        );
        let mut cert_section = column![
            Space::new().height(16),
            dir_row(vec![
                text(t("certificate")).size(12).color(OryxisColors::t().text_secondary).boxed(),
                Space::new().width(Length::Fill).boxed(),
                cert_browse_btn,
            ]).align_y(iced::Alignment::Center),
            Space::new().height(6),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("panel-key-import-cert")),
                10.0,
                text_editor(&self.keys_ui.import_cert_content)
                    .id(iced::widget::Id::new("panel-key-import-cert"))
                    .on_action(|v| Message::Keys(KeysMessage::KeyImportCertAction(v)))
                    .placeholder("ssh-ed25519-cert-v01@openssh.com AAAA...")
                    .padding(10)
                    .height(72)
                    .font(iced::Font::MONOSPACE)
                    .size(11)
                    .style(crate::widgets::rounded_editor_style)
                    .boxed(),
            ),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());
        if self.keys_ui.import_form.cert_detected {
            cert_section = cert_section.push(Space::new().height(6).boxed()).push(
                dir_row(vec![
                    iced_fonts::lucide::circle_check()
                        .size(12)
                        .color(OryxisColors::t().success)
                        .boxed(),
                    Space::new().width(6).boxed(),
                    text(t("cert_detected_hint")).size(11).color(OryxisColors::t().success).boxed(),
                ]).align_y(iced::Alignment::Center).boxed(),
            );
        } else {
            cert_section = cert_section.push(Space::new().height(4).boxed()).push(
                text(t("certificate_desc")).size(11).color(OryxisColors::t().text_muted).boxed(),
            );
        }

        // Shared form chrome: inline error above the footer, disabled
        // Save while there is no key content (structural gating
        // instead of the old color-only hint that still took clicks).
        let panel_error = crate::widgets::form_error(self.keys_ui.error.as_deref());
        let save_label = if self.keys_ui.import_form.editing_id.is_some() { t("update_key") } else { t("save_key") };
        let footer = crate::widgets::form_footer(
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Keys(KeysMessage::HideKeyPanel)),
                6.0,
                crate::widgets::form_cancel_button(Message::Keys(KeysMessage::HideKeyPanel)),
            ),
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Keys(KeysMessage::ImportKey)),
                6.0,
                crate::widgets::form_save_button(
                    save_label,
                    can_save.then_some(Message::Keys(KeysMessage::ImportKey)),
                ),
            ),
        );

        let panel_content = column![
            panel_header,
            container(
                column![
                    name_field,
                    Space::new().height(16),
                    // Section header matches the Certificate one: label
                    // leading, small Browse trailing.
                    dir_row(vec![
                        text(t("private_key"))
                            .size(12)
                            .color(OryxisColors::t().text_secondary)
                            .boxed(),
                        Space::new().width(Length::Fill).boxed(),
                        browse_btn,
                    ])
                    .align_y(iced::Alignment::Center),
                    Space::new().height(6),
                    editor,
                    Space::new().height(4),
                    file_status,
                    passphrase_section,
                    public_section,
                    cert_section,
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 20.0, bottom: 0.0, left: 20.0 })
            .height(Length::Fill),
            panel_error,
            footer,
        ]
        .height(Length::Fill);

        crate::widgets::side_panel_frame(panel_content.boxed(), OryxisColors::t().bg_sidebar, self.panel_width)
    }
}
