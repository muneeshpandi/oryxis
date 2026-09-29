//! Settings -> Connection: the login-automation management block.
//!
//! Creation lives in the host editor (that is where a user discovers
//! they need one). This surface is for the things that need the whole
//! list: seeing which automations exist, how many hosts each serves,
//! renaming, deleting, and editing the steps a preset generated when
//! the bastion turns out to want something the three-field form cannot
//! express.

use super::*;
use iced::widget::column;

use oryxis_core::login_script::{ExpectPattern, LoginStep, SecretRef, SendPayload};

impl Oryxis {
    pub(crate) fn login_scripts_section(&self) -> Element<'_, Message> {
        let usage = self
            .vault
            .as_ref()
            .and_then(|v| v.login_script_usage().ok())
            .unwrap_or_default();

        let mut col = column![
            text(t("login_scripts"))
                .size(13)
                .color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("login_scripts_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(12),
        ];

        if self.login_scripts.is_empty() {
            col = col.push(
                text(t("login_script_empty"))
                    .size(12)
                    .color(OryxisColors::t().text_muted).boxed(),
            );
            return panel_section(col);
        }

        for script in &self.login_scripts {
            let count = usage.get(&script.id).copied().unwrap_or(0);
            let editing = self.login_script_form.editing_id == Some(script.id);
            let confirming = self.login_script_form.confirm_delete == Some(script.id);

            let header = dir_row(vec![
                column![
                    text(script.name.clone())
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                    text(
                        t("login_script_used_by").replace("{count}", &count.to_string()),
                    )
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                ].boxed(),
                Space::new().width(Length::Fill).boxed(),
                self.settings_nav_slot_labeled(
                    t("edit"),
                    crate::keynav::RowAction::activate(Message::Settings(
                        SettingsMessage::LoginScriptEdit(script.id),
                    )),
                    6.0,
                    crate::widgets::styled_button(
                        t("edit"),
                        Message::Settings(SettingsMessage::LoginScriptEdit(script.id)),
                        OryxisColors::t().bg_hover,
                    ),
                ),
                Space::new().width(8).boxed(),
                self.settings_nav_slot_labeled(
                    t("delete"),
                    crate::keynav::RowAction::activate(Message::Settings(
                        SettingsMessage::LoginScriptRequestDelete(script.id),
                    )),
                    6.0,
                    crate::widgets::styled_button(
                        t("delete"),
                        Message::Settings(SettingsMessage::LoginScriptRequestDelete(script.id)),
                        OryxisColors::t().bg_hover,
                    ),
                ),
            ])
            .align_y(iced::Alignment::Center);

            col = col.push(header.boxed());

            if confirming {
                col = col.push(Space::new().height(8).boxed()).push(
                    dir_row(vec![
                        text(t("login_script_delete_confirm"))
                            .size(11)
                            .color(OryxisColors::t().warning)
                            .boxed(),
                        Space::new().width(Length::Fill).boxed(),
                        self.settings_nav_slot_labeled(
                            t("delete"),
                            crate::keynav::RowAction::activate(Message::Settings(
                                SettingsMessage::LoginScriptDelete(script.id),
                            )),
                            6.0,
                            crate::widgets::styled_button(
                                t("delete"),
                                Message::Settings(SettingsMessage::LoginScriptDelete(script.id)),
                                OryxisColors::t().error,
                            ),
                        ),
                        Space::new().width(8).boxed(),
                        self.settings_nav_slot_labeled(
                            t("cancel"),
                            crate::keynav::RowAction::activate(Message::Settings(
                                SettingsMessage::LoginScriptCancelDelete,
                            )),
                            6.0,
                            crate::widgets::styled_button(
                                t("cancel"),
                                Message::Settings(SettingsMessage::LoginScriptCancelDelete),
                                OryxisColors::t().bg_hover,
                            ),
                        ),
                    ])
                    .align_y(iced::Alignment::Center).boxed(),
                );
            }

            if editing {
                col = col.push(Space::new().height(10).boxed()).push(self.login_script_editor());
            }

            col = col.push(Space::new().height(14).boxed());
        }

        panel_section(col)
    }

