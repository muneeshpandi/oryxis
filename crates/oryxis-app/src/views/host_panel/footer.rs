//! Host editor: the bottom action row (Connect-without-saving / Save,
//! with the new-host vs quick-edit emphasis swap).
use super::*;

impl Oryxis {
    pub(super) fn hp_actions_row(&self, _has_address: bool) -> Element<'_, Message> {
        // ── Bottom actions ──
        // New-host flow: Connect (without saving) sits BEFORE Save both
        // visually and in the panel-nav recording. Quick-edit flow
        // (opened from an in-flight quick connect's progress screen):
        // the emphasis SWAPS, Connect takes the primary accent slot on
        // the trailing edge (the flow edits the temporary host and
        // re-dials) and Save becomes the explicit persist opt-in. Both
        // buttons are closures so each arrangement constructs them in
        // its own visual order (recording happens at construction).
        let quick_flow = self.vaults.cur().editor_form.quick_flow;
        let save_primary = !quick_flow;
        // Both footer buttons are SOLID for a clean, consistent look:
        // the primary action fills with the accent, the secondary with a
        // neutral solid surface (`bg_selected`) — never a faint outlined
        // box. Emphasis is the accent-vs-neutral contrast, not fill vs.
        // no-fill. (Previously Save only turned solid once an address was
        // typed and Connect was a flat outline, so the footer read washed
        // out.) Primary text pairs with `button_text`; the neutral solid
        // keeps the readable `text_primary`.
        let primary_style = |_: &iced::Theme, status: button::Status| {
            let accent = OryxisColors::t().accent;
            let bg = match status {
                button::Status::Hovered => Color {
                    r: (accent.r + 0.05).min(1.0),
                    g: (accent.g + 0.05).min(1.0),
                    b: (accent.b + 0.05).min(1.0),
                    a: 1.0,
                },
                button::Status::Pressed => Color { a: 0.85, ..accent },
                _ => accent,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }
        };
        let secondary_style = |_: &iced::Theme, status: button::Status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => OryxisColors::t().bg_hover,
                _ => OryxisColors::t().bg_selected,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }
        };
        let make_save_btn = |app: &Self| -> Element<'_, Message> {
            let fg = if save_primary {
                OryxisColors::t().button_text
            } else {
                OryxisColors::t().text_primary
            };
            app.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorSave)),
                8.0,
                button(
                    container(text(crate::i18n::t("save")).size(12).color(fg))
                        .padding(Padding { top: 7.0, right: 0.0, bottom: 7.0, left: 0.0 })
                        .width(Length::Fill)
                        .center_x(Length::Fill),
                )
                .on_press(Message::Editor(EditorMessage::EditorSave))
                .width(Length::Fill)
                .style(move |t, status| {
                    if save_primary {
                        primary_style(t, status)
                    } else {
                        secondary_style(t, status)
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
            // Connect is the PRIMARY in the quick-edit flow (accent),
            // otherwise the SECONDARY neutral-solid. Either way it is a
            // solid, filled button — never the old faint outline. The
            // label stays readable even before an address is typed (the
            // button's own validation gates the action, not a greyed
            // label).
            let connect_primary = quick_flow;
            let fg = if connect_primary {
                OryxisColors::t().button_text
            } else {
                OryxisColors::t().text_primary
            };
            let btn = app.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorConnectWithoutSaving)),
                8.0,
                button(
                    container(text(crate::i18n::t("connect")).size(12).color(fg))
                        .padding(Padding { top: 7.0, right: 0.0, bottom: 7.0, left: 0.0 })
                        .width(Length::Fill)
                        .center_x(Length::Fill),
                )
                .on_press(Message::Editor(EditorMessage::EditorConnectWithoutSaving))
                .width(Length::Fill)
                .style(move |t, status| {
                    if connect_primary {
                        primary_style(t, status)
                    } else {
                        secondary_style(t, status)
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
        let actions_row: Element<'_, Message> = if self.vaults.cur().editor_form.editing_id.is_none() {
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
            // Editing an existing host: an explicit Cancel / Save footer
            // (matching every other editor's `form_footer`), so the save
            // affordance is visible and consistent rather than implicit.
            // The underlying model is still auto-save on close, so:
            //   Save   = commit the edits and close (reuse EditorSave).
            //   Cancel = discard the edits and close (EditorDiscardClose,
            //            which drops the baseline so the close-flush net
            //            persists nothing). Solid red, like every Cancel.
            // Recorded on the panel keynav ring in visual order (cancel
            // then save), built after the fields so they land last.
            let cancel = self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorDiscardClose)),
                6.0,
                crate::widgets::form_cancel_button(Message::Editor(EditorMessage::EditorDiscardClose)),
            );
            let save = self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorSave)),
                6.0,
                crate::widgets::form_save_button(
                    crate::i18n::t("save"),
                    Some(Message::Editor(EditorMessage::EditorSave)),
                ),
            );
            dir_row(vec![cancel, Space::new().width(8).boxed(), save])
                .width(Length::Fill)
                .boxed()
        };
        actions_row
    }
}
