//! Read-only certificate viewer modal (B2). Split out of views/keys.rs.

use iced::Widget as _;
use super::*;
use iced::widget::column;

impl Oryxis {
    /// Read-only viewer for a key's attached OpenSSH certificate (B2).
    /// Renders the parsed [`crate::state::CertViewerData`]; offers Remove
    /// (behind the standard confirm) and Close. Keynav rows record under
    /// `Modal::CertificateViewer` (Confirm family: Close is the default).
    pub(crate) fn view_cert_viewer_modal(&self) -> Element<'_, Message> {
        let Some(data) = self.cert_viewer.as_ref() else {
            return Space::new().boxed();
        };
        let c = OryxisColors::t();
        self.modal_nav_reset();

        // One label/value row; value in monospace for ids/fingerprints.
        let info_row = |label: String, value: String, mono: bool| -> Element<'_, Message> {
            let value_widget = text(value).size(12).color(c.text_primary);
            let value_widget = if mono { value_widget.font(iced::Font::MONOSPACE) } else { value_widget };
            column![
                text(label).size(11).color(c.text_muted),
                Space::new().height(2),
                value_widget,
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .boxed()
        };

        let mut body = column![
            dir_row(vec![
                iced_fonts::lucide::badge_check().size(16).color(c.accent).boxed(),
                Space::new().width(8).boxed(),
                container(text(&data.key_label).size(16).color(c.text_primary))
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .boxed(),
            ])
            .align_y(iced::Alignment::Center),
            Space::new().height(14),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        if data.expired {
            body = body.push(
                container(
                    dir_row(vec![
                        iced_fonts::lucide::triangle_alert().size(13).color(c.error).boxed(),
                        Space::new().width(6).boxed(),
                        text(t("cert_expired_warn")).size(12).color(c.error).boxed(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color { a: 0.1, ..c.error })),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }).boxed(),
            )
            .push(Space::new().height(12).boxed());
        }

        // Type (a full phrase) as its own line, then serial + key id.
        body = body
            .push(
                container(
                    text(t(if data.is_host { "cert_type_host" } else { "cert_type_user" }))
                        .size(12)
                        .color(c.accent),
                )
                .width(Length::Fill)
                .align_x(dir_align_x()).boxed(),
            )
            .push(Space::new().height(12).boxed())
            .push(info_row(
                t("cert_serial").to_string(),
                data.serial.to_string(),
                false,
            ));
        if !data.key_id.is_empty() {
            body = body.push(Space::new().height(10).boxed()).push(info_row(
                t("cert_key_id").to_string(),
                data.key_id.clone(),
                false,
            ));
        }
        let principals = if data.principals.is_empty() {
            "*".to_string()
        } else {
            data.principals.join(", ")
        };
        body = body
            .push(Space::new().height(10).boxed())
            .push(info_row(t("cert_principals").to_string(), principals, false));
        if !data.valid_from.is_empty() {
            body = body.push(Space::new().height(10).boxed()).push(info_row(
                t("cert_valid_from").to_string(),
                data.valid_from.clone(),
                false,
            ));
        }
        let until_label = data.valid_until.clone();
        if !until_label.is_empty() {
            let until_value = text(until_label)
                .size(12)
                .color(if data.expired { c.error } else { c.text_primary });
            body = body.push(Space::new().height(10).boxed()).push(
                column![
                    text(t("cert_valid_until")).size(11).color(c.text_muted),
                    Space::new().height(2),
                    until_value,
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()).boxed(),
            );
        }
        body = body.push(Space::new().height(10).boxed()).push(info_row(
            t("key_ca_sha256").to_string(),
            data.ca_fingerprint.clone(),
            true,
        ));

        let buttons = dir_row(vec![
            self.modal_nav_slot(
                crate::keynav::RowAction::activate(Message::Keys(KeysMessage::RequestRemoveKeyCertificate(data.key_idx))),
                6.0,
                false,
                crate::widgets::styled_button(t("cert_remove"), Message::Keys(KeysMessage::RequestRemoveKeyCertificate(data.key_idx)), c.error),
            ),
            Space::new().width(Length::Fill).boxed(),
            self.modal_nav_slot_default(
                crate::keynav::RowAction::activate(Message::Keys(KeysMessage::CloseCertViewer)),
                6.0,
                false,
                crate::widgets::styled_button(t("close"), Message::Keys(KeysMessage::CloseCertViewer), c.accent),
            ),
        ])
        .align_y(iced::Alignment::Center);

        let card = container(
            column![body, Space::new().height(18), buttons]
                .width(Length::Fill)
                .align_x(dir_align_x()),
        )
        .width(Length::Fixed(440.0))
        .padding(24)
        .style(move |_| container::Style {
            background: Some(Background::Color(c.bg_sidebar)),
            border: Border { color: c.border, width: 1.0, radius: Radius::from(12.0) },
            ..Default::default()
        });
        card.boxed()
    }
}