    /// The step editor for the expanded script: the escape hatch for a
    /// bastion whose dialogue the three-field preset cannot express.
    fn login_script_editor(&self) -> Element<'_, Message> {
        let form = &self.login_script_form;
        let mut col = column![
            self.settings_nav_slot_labeled(
                t("name"),
                crate::keynav::RowAction::input(iced::widget::Id::new("set-login-script-name")),
                10.0,
                text_input(t("login_script_name_ph"), &form.name)
                    .id(iced::widget::Id::new("set-login-script-name"))
                    .on_input(|v| Message::Settings(SettingsMessage::LoginScriptNameChanged(v)))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .boxed(),
            ),
            Space::new().height(12),
            text(t("login_script_steps"))
                .size(12)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
        ];

        for (i, step) in form.steps.iter().enumerate() {
            col = col.push(self.login_script_step_row(i, step));
            col = col.push(Space::new().height(10).boxed());
        }

        let footer = dir_row(vec![
            self.settings_nav_slot_labeled(
                t("login_script_add_step"),
                crate::keynav::RowAction::activate(Message::Settings(
                    SettingsMessage::LoginScriptAddStep,
                )),
                6.0,
                crate::widgets::styled_button(
                    t("login_script_add_step"),
                    Message::Settings(SettingsMessage::LoginScriptAddStep),
                    OryxisColors::t().bg_hover,
                ),
            ),
            Space::new().width(Length::Fill).boxed(),
            self.settings_nav_slot_labeled(
                t("save"),
                crate::keynav::RowAction::activate(Message::Settings(
                    SettingsMessage::LoginScriptSave,
                )),
                6.0,
                crate::widgets::styled_button(
                    t("save"),
                    Message::Settings(SettingsMessage::LoginScriptSave),
                    OryxisColors::t().accent,
                ),
            ),
            Space::new().width(8).boxed(),
            self.settings_nav_slot_labeled(
                t("cancel"),
                crate::keynav::RowAction::activate(Message::Settings(
                    SettingsMessage::LoginScriptCancelEdit,
                )),
                6.0,
                crate::widgets::styled_button(
                    t("cancel"),
                    Message::Settings(SettingsMessage::LoginScriptCancelEdit),
                    OryxisColors::t().bg_hover,
                ),
            ),
        ])
        .align_y(iced::Alignment::Center);

        if let Some(err) = &form.error {
            col = col.push(
                text(err.clone())
                    .size(11)
                    .color(OryxisColors::t().error).boxed(),
            );
            col = col.push(Space::new().height(8).boxed());
        }

