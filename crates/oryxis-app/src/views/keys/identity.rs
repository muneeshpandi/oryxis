//! Identity editor side panel. Split out of views/keys.rs.

use iced::Widget as _;
use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_identity_panel(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order (row mode: Up/Down from any input).
        self.panel_nav_reset();
        let panel_title = if self.identity_form.editing_id.is_some() { t("edit_identity") } else { t("new_identity") };

        // Panel header
        let panel_header = container(
            dir_row(vec![
                text(panel_title).size(18).color(OryxisColors::t().text_primary).boxed(),
                Space::new().width(Length::Fill).boxed(),
                crate::widgets::panel_close_button(Message::Keys(KeysMessage::HideIdentityPanel)).boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Label field
        let label_field = column![
            text(t("label")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("panel-identity-label")),
                10.0,
                text_input(t("my_identity_placeholder"), &self.identity_form.label)
                    .id(iced::widget::Id::new("panel-identity-label"))
                    .on_input(|v| Message::Keys(KeysMessage::IdentityLabelChanged(v)))
                    .padding(10)
                    .size(13)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .boxed(),
            ),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Username field
        let username_field = column![
            text(t("username")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            dir_row(vec![
                iced_fonts::lucide::user().size(13).color(OryxisColors::t().text_muted).boxed(),
                Space::new().width(10).boxed(),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("panel-identity-username")),
                    10.0,
                    text_input("root", &self.identity_form.username)
                        .id(iced::widget::Id::new("panel-identity-username"))
                        .on_input(|v| Message::Keys(KeysMessage::IdentityUsernameChanged(v)))
                        .padding(10)
                        .size(13)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                ),
            ]).align_y(iced::Alignment::Center),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Password field with eye toggle. Keyboard row: Tab focuses the
        // inner input via its id.
        let identity_pw_placeholder: &'static str = if self.identity_form.has_existing_password
            && !self.identity_form.password.touched()
        {
            "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"
        } else {
            t("password")
        };
        // Keyboard rows: the field, then its reveal eye.
        self.panel_nav_record(crate::keynav::RowAction::input(
            iced::widget::Id::new("panel-identity-password"),
        ));
        let password_field = column![
            text(t("password")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            dir_row(vec![
                iced_fonts::lucide::keyboard().size(13).color(OryxisColors::t().text_muted).boxed(),
                Space::new().width(10).boxed(),
                crate::widgets::password_input_with_eye_nav(
                    identity_pw_placeholder,
                    self.identity_form.password.as_str(),
                    |v| Message::Keys(KeysMessage::IdentityPasswordChanged(v.into())),
                    None,
                    self.identity_form.password_visible,
                    Message::Keys(KeysMessage::IdentityTogglePasswordVisibility),
                    10.0,
                    Some(iced::widget::Id::new("panel-identity-password")),
                    |eye| self.panel_nav_slot(
                        crate::keynav::RowAction::activate(
                            Message::Keys(KeysMessage::IdentityTogglePasswordVisibility),
                        ),
                        6.0,
                        eye,
                    ),
                ),
            ]).align_y(iced::Alignment::Center),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Key selector. Focusable select: Tab reaches it, Enter/Space
        // open it, the widget owns arrows/Esc while focused.
        let key_options = {
            let mut opts = vec!["(none)".to_string()];
            opts.extend(self.keys.iter().map(|k| k.label.clone()));
            opts
        };
        let key_selected = self.identity_form.key.clone().unwrap_or_else(|| "(none)".into());
        let key_field = column![
            text(t("ssh_key")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            dir_row(vec![
                text(t("add_key_btn")).size(12).color(OryxisColors::t().accent).boxed(),
                Space::new().width(16).boxed(),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("identity-pick-key")),
                    10.0,
                    pick_list(
                        Some(key_selected),
                        key_options,
                        |s: &String| s.clone(),
                    )
                    .on_select(|v| Message::Keys(KeysMessage::IdentityKeyChanged(v)))
                    .id(iced::widget::Id::new("identity-pick-key"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .padding(10).text_size(13).style(crate::widgets::rounded_pick_list_style)
                    .boxed(),
                ),
            ]).align_y(iced::Alignment::Center),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Linked connections (only when editing)
        let linked_section: Element<'_, Message> = if let Some(editing_id) = self.identity_form.editing_id {
            let linked: Vec<&Connection> = self.connections.iter()
                .filter(|c| c.identity_id == Some(editing_id))
                .collect();
            if linked.is_empty() {
                column![
                    Space::new().height(16),
                    text(t("linked_to")).size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(4),
                    text(t("no_connections_identity")).size(11).color(OryxisColors::t().text_muted),
                ].boxed()
            } else {
                let mut items: Vec<Element<'_, Message>> = vec![
                    Space::new().height(16).boxed(),
                    text(t("linked_to")).size(12).color(OryxisColors::t().text_muted).boxed(),
                    Space::new().height(4).boxed(),
                ];
                for conn in linked {
                    items.push(
                        container(
                            dir_row(vec![
                                iced_fonts::lucide::server().size(11).color(OryxisColors::t().text_muted).boxed(),
                                Space::new().width(8).boxed(),
                                text(&conn.label).size(12).color(OryxisColors::t().text_secondary).boxed(),
                            ]).align_y(iced::Alignment::Center),
                        )
                        .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
                        .boxed()
                    );
                }
                column(items).boxed()
            }
        } else {
            Space::new().boxed()
        };

        // Shared form footer: disabled Save while the label is empty
        // (structural gating instead of the old color-only hint that
        // still accepted clicks), Cancel closes the panel like Esc.
        let save_label = if self.identity_form.editing_id.is_some() { crate::i18n::t("update_identity") } else { crate::i18n::t("save_identity") };
        let has_label = !self.identity_form.label.trim().is_empty();
        let footer = crate::widgets::form_footer(
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Keys(KeysMessage::HideIdentityPanel)),
                6.0,
                crate::widgets::form_cancel_button(Message::Keys(KeysMessage::HideIdentityPanel)),
            ),
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Keys(KeysMessage::SaveIdentity)),
                6.0,
                crate::widgets::form_save_button(
                    save_label,
                    has_label.then_some(Message::Keys(KeysMessage::SaveIdentity)),
                ),
            ),
        );

        let panel_content = column![
            panel_header,
            container(
                column![
                    label_field,
                    Space::new().height(16),
                    username_field,
                    Space::new().height(16),
                    password_field,
                    Space::new().height(16),
                    key_field,
                    linked_section,
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 20.0, bottom: 0.0, left: 20.0 })
            .height(Length::Fill),
            footer,
        ]
        .height(Length::Fill);

        crate::widgets::side_panel_frame(panel_content.boxed(), OryxisColors::t().bg_sidebar, self.panel_width)
    }
}
