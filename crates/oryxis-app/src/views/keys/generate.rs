//! Key generation side panel (spec form + result screen). Split out of views/keys.rs.

use iced::Widget as _;
use super::*;
use iced::widget::column;

impl Oryxis {
    /// The key-generation panel (keychain > ADD > Generate key). Two
    /// screens in one surface: the spec form, then the result screen
    /// once a key was generated and saved (fingerprint + public line +
    /// copy/save/export actions). Private material never enters this
    /// view's state; export re-reads it from the vault.
    pub(crate) fn view_key_generate_panel(&self) -> Element<'_, Message> {
        use crate::state::KeyGenAlgo;

        // Keyboard rows are recorded in visual order.
        self.panel_nav_reset();
        let form = &self.keys_ui.generate_form;

        let panel_header = container(
            dir_row(vec![
                text(t("generate_key")).size(18).color(OryxisColors::t().text_primary).boxed(),
                Space::new().width(Length::Fill).boxed(),
                crate::widgets::panel_close_button(Message::Keys(KeysMessage::HideKeyGeneratePanel)).boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 16.0, bottom: 12.0, left: 16.0 });

        let body: Element<'_, Message> = if let Some(result) = &form.result {
            // ── Result screen ──
            let public_block = container(
                text(result.public_key.clone())
                    .size(11)
                    .font(iced::Font::MONOSPACE)
                    .color(OryxisColors::t().text_secondary),
            )
            .padding(10)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });

            let mut col = column![
                dir_row(vec![
                    iced_fonts::lucide::circle_check().size(13).color(OryxisColors::t().success).boxed(),
                    Space::new().width(6).boxed(),
                    text(t("keygen_result_saved")).size(12).color(OryxisColors::t().success).boxed(),
                ]).align_y(iced::Alignment::Center),
                Space::new().height(12),
                crate::widgets::panel_field(
                    t("key_fingerprint"),
                    text(result.fingerprint.clone())
                        .size(11)
                        .font(iced::Font::MONOSPACE)
                        .color(OryxisColors::t().text_secondary)
                        .boxed(),
                ),
                Space::new().height(12),
                crate::widgets::panel_field(t("public_key"), public_block.boxed()),
                Space::new().height(10),
                dir_row(vec![
                    self.panel_nav_slot(
                        crate::keynav::RowAction::activate(Message::Keys(KeysMessage::CopyGeneratedPublicKey)),
                        6.0,
                        crate::widgets::styled_button(
                            t("keygen_copy_public"),
                            Message::Keys(KeysMessage::CopyGeneratedPublicKey),
                            OryxisColors::t().bg_selected,
                        ),
                    ),
                    Space::new().width(8).boxed(),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::activate(Message::Keys(KeysMessage::SaveGeneratedPublicKeyFile)),
                        6.0,
                        crate::widgets::styled_button(
                            t("keygen_save_pub"),
                            Message::Keys(KeysMessage::SaveGeneratedPublicKeyFile),
                            OryxisColors::t().bg_selected,
                        ),
                    ),
                ]),
                Space::new().height(20),
                text(t("keygen_export_private")).size(13).color(OryxisColors::t().text_primary),
                Space::new().height(6),
                text(t("keygen_export_desc")).size(11).color(OryxisColors::t().text_muted),
                Space::new().height(8),
            ];
            // Export passphrase pair; empty pair = plaintext export with
            // an explicit warning line. Keyboard rows per field: the
            // input, then its reveal eye (recorded by `wrap_eye`).
            self.panel_nav_record(crate::keynav::RowAction::input(
                iced::widget::Id::new("keygen-export-pass"),
            ));
            col = col.push(crate::widgets::panel_field(
                t("keygen_export_passphrase"),
                crate::widgets::password_input_with_eye_nav(
                    "",
                    &form.export_passphrase,
                    |v| Message::Keys(KeysMessage::KeyGenExportPassphraseChanged(v.into())),
                    None,
                    form.export_passphrase_visible,
                    Message::Keys(KeysMessage::KeyGenExportPassphraseToggleVisibility),
                    10.0,
                    Some(iced::widget::Id::new("keygen-export-pass")),
                    |eye| self.panel_nav_slot(
                        crate::keynav::RowAction::activate(
                            Message::Keys(KeysMessage::KeyGenExportPassphraseToggleVisibility),
                        ),
                        6.0,
                        eye,
                    ),
                ),
            ));
            col = col.push(Space::new().height(8).boxed());
            self.panel_nav_record(crate::keynav::RowAction::input(
                iced::widget::Id::new("keygen-export-pass-confirm"),
            ));
            col = col.push(crate::widgets::panel_field(
                t("keygen_export_passphrase_confirm"),
                crate::widgets::password_input_with_eye_nav(
                    "",
                    &form.export_passphrase_confirm,
                    |v| Message::Keys(KeysMessage::KeyGenExportPassphraseConfirmChanged(v.into())),
                    None,
                    form.export_passphrase_confirm_visible,
                    Message::Keys(KeysMessage::KeyGenExportPassphraseConfirmToggleVisibility),
                    10.0,
                    Some(iced::widget::Id::new("keygen-export-pass-confirm")),
                    |eye| self.panel_nav_slot(
                        crate::keynav::RowAction::activate(
                            Message::Keys(KeysMessage::KeyGenExportPassphraseConfirmToggleVisibility),
                        ),
                        6.0,
                        eye,
                    ),
                ),
            ));
            if form.export_passphrase.is_empty() && form.export_passphrase_confirm.is_empty() {
                col = col.push(Space::new().height(6).boxed());
                col = col.push(
                    text(t("keygen_export_plaintext_warn"))
                        .size(11)
                        .color(OryxisColors::t().warning).boxed(),
                );
            }
            col = col.push(Space::new().height(10).boxed());
            col = col.push(self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Keys(KeysMessage::ExportGeneratedPrivateKey)),
                6.0,
                crate::widgets::styled_button(
                    t("keygen_export_btn"),
                    Message::Keys(KeysMessage::ExportGeneratedPrivateKey),
                    OryxisColors::t().bg_selected,
                ),
            ));
            col.width(Length::Fill).align_x(dir_align_x()).boxed()
        } else {
            // ── Spec form ──
            self.panel_nav_record(crate::keynav::RowAction::input(
                iced::widget::Id::new("keygen-label"),
            ));
            let label_field = crate::widgets::panel_field(
                t("keygen_label"),
                text_input("deploy-key", &form.label)
                    .id(iced::widget::Id::new("keygen-label"))
                    .on_input(|v| Message::Keys(KeysMessage::KeyGenLabelChanged(v)))
                    .padding(10)
                    .size(13)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .boxed(),
            );

            let algo_picker = crate::widgets::panel_field(
                t("keygen_algorithm"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("keygen-algo")),
                    10.0,
                    pick_list(
                        Some(form.algo),
                        [KeyGenAlgo::Ed25519, KeyGenAlgo::Rsa, KeyGenAlgo::Ecdsa],
                        |a: &KeyGenAlgo| a.to_string(),
                    )
                    .on_select(|v| Message::Keys(KeysMessage::KeyGenAlgoSelected(v)))
                    .id(iced::widget::Id::new("keygen-algo"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .padding(10)
                    .text_size(13)
                    .style(crate::widgets::rounded_pick_list_style)
                    .boxed(),
                ),
            );

            // Dependent sub-picker, only for RSA / ECDSA.
            let sub_picker: Element<'_, Message> = match form.algo {
                KeyGenAlgo::Ed25519 => Space::new().boxed(),
                KeyGenAlgo::Rsa => column![
                    Space::new().height(12),
                    crate::widgets::panel_field(
                        t("keygen_bits"),
                        self.panel_nav_slot(
                            crate::keynav::RowAction::input(iced::widget::Id::new("keygen-bits")),
                            10.0,
                            pick_list(
                                Some(form.rsa_bits),
                                [
                                    oryxis_vault::RsaBits::B2048,
                                    oryxis_vault::RsaBits::B3072,
                                    oryxis_vault::RsaBits::B4096,
                                ],
                                |b: &oryxis_vault::RsaBits| b.to_string(),
                            )
                            .on_select(|v| Message::Keys(KeysMessage::KeyGenBitsSelected(v)))
                            .id(iced::widget::Id::new("keygen-bits"))
                            .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                            .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                            .padding(10)
                            .text_size(13)
                            .style(crate::widgets::rounded_pick_list_style)
                            .boxed(),
                        ),
                    ),
                ].boxed(),
                KeyGenAlgo::Ecdsa => column![
                    Space::new().height(12),
                    crate::widgets::panel_field(
                        t("keygen_curve"),
                        self.panel_nav_slot(
                            crate::keynav::RowAction::input(iced::widget::Id::new("keygen-curve")),
                            10.0,
                            pick_list(
                                Some(form.ecdsa_curve),
                                [
                                    oryxis_vault::EcdsaCurveChoice::P256,
                                    oryxis_vault::EcdsaCurveChoice::P384,
                                    oryxis_vault::EcdsaCurveChoice::P521,
                                ],
                                |c: &oryxis_vault::EcdsaCurveChoice| c.to_string(),
                            )
                            .on_select(|v| Message::Keys(KeysMessage::KeyGenCurveSelected(v)))
                            .id(iced::widget::Id::new("keygen-curve"))
                            .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                            .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                            .padding(10)
                            .text_size(13)
                            .style(crate::widgets::rounded_pick_list_style)
                            .boxed(),
                        ),
                    ),
                ].boxed(),
            };

            self.panel_nav_record(crate::keynav::RowAction::input(
                iced::widget::Id::new("keygen-comment"),
            ));
            let comment_field = crate::widgets::panel_field(
                t("keygen_comment"),
                text_input("user@example.com", &form.comment)
                    .id(iced::widget::Id::new("keygen-comment"))
                    .on_input(|v| Message::Keys(KeysMessage::KeyGenCommentChanged(v)))
                    .padding(10)
                    .size(13)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .boxed(),
            );

            let working: Element<'_, Message> = if form.working {
                column![
                    Space::new().height(10),
                    text(t("keygen_working")).size(12).color(OryxisColors::t().text_muted),
                ].boxed()
            } else {
                Space::new().boxed()
            };

            column![
                label_field,
                Space::new().height(12),
                algo_picker,
                sub_picker,
                Space::new().height(12),
                comment_field,
                working,
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .boxed()
        };

        // Shared form chrome: error above the footer, Cancel/Generate
        // (Generate disabled while a task is in flight; the result
        // screen swaps the primary for Done).
        let panel_error = crate::widgets::form_error(form.error.as_deref());
        let footer = if form.result.is_some() {
            crate::widgets::form_footer(
                Space::new().boxed(),
                self.panel_nav_slot(
                    crate::keynav::RowAction::activate(Message::Keys(KeysMessage::HideKeyGeneratePanel)),
                    6.0,
                    crate::widgets::form_save_button(
                        t("done"),
                        Some(Message::Keys(KeysMessage::HideKeyGeneratePanel)),
                    ),
                ),
            )
        } else {
            crate::widgets::form_footer(
                self.panel_nav_slot(
                    crate::keynav::RowAction::activate(Message::Keys(KeysMessage::HideKeyGeneratePanel)),
                    6.0,
                    crate::widgets::form_cancel_button(Message::Keys(KeysMessage::HideKeyGeneratePanel)),
                ),
                self.panel_nav_slot(
                    crate::keynav::RowAction::activate(Message::Keys(KeysMessage::GenerateKey)),
                    6.0,
                    crate::widgets::form_save_button(
                        t("keygen_generate_btn"),
                        (!form.working).then_some(Message::Keys(KeysMessage::GenerateKey)),
                    ),
                ),
            )
        };

        let panel_content = column![
            panel_header,
            scrollable(
                container(body)
                    .padding(Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 16.0 }),
            )
            // Shared id: the keyboard router keeps the selected row in view.
            .id(iced::widget::Id::new("side-panel-scroll"))
            .height(Length::Fill),
            panel_error,
            footer,
        ]
        .height(Length::Fill);

        crate::widgets::side_panel_frame(panel_content.boxed(), OryxisColors::t().bg_sidebar, self.panel_width)
    }
}
