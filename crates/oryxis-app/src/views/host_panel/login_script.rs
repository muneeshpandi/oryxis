//! Host editor: the Login automation block (issue #122).
//!
//! For hosts reached through a bastion that authenticates INSIDE the
//! TTY. The block is three things stacked: pick the shared script,
//! fill its `{placeholders}` for this host, and hold the credential it
//! types at the asset's own prompt. Creating a script happens inline,
//! because the moment a user discovers they need one is while they are
//! staring at the host that will not log in.
//!
//! Rows are built (= keynav-recorded) in the order they render, per the
//! panel contract in `mod.rs`.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(super) fn hp_login_script_block(&self, is_ssh: bool) -> Element<'_, Message> {
        if !is_ssh {
            // Telnet has its own credential autofill inside the
            // session, serial has no login at all, and a remote desktop
            // drives no terminal to read.
            return empty();
        }

        let selected = self.login_script_selected();
        let (prev, next) = crate::keynav::slots::cycle_pair(
            self.editor_login_script_combo.options(),
            &selected,
            |v| Message::Editor(EditorMessage::EditorLoginScriptChanged(v)),
        );
        let picker: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::picker(prev, next),
            10.0,
            iced::widget::combo_box(
                &self.editor_login_script_combo,
                // Owned placeholder (frame-local String); see auth.rs.
                selected.clone(),
                Some(&selected),
                |v| Message::Editor(EditorMessage::EditorLoginScriptChanged(v)),
            )
            .on_open(Message::Editor(EditorMessage::EditorLoginScriptComboOpened))
            // Restores the committed pick on blur; see auth.rs.
            .on_close(Message::NoOp)
            .size(13)
            .padding(10)
            .input_style(crate::widgets::rounded_input_style)
            .menu_style(crate::widgets::combo_menu_style)
            .width(Length::Fill)
            .into(),
        );

        let mut block = column![
            text(t("login_script_label"))
                .size(12)
                .color(OryxisColors::t().text_muted),
            Space::new().height(4),
            text(t("login_script_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            picker,
        ];

        if let Some(draft) = &self.editor_form.login_script_draft {
            block = block.push(Space::new().height(ROW_GAP)).push(
                self.hp_login_script_draft(draft),
            );
        } else {
            // The selected script's steps, read-only. The entity IS an
            // ordered expect/send list; without this the preset form was
            // the only face the feature showed, and it read as a fixed
            // three-field form instead of a generator for that list.
            if let Some(script) = self
                .editor_form
                .login_script_id
                .and_then(|id| self.login_scripts.iter().find(|s| s.id == id))
            {
                let mut steps = column![].spacing(2);
                for (i, step) in script.steps.iter().enumerate() {
                    use oryxis_core::login_script::{ExpectPattern, SendPayload};
                    let expect = match &step.expect {
                        Some(ExpectPattern::Suffix(s)) => s.clone(),
                        Some(ExpectPattern::Regex(r)) => format!("re:{r}"),
                        None => String::new(),
                    };
                    let send = match &step.send {
                        // Literal text shows as itself (placeholders
                        // included); everything else shows its localized
                        // kind, since a secret has no text to show.
                        SendPayload::Text(t) => t.clone(),
                        other => {
                            crate::views::settings::login_scripts::send_label_of(other)
                        }
                    };
                    let line = if expect.is_empty() {
                        format!("{}. → {}", i + 1, send)
                    } else {
                        format!("{}. {} → {}", i + 1, expect, send)
                    };
                    steps = steps.push(
                        text(line)
                            .size(11)
                            .font(iced::Font::MONOSPACE)
                            .color(OryxisColors::t().text_muted),
                    );
                }
                let edit_msg = Message::Settings(
                    crate::app::SettingsMessage::LoginScriptOpenInSettings(script.id),
                );
                let edit_btn = self.panel_nav_slot(
                    crate::keynav::RowAction::activate(edit_msg.clone()),
                    6.0,
                    crate::widgets::styled_button(
                        t("login_script_edit_steps"),
                        edit_msg,
                        OryxisColors::t().bg_hover,
                    ),
                );
                block = block
                    .push(Space::new().height(ROW_GAP))
                    .push(steps)
                    .push(Space::new().height(8))
                    .push(edit_btn);
            }
            for (name, value) in self.login_script_variables() {
                let id = iced::widget::Id::from(format!("editor-script-var-{name}"));
                self.panel_nav_record(crate::keynav::RowAction::input(id.clone()));
                let var = name.clone();
                // Hand-rolled instead of `panel_field`, which borrows
                // its label for the element's whole lifetime: the label
                // here is the variable name, owned per iteration.
                let row = column![
                    // The variable name IS the label. It is what the
                    // script author wrote and what the step shows, so
                    // translating it would break the link between the
                    // two surfaces.
                    text(name).size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(4),
                    // Owned value: the pair is produced per iteration
                    // and text_input borrows its fragments for 'a.
                    text_input(t("login_script_var_ph"), value)
                        .id(id)
                        .on_input(move |v| {
                            Message::Editor(EditorMessage::EditorLoginScriptVarChanged(
                                var.clone(),
                                v,
                            ))
                        })
                        .padding(10)
                        .size(13)
                        .style(crate::widgets::rounded_input_style),
                ]
                .width(Length::Fill);
                block = block.push(Space::new().height(ROW_GAP)).push(row);
            }

            if self.login_script_uses_target_password() {
                let placeholder: &'static str = if self
                    .editor_form
                    .has_existing_target_password
                    && !self.editor_form.target_password.touched()
                {
                    "••••••••"
                } else {
                    t("target_password_ph")
                };
                let id = iced::widget::Id::new("editor-target-password");
                self.panel_nav_record(crate::keynav::RowAction::input(id.clone()));
                block = block.push(Space::new().height(ROW_GAP)).push(panel_field(
                    t("target_password"),
                    crate::widgets::password_input_with_eye_nav(
                        placeholder,
                        self.editor_form.target_password.as_str(),
                        |v| Message::Editor(EditorMessage::EditorTargetPasswordChanged(v.into())),
                        self.hp_submit(),
                        self.editor_form.target_password_visible,
                        Message::Editor(EditorMessage::EditorToggleTargetPasswordVisibility),
                        10.0,
                        Some(id),
                        |eye| {
                            self.panel_nav_slot(
                                crate::keynav::RowAction::activate(Message::Editor(
                                    EditorMessage::EditorToggleTargetPasswordVisibility,
                                )),
                                6.0,
                                eye,
                            )
                        },
                    ),
                ));
            }
        }

        block.into()
    }

    /// The inline "new script" sub-form: pick a template, name it, and
    /// adjust the three prompts the bastion actually prints.
    fn hp_login_script_draft<'a>(
        &'a self,
        draft: &'a crate::state::LoginScriptDraft,
    ) -> Element<'a, Message> {
        let template_label = match draft.template {
            crate::state::ScriptTemplate::JumpServer => t("login_script_tpl_jumpserver"),
            crate::state::ScriptTemplate::Bastion => t("login_script_tpl_bastion"),
        }
        .to_string();
        let (tpl_prev, tpl_next) = crate::keynav::slots::cycle_pair(
            self.editor_script_template_combo.options(),
            &template_label,
            |v| Message::Editor(EditorMessage::EditorScriptDraftTemplateChanged(v)),
        );
        let template_picker: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::picker(tpl_prev, tpl_next),
            10.0,
            iced::widget::combo_box(
                &self.editor_script_template_combo,
                // Owned placeholder (frame-local String); see auth.rs.
                template_label.clone(),
                Some(&template_label),
                |v| Message::Editor(EditorMessage::EditorScriptDraftTemplateChanged(v)),
            )
            .size(13)
            .padding(10)
            .input_style(crate::widgets::rounded_input_style)
            .menu_style(crate::widgets::combo_menu_style)
            .width(Length::Fill)
            .into(),
        );

        // One helper for the four text rows: same shape, same keynav
        // contract, different field.
        // `value` carries the outer 'a explicitly: the closure feeds it
        // to `text_input`, which post-refactor borrows its fragments
        // for the element's lifetime instead of copying.
        let field = |id: &'static str,
                     label: &'static str,
                     placeholder: &'static str,
                     value: &'a str,
                     on_input: fn(String) -> Message|
         -> Element<'a, Message> {
            let wid = iced::widget::Id::new(id);
            self.panel_nav_record(crate::keynav::RowAction::input(wid.clone()));
            panel_field(
                label,
                text_input(placeholder, value)
                    .id(wid)
                    .on_input(on_input)
                    .padding(10)
                    .size(13)
                    .style(crate::widgets::rounded_input_style)
                    .into(),
            )
        };

        let actions = dir_row(vec![
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(
                    EditorMessage::EditorScriptDraftCreate,
                )),
                6.0,
                crate::widgets::styled_button(
                    t("login_script_create"),
                    Message::Editor(EditorMessage::EditorScriptDraftCreate),
                    OryxisColors::t().accent,
                ),
            ),
            Space::new().width(8).into(),
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(
                    EditorMessage::EditorScriptDraftCancel,
                )),
                6.0,
                crate::widgets::styled_button(
                    t("cancel"),
                    Message::Editor(EditorMessage::EditorScriptDraftCancel),
                    OryxisColors::t().bg_hover,
                ),
            ),
        ])
        .align_y(iced::Alignment::Center);

        container(
            column![
                template_picker,
                Space::new().height(ROW_GAP),
                field(
                    "editor-script-name",
                    t("login_script_name"),
                    t("login_script_name_ph"),
                    &draft.name,
                    |v| Message::Editor(EditorMessage::EditorScriptDraftNameChanged(v)),
                ),
                Space::new().height(ROW_GAP),
                // The three prompts. Blank means "the bastion never
                // asks this", which drops the step rather than waiting
                // for something that will not come.
                field(
                    "editor-script-asset",
                    t("login_script_asset_prompt"),
                    t("login_script_prompt_ph"),
                    &draft.asset_prompt,
                    |v| Message::Editor(EditorMessage::EditorScriptDraftPromptChanged(
                        crate::state::ScriptPromptField::Asset,
                        v,
                    )),
                ),
                Space::new().height(ROW_GAP),
                field(
                    "editor-script-user",
                    t("login_script_user_prompt"),
                    t("login_script_prompt_ph"),
                    &draft.user_prompt,
                    |v| Message::Editor(EditorMessage::EditorScriptDraftPromptChanged(
                        crate::state::ScriptPromptField::User,
                        v,
                    )),
                ),
                Space::new().height(ROW_GAP),
                field(
                    "editor-script-password",
                    t("login_script_password_prompt"),
                    t("login_script_prompt_ph"),
                    &draft.password_prompt,
                    |v| Message::Editor(EditorMessage::EditorScriptDraftPromptChanged(
                        crate::state::ScriptPromptField::Credential,
                        v,
                    )),
                ),
                Space::new().height(ROW_GAP),
                actions,
            ]
        )
        .padding(Padding {
            top: 12.0,
            right: 12.0,
            bottom: 12.0,
            left: 12.0,
        })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border {
                radius: Radius::from(6.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
    }
}
