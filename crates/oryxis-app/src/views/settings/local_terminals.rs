//! Settings -> Terminal "Local terminals" card + its add/edit modal.
//! Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    /// Settings → Terminal "Local terminals" management card: the
    /// "always open X vs always ask" default picker, a grid of per-item
    /// cards (each with a hover-revealed remove), and Re-scan / Add
    /// actions in the header's top-right corner. The "Add" button opens
    /// a modal form (see `view_local_terminal_add_modal`). The list is
    /// the persisted, machine-local one populated by the one-time scan.
    pub(crate) fn local_terminals_card(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let entries = self.local_terminals.as_deref().unwrap_or(&[]);

        // Header: title + description on the leading edge, the Re-scan and
        // Add actions pinned to the top-right corner.
        let header = dir_row(vec![
            column![
                text(t("local_terminals")).size(13).color(c.text_primary),
                Space::new().height(4),
                text(t("local_terminals_desc")).size(11).color(c.text_muted),
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .boxed(),
            dir_row(vec![
                self.settings_nav_slot_labeled(
                    t("rescan_terminals"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::RescanLocalTerminals)),
                    6.0,
                    styled_button(t("rescan_terminals"), Message::Settings(SettingsMessage::RescanLocalTerminals), c.bg_selected),
                ),
                Space::new().width(8).boxed(),
                self.settings_nav_slot_labeled(
                    t("add_terminal"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::OpenLocalTerminalAddModal)),
                    6.0,
                    styled_button(t("add_terminal"), Message::Settings(SettingsMessage::OpenLocalTerminalAddModal), c.accent),
                ),
            ])
            .align_y(iced::Alignment::Center)
            .boxed(),
        ])
        .align_y(iced::Alignment::Start);

        // Default-behavior picker. The `None` sentinel = "always ask
        // (picker)"; every other option's identity is the entry id, shown
        // via the captured id->label map.
        let mut options: Vec<Option<uuid::Uuid>> = Vec::with_capacity(entries.len() + 1);
        options.push(None);
        options.extend(entries.iter().map(|e| Some(e.id)));
        let label_map: Vec<(uuid::Uuid, String)> =
            entries.iter().map(|e| (e.id, e.label.clone())).collect();
        let picker_label = t("always_ask_picker").to_string();
        let selected = Some(self.local_terminal_default);
        // Left/Right cycle "always ask" + every entry; the options vec
        // is consumed by pick_list, so cycle_pair reads a clone.
        let (def_prev, def_next) = crate::keynav::slots::cycle_pair(
            &options.clone(),
            &self.local_terminal_default,
            |v| Message::Settings(SettingsMessage::SetDefaultLocalTerminal(v)),
        );
        let default_picker = pick_list(selected, options, move |o: &Option<uuid::Uuid>| match o {
            None => picker_label.clone(),
            Some(id) => label_map
                .iter()
                .find(|(eid, _)| eid == id)
                .map(|(_, l)| l.clone())
                .unwrap_or_else(|| id.to_string()),
        })
        .on_select(|v| Message::Settings(SettingsMessage::SetDefaultLocalTerminal(v)))
        .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
        .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
        .width(280)
        .padding(10)
        .text_size(13)
        .style(crate::widgets::rounded_pick_list_style);
        let default_picker = self.settings_nav_slot_labeled(
            t("default_terminal_behavior"),
            crate::keynav::RowAction::picker(def_prev, def_next),
            8.0,
            default_picker.boxed(),
        );

        // One card per curated terminal, with a hover-revealed remove
        // action in the corner (card-action-icon convention). Enter
        // opens the edit modal (the card's primary action); the remove
        // button stays hover-only.
        let mut cards = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(8);
        if entries.is_empty() {
            cards = cards.push(text(t("local_terminals_empty")).size(12).color(c.text_muted).boxed());
        }
        for (idx, entry) in entries.iter().enumerate() {
            cards = cards.push(self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::OpenLocalTerminalEditModal(entry.id))),
                8.0,
                self.local_terminal_item_card(idx, entry),
            ));
        }

        panel_section(column![
            header,
            Space::new().height(14),
            text(t("default_terminal_behavior")).size(12).color(c.text_secondary),
            Space::new().height(6),
            default_picker,
            Space::new().height(16),
            cards,
        ])
    }

    /// One terminal item rendered as a card: a colored icon chip, the
    /// label (+ "manual" badge) and the resolved command line, with
    /// floating edit / remove buttons revealed on hover.
    fn local_terminal_item_card<'a>(
        &self,
        idx: usize,
        entry: &'a crate::state::LocalTerminalEntry,
    ) -> Element<'a, Message> {
        let c = OryxisColors::t();
        // Icon chip: explicit override, else OS hint, else terminal glyph.
        let (glyph, col) = crate::os_icon::local_terminal_icon(
            entry.icon.as_deref(),
            &entry.label,
            entry.color.as_deref(),
            c.accent,
        );
        let chip = container(glyph.view(16.0, Color::WHITE))
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(32.0))
            .center_x(Length::Fixed(32.0))
            .center_y(Length::Fixed(32.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(col)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            });

        let mut title: Vec<Element<'a, Message>> =
            vec![text(entry.label.clone()).size(13).color(c.text_primary).boxed()];
        if entry.manual {
            title.push(Space::new().width(8).boxed());
            title.push(
                container(text(t("manual_badge")).size(10).color(c.text_secondary))
                    .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_selected)),
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        ..Default::default()
                    })
                    .boxed(),
            );
        }
        let cmdline = if entry.args.is_empty() {
            entry.program.clone()
        } else {
            format!("{} {}", entry.program, entry.args.join(" "))
        };
        let card = container(
            dir_row(vec![
                chip.boxed(),
                Space::new().width(12).boxed(),
                column![
                    dir_row(title).align_y(iced::Alignment::Center),
                    Space::new().height(3),
                    text(cmdline).size(11).color(c.text_muted),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x())
                .boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding(12)
        .style(|_: &iced::Theme| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

        let mut stack = iced::widget::Stack::<iced::Element<'_, _>>::new().push(card.boxed());
        if self.cur_nav().hover.local_terminal_card == Some(idx) {
            let actions = dir_row(vec![
                local_terminal_card_btn(
                    iced_fonts::lucide::pencil(),
                    c.text_secondary,
                    Message::Settings(SettingsMessage::OpenLocalTerminalEditModal(entry.id)),
                ),
                Space::new().width(4).boxed(),
                local_terminal_card_btn(
                    iced_fonts::lucide::trash(),
                    c.error,
                    Message::Settings(SettingsMessage::RemoveLocalTerminal(entry.id)),
                ),
            ])
            .align_y(iced::Alignment::Center);
            stack = stack.push(
                container(actions)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .align_y(iced::alignment::Vertical::Top)
                    .padding(8).boxed(),
            );
        }
        iced::widget::MouseArea::new(stack)
            .on_enter(Message::Settings(SettingsMessage::LocalTerminalCardHovered(idx)))
            .on_exit(Message::Settings(SettingsMessage::LocalTerminalCardUnhovered(idx)))
            .boxed()
    }

    /// The "add local terminal" modal: label / program / arguments form
    /// with inline validation and a Cancel / Add footer. Layered by
    /// `root_view` when `local_terminal_add_open` is set.
    pub(crate) fn view_local_terminal_add_modal(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let editing = self.local_terminal_form.editing_id.is_some();
        let title = if editing { t("edit_terminal") } else { t("add_terminal") };
        let submit_label = if editing { t("save") } else { t("add_terminal") };

        // Appearance box: the live icon chip on its color, clickable to
        // open the shared host icon / color picker.
        let (glyph, col) = crate::os_icon::local_terminal_icon(
            self.local_terminal_form.icon.as_deref(),
            &self.local_terminal_form.label,
            self.local_terminal_form.color.as_deref(),
            c.accent,
        );
        let appearance = button(
            dir_row(vec![
                container(glyph.view(18.0, Color::WHITE))
                    .width(Length::Fixed(36.0))
                    .height(Length::Fixed(36.0))
                    .center_x(Length::Fixed(36.0))
                    .center_y(Length::Fixed(36.0))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(col)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    })
                    .boxed(),
                Space::new().width(12).boxed(),
                text(t("terminal_icon_color")).size(13).color(c.text_secondary).boxed(),
                Space::new().width(Length::Fill).boxed(),
                iced_fonts::lucide::pencil().size(13).color(c.text_muted).boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::Settings(SettingsMessage::OpenLocalTerminalIconPicker))
        .padding(10)
        .width(Length::Fill)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                BtnStatus::Pressed => OryxisColors::t().bg_selected,
                _ => OryxisColors::t().bg_primary,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }
        });

        let mut body = column![
            text(title)
                .size(15)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(c.text_primary),
            Space::new().height(16),
            panel_field(
                t("terminal_label"),
                text_input("PowerShell", &self.local_terminal_form.label)
                    .on_input(|v| Message::Settings(SettingsMessage::LocalTerminalFormLabelChanged(v)))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .boxed(),
            ),
            Space::new().height(10),
            panel_field(
                t("terminal_program"),
                text_input("/usr/bin/zsh", &self.local_terminal_form.program)
                    .on_input(|v| Message::Settings(SettingsMessage::LocalTerminalFormProgramChanged(v)))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .boxed(),
            ),
            Space::new().height(10),
            panel_field(
                t("terminal_args"),
                text_input("-l", &self.local_terminal_form.args)
                    .on_input(|v| Message::Settings(SettingsMessage::LocalTerminalFormArgsChanged(v)))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .boxed(),
            ),
            Space::new().height(10),
            panel_field(
                t("tags"),
                text_input(t("tags_placeholder"), &self.local_terminal_form.tags)
                    .on_input(|v| Message::Settings(SettingsMessage::LocalTerminalFormTagsChanged(v)))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .boxed(),
            ),
            Space::new().height(12),
            panel_field(t("terminal_icon_color"), appearance.boxed()),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());
        if let Some(err) = self.local_terminal_form.error {
            body = body
                .push(Space::new().height(8).boxed())
                .push(text(t(err)).size(11).color(c.error).boxed());
        }
        body = body.push(Space::new().height(18).boxed()).push(dir_row(vec![
            Space::new().width(Length::Fill).boxed(),
            styled_button(t("cancel"), Message::Settings(SettingsMessage::CloseLocalTerminalAddModal), c.bg_selected),
            Space::new().width(8).boxed(),
            styled_button(submit_label, Message::Settings(SettingsMessage::AddLocalTerminalSubmit), c.accent),
        ]).boxed());

        let dialog = iced::widget::MouseArea::new(
            container(body)
                .width(Length::Fixed(420.0))
                .padding(20)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border {
                        radius: Radius::from(12.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    shadow: iced::Shadow {
                        color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
                        offset: iced::Vector::new(0.0, 8.0),
                        blur_radius: 24.0,
                    },
                    ..Default::default()
                }),
        )
        .on_press(Message::NoOp);
        dialog.boxed()
    }
}

/// Floating icon button for a local-terminal card's hover actions
/// (edit / remove). `fg` tints the glyph; the background carries the
/// hover / press feedback.
fn local_terminal_card_btn<'a>(
    icon: iced::widget::Text<'a>,
    fg: Color,
    msg: Message,
) -> Element<'a, Message> {
    button(
        container(icon.size(13).color(fg))
            .center_x(Length::Fixed(24.0))
            .center_y(Length::Fixed(24.0)),
    )
    .on_press(msg)
    .padding(0)
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            BtnStatus::Pressed => OryxisColors::t().bg_selected,
            _ => OryxisColors::t().bg_surface,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        }
    })
    .boxed()
}
