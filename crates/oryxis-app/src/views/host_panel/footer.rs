//! Host editor: the bottom action row (Connect-without-saving / Save,
//! with the new-host vs quick-edit emphasis swap).
use super::*;

impl Oryxis {
    pub(super) fn hp_actions_row(&self, has_address: bool) -> Element<'_, Message> {
        // ── Bottom actions ──
        // New-host flow: Connect (without saving) sits BEFORE Save both
        // visually and in the panel-nav recording. Quick-edit flow
        // (opened from an in-flight quick connect's progress screen):
        // the emphasis SWAPS, Connect takes the primary accent slot on
        // the trailing edge (the flow edits the temporary host and
        // re-dials) and Save becomes the explicit persist opt-in. Both
        // buttons are closures so each arrangement constructs them in
        // its own visual order (recording happens at construction).
        let quick_flow = self.editor_form.quick_flow;
        let save_primary = !quick_flow;
        let save_btn_bg = if save_primary && has_address {
            OryxisColors::t().accent
        } else {
            OryxisColors::t().bg_surface
        };
        let make_save_btn = |app: &Self| -> Element<'_, Message> {
            app.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorSave)),
                8.0,
                button(
                    container(text(crate::i18n::t("save")).size(14).color(OryxisColors::t().text_primary))
                        .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                        .width(Length::Fill)
                        .center_x(Length::Fill),
                )
                .on_press(Message::Editor(EditorMessage::EditorSave))
                .width(Length::Fill)
                .style(move |_, status| {
                    let bg = if save_primary {
                        save_btn_bg
                    } else {
                        match status {
                            button::Status::Hovered | button::Status::Pressed => {
                                OryxisColors::t().bg_hover
                            }
                            _ => OryxisColors::t().bg_surface,
                        }
                    };
                    let border = if save_primary {
                        Border { radius: Radius::from(8.0), ..Default::default() }
                    } else {
                        Border {
                            radius: Radius::from(8.0),
                            width: 1.0,
                            color: OryxisColors::t().border,
                        }
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border,
                        ..Default::default()
                    }
                })
                .boxed(),
            )
        };

        // "Connect" (without saving): quick-connect straight from the form,
        // new-host flow only (an existing host already has a card to
        // connect from, and its stored secrets would not ride along).
        // The short label gets a tooltip spelling out that nothing is
        // written to the vault.
        let make_connect_btn = |app: &Self| -> Element<'_, Message> {
            let connect_bg = if quick_flow && has_address {
                OryxisColors::t().accent
            } else {
                OryxisColors::t().bg_surface
            };
            let btn = app.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorConnectWithoutSaving)),
                8.0,
                button(
                    container(
                        text(crate::i18n::t("connect"))
                            .size(14)
                            .color(if has_address {
                                OryxisColors::t().text_primary
                            } else {
                                OryxisColors::t().text_muted
                            }),
                    )
                    .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                    .width(Length::Fill)
                    .center_x(Length::Fill),
                )
                .on_press(Message::Editor(EditorMessage::EditorConnectWithoutSaving))
                .width(Length::Fill)
                .style(move |_, status| {
                    if quick_flow {
                        button::Style {
                            background: Some(Background::Color(connect_bg)),
                            border: Border { radius: Radius::from(8.0), ..Default::default() },
                            ..Default::default()
                        }
                    } else {
                        let bg = match status {
                            button::Status::Hovered | button::Status::Pressed => {
                                OryxisColors::t().bg_hover
                            }
                            _ => OryxisColors::t().bg_surface,
                        };
                        button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border {
                                radius: Radius::from(8.0),
                                width: 1.0,
                                color: OryxisColors::t().border,
                            },
                            ..Default::default()
                        }
                    }
                })
                .boxed(),
            );
            iced::widget::tooltip(
                btn,
                container(
                    text(crate::i18n::t("quick_connect_not_saved"))
                        .size(11)
                        .color(OryxisColors::t().text_primary),
                )
                .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                }),
                iced::widget::tooltip::Position::Top,
            )
            .boxed()
        };
        let actions_row: Element<'_, Message> = if self.editor_form.editing_id.is_none() {
            let (leading, trailing) = if quick_flow {
                // Connect is the primary: it takes the trailing slot Save
                // holds in the normal flow.
                (make_save_btn(self), make_connect_btn(self))
            } else {
                (make_connect_btn(self), make_save_btn(self))
            };
            dir_row(vec![leading, Space::new().width(8).boxed(), trailing])
                .width(Length::Fill)
                .boxed()
        } else {
            // Editing an existing host: NOTHING. Removing the Save
            // button was about removing the footer, not about replacing
            // it with a caption; a standing "changes are saved
            // automatically" is a label the user reads once and then
            // has to look past forever. The write happens when the
            // drawer closes (dispatch_editor/autosave.rs), which is the
            // same gesture that already means "done", so there is
            // nothing here to confirm and nothing to press. Enter
            // saves-and-closes via the fields' on_submit; Esc / X
            // flush on their way out; only a FAILED write speaks up
            // (inline error + toast).
            Space::new().boxed()
        };
        actions_row
    }
}