        container(col.push(footer.boxed()))
            .padding(Padding {
                top: 12.0,
                right: 12.0,
                bottom: 12.0,
                left: 12.0,
            })
            .style(|_| container::Style {
                background: Some(iced::Background::Color(OryxisColors::t().bg_hover)),
                border: iced::Border {
                    radius: iced::border::Radius::from(6.0),
                    ..Default::default()
                },
                ..Default::default()
            })
            .boxed()
    }

    // `step` shares the element's lifetime: the Text-payload input
    // borrows its value straight from the step (post-refactor
    // `text_input` keeps the borrow), and the caller iterates steps
    // owned by `self` anyway.
    fn login_script_step_row<'a>(&'a self, i: usize, step: &'a LoginStep) -> Element<'a, Message> {
        let expect_text = match &step.expect {
            Some(ExpectPattern::Suffix(s)) => s.clone(),
            // A regex is shown with the marker the parser uses, so the
            // one field can round-trip both forms without a second
            // control per row.
            Some(ExpectPattern::Regex(r)) => format!("re:{r}"),
            None => String::new(),
        };
        let send_label = send_label_of(&step.send);
        let send_options: Vec<String> = SEND_KINDS.iter().map(|k| t(k).to_string()).collect();
        let (prev, next) =
            crate::keynav::slots::cycle_pair(&send_options, &send_label, move |v| {
                Message::Settings(SettingsMessage::LoginScriptStepSendKind(i, v))
            });

        let expect_id = iced::widget::Id::from(format!("set-login-step-expect-{i}"));
        self.settings_nav_record(crate::keynav::RowAction::input(expect_id.clone()));
        // Owned value: `expect_text` is derived per row and text_input
        // borrows its fragments for the element's lifetime.
        let expect_input = text_input(t("login_script_prompt_ph"), expect_text)
            .id(expect_id)
            .on_input(move |v| Message::Settings(SettingsMessage::LoginScriptStepExpect(i, v)))
            .padding(8)
            .style(crate::widgets::rounded_input_style);

        let send_picker = self.settings_nav_slot_labeled(
            t("login_script_step_send"),
            crate::keynav::RowAction::picker(prev, next),
            8.0,
            pick_list(
                Some(send_label.clone()),
                send_options.clone(),
                |s: &String| s.clone(),
            )
            .on_select(move |v| Message::Settings(SettingsMessage::LoginScriptStepSendKind(i, v)))
            .padding(8)
            .text_size(13)
            .boxed(),
        );

        let mut rows = column![
            dir_row(vec![
                text(t("login_script_step_expect"))
                    .size(11)
                    .color(OryxisColors::t().text_muted)
                    .width(90)
                    .boxed(),
                expect_input.boxed(),
            ])
            .align_y(iced::Alignment::Center)
            .spacing(8),
            Space::new().height(6),
            dir_row(vec![
                text(t("login_script_step_send"))
                    .size(11)
                    .color(OryxisColors::t().text_muted)
                    .width(90)
                    .boxed(),
                send_picker,
            ])
            .align_y(iced::Alignment::Center)
            .spacing(8),
        ];

        // The text a `Text` step sends is the only payload the user can
        // author; every other kind is a reference resolved at send time.
        if let SendPayload::Text(value) = &step.send {
            let id = iced::widget::Id::from(format!("set-login-step-text-{i}"));
            self.settings_nav_record(crate::keynav::RowAction::input(id.clone()));
            rows = rows.push(Space::new().height(6).boxed()).push(
                dir_row(vec![
                    Space::new().width(90).boxed(),
                    text_input(t("login_script_var_ph"), value)
                        .id(id)
                        .on_input(move |v| {
                            Message::Settings(SettingsMessage::LoginScriptStepText(i, v))
                        })
                        .padding(8)
                        .style(crate::widgets::rounded_input_style)
                        .boxed(),
                ])
                .align_y(iced::Alignment::Center)
                .spacing(8).boxed(),
            );
        }

        let optional = step.optional;
        rows = rows.push(Space::new().height(6).boxed()).push(
            dir_row(vec![
                Space::new().width(90).boxed(),
                self.settings_nav_slot_labeled(
                    t("login_script_step_optional"),
                    crate::keynav::RowAction::activate(Message::Settings(
                        SettingsMessage::LoginScriptStepOptional(i),
                    )),
                    6.0,
                    // A toggle chip: the label names the flag, the fill
                    // says whether it is on.
                    crate::widgets::styled_button(
                        t("login_script_step_optional"),
                        Message::Settings(SettingsMessage::LoginScriptStepOptional(i)),
                        if optional {
                            OryxisColors::t().accent
                        } else {
                            OryxisColors::t().bg_hover
                        },
                    ),
                ),
                Space::new().width(Length::Fill).boxed(),
                self.settings_nav_slot_labeled(
                    t("delete"),
                    crate::keynav::RowAction::activate(Message::Settings(
                        SettingsMessage::LoginScriptRemoveStep(i),
                    )),
                    6.0,
                    crate::widgets::styled_button(
                        t("delete"),
                        Message::Settings(SettingsMessage::LoginScriptRemoveStep(i)),
                        OryxisColors::t().bg_hover,
                    ),
                ),
            ])
            .align_y(iced::Alignment::Center).boxed(),
        );

        rows.boxed()
    }
}

/// The pickable send kinds, in the order they appear in the picker.
pub(crate) const SEND_KINDS: [&str; 6] = [
    "login_script_send_text",
    "login_script_send_target_password",
    "login_script_send_host_password",
    "login_script_send_totp",
    "login_script_send_enter",
    "login_script_send_nothing",
];

/// Display label for a payload, matching `SEND_KINDS`.
pub(crate) fn send_label_of(send: &SendPayload) -> String {
    let key = match send {
        SendPayload::Text(_) => "login_script_send_text",
        SendPayload::Secret(SecretRef::TargetPassword) => "login_script_send_target_password",
        SendPayload::Secret(SecretRef::ConnectionPassword) => "login_script_send_host_password",
        SendPayload::Secret(SecretRef::Totp) => "login_script_send_totp",
        // An identity reference has no picker entry: it can only arrive
        // from a synced peer, and showing it as its own kind would let
        // the user "pick" an identity this UI cannot choose.
        SendPayload::Secret(SecretRef::Identity(_)) => "login_script_send_host_password",
        SendPayload::Key(_) => "login_script_send_enter",
        SendPayload::Nothing => "login_script_send_nothing",
    };
    t(key).to_string()
}
