//! Custom terminal theme editor, rendered as a centered modal over the
//! app. Opened from the "+" card in the Settings -> Terminal theme grid
//! (create) or the edit affordance on a custom theme card. Built-in
//! presets aren't editable; custom themes live inline in that same grid.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::{button, column, container, row, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{SettingsMessage, Message, Oryxis};
use crate::i18n::t;
use crate::state::ThemeColorSlot;
use crate::theme::OryxisColors;
use crate::widgets::{dir_row, parse_hex_color};

/// Standard ANSI slot names (technical identifiers, left untranslated like
/// the palette comments in `colors.rs`).
const ANSI_NAMES: [&str; 16] = [
    "Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White",
    "Bright Black", "Bright Red", "Bright Green", "Bright Yellow",
    "Bright Blue", "Bright Magenta", "Bright Cyan", "Bright White",
];

impl Oryxis {
    /// The custom-theme editor modal. Caller renders it (over the base) only
    /// while `theme_editor` is `Some`.
    pub(crate) fn view_theme_editor_modal(&self) -> Element<'_, Message> {
        let Some(form) = self.theme_ui.editor.as_ref() else {
            return Space::new().boxed();
        };
        let title = if form.editing_id.is_some() {
            t("theme_edit")
        } else {
            t("theme_new_custom")
        };

        let name_input = text_input(t("theme_name"), &form.name)
            .on_input(|v| Message::Settings(SettingsMessage::ThemeEditorNameChanged(v)))
            .padding(10)
            .size(13)
            .style(crate::widgets::rounded_input_style);

        // Background / foreground / cursor, then the 16 ANSI entries. Each
        // swatch opens the compact color-picker popover for its slot.
        let mut slots = column![
            color_row(t("theme_background"), ThemeColorSlot::Background, &form.background),
            color_row(t("theme_foreground"), ThemeColorSlot::Foreground, &form.foreground),
            color_row(t("theme_cursor"), ThemeColorSlot::Cursor, &form.cursor),
            Space::new().height(10),
            text(t("theme_ansi_colors"))
                .size(13)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(6),
        ]
        .spacing(6);
        for i in 0..16u8 {
            slots = slots.push(color_row(
                ANSI_NAMES[i as usize],
                ThemeColorSlot::Ansi(i),
                &form.ansi[i as usize],
            ));
        }

        let form_col = column![
            text(t("theme_name")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            name_input,
            Space::new().height(16),
            slots,
        ]
        .width(Length::Fixed(330.0));

        let preview = column![
            text(t("theme_preview")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(8),
            theme_preview(form),
        ];

        // Shared form chrome (buttons + error slot); the row itself
        // keeps the modal card's trailing alignment.
        let footer = column![
            crate::widgets::form_error(form.error.as_deref()),
            dir_row(vec![
                Space::new().width(Length::Fill).boxed(),
                crate::widgets::form_cancel_button(Message::Settings(SettingsMessage::ThemeEditorClose)),
                Space::new().width(8).boxed(),
                crate::widgets::form_save_button(t("save"), Some(Message::Settings(SettingsMessage::ThemeEditorSave))),
            ])
            .align_y(iced::Alignment::Center),
        ]
        .spacing(8);

        // Pad the scrollable content on the right so the scrollbar doesn't
        // sit on top of the hex inputs.
        let scroll_form = scrollable(
            container(form_col)
                .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 0.0 }),
        )
        .height(Length::Fixed(380.0));

        let body = column![
            text(title).size(18).color(OryxisColors::t().text_primary),
            Space::new().height(16),
            dir_row(vec![
                scroll_form.boxed(),
                Space::new().width(24).boxed(),
                preview.boxed(),
            ]),
            Space::new().height(16),
            footer,
        ];

        let card = container(body)
            .padding(24)
            .width(Length::Fixed(720.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });

        // Trap clicks inside the card so they don't dismiss; backdrop click
        // closes the editor.
        let card_trap: Element<'_, Message> =
            MouseArea::new(card).on_press(Message::NoOp).boxed();

        let backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new()).width(Length::Fill).height(Length::Fill).style(|_| {
                container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                    ..Default::default()
                }
            }),
        )
        .on_press(Message::Settings(SettingsMessage::ThemeEditorClose))
        .boxed();

        let mut stack = iced::widget::Stack::<iced::Element<'_, _>>::new()
            .push(backdrop)
            .push(
                container(card_trap)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill).boxed(),
            )
            .width(Length::Fill)
            .height(Length::Fill);
        // Compact color-picker popover, anchored at the clicked swatch.
        if let Some((slot, anchor)) = self.theme_ui.color_popover {
            stack = stack.push(self.color_popover(form, slot, anchor));
        }
        // `opaque` makes the whole modal capture every mouse event so
        // clicks / scroll / hover don't fall through to the view behind.
        iced::widget::opaque(stack).boxed()
    }

    /// Compact color picker popover (SV square + hue bar + hex + presets),
    /// positioned at `anchor`, dismissed by clicking outside it.
    fn color_popover<'a>(
        &'a self,
        form: &'a crate::state::ThemeEditorForm,
        slot: ThemeColorSlot,
        anchor: iced::Point,
    ) -> Element<'a, Message> {
        let hex = match slot {
            ThemeColorSlot::Background => &form.background,
            ThemeColorSlot::Foreground => &form.foreground,
            ThemeColorSlot::Cursor => &form.cursor,
            ThemeColorSlot::Ansi(i) => &form.ansi[i as usize],
        };
        let color = parse_hex_color(hex).unwrap_or(Color::BLACK);

        let card = container(
            column![
                crate::color_picker::color_picker(color, move |hex| {
                    Message::Settings(SettingsMessage::ThemeEditorColorChanged(slot, hex))
                }),
                Space::new().height(10),
                text_input("#RRGGBB", hex)
                    .on_input(move |v| Message::Settings(SettingsMessage::ThemeEditorColorChanged(slot, v)))
                    .padding(7)
                    .size(12)
                    .style(crate::widgets::rounded_input_style),
                Space::new().height(10),
                preset_grid(slot),
            ]
            .spacing(0),
        )
        .padding(12)
        .width(Length::Fixed(236.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border {
                radius: Radius::from(10.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 14.0,
            },
            ..Default::default()
        });
        let card_trap: Element<'_, Message> =
            MouseArea::new(card).on_press(Message::NoOp).boxed();

        // Clamp the anchor so the popover stays on screen.
        const PW: f32 = 236.0;
        const PH: f32 = 330.0;
        let x = anchor.x.min((self.cur_window_size().width - PW).max(0.0)).max(0.0);
        let y = anchor.y.min((self.cur_window_size().height - PH).max(0.0)).max(0.0);
        let positioned = column![row![Space::new().width(x), card_trap]]
            .push(Space::new().boxed());
        let positioned = column![Space::new().height(y), positioned];

        let pop_backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new()).width(Length::Fill).height(Length::Fill),
        )
        .on_press(Message::Settings(SettingsMessage::ThemeEditorClosePicker))
        .boxed();

        iced::widget::Stack::<iced::Element<'_, _>>::new()
            .push(pop_backdrop)
            .push(positioned.boxed())
            .width(Length::Fill)
            .height(Length::Fill)
            .boxed()
    }

    /// Import-a-scheme modal: paste an iTerm / Windows Terminal / base16
    /// scheme; on import the parsed colors open in the editor for review.
    pub(crate) fn view_theme_import_modal(&self) -> Element<'_, Message> {
        let name_input = text_input(t("theme_name"), &self.theme_ui.import_name)
            .on_input(|v| Message::Settings(SettingsMessage::ThemeImportNameChanged(v)))
            .padding(10)
            .size(13)
            .style(crate::widgets::rounded_input_style);

        let paste = container(
            iced::widget::text_editor(&self.theme_ui.import_content)
                .on_action(|v| Message::Settings(SettingsMessage::ThemeImportContentAction(v)))
                .padding(10)
                .height(Length::Fixed(220.0))
                .font(iced::Font::MONOSPACE)
                .size(11),
        );

        let mut col = column![
            text(t("theme_import_title")).size(18).color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(t("theme_import_hint")).size(12).color(OryxisColors::t().text_muted),
            Space::new().height(16),
            text(t("theme_name")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            name_input,
            Space::new().height(12),
            paste,
        ]
        .spacing(0);
        // Shared form chrome (buttons + error slot); the row itself
        // keeps the modal card's trailing alignment.
        col = col.push(Space::new().height(8).boxed());
        col = col.push(crate::widgets::form_error(self.theme_ui.import_error.as_deref()));
        col = col.push(Space::new().height(8).boxed());
        // "Load from file" fills the paste box from a scheme file picked
        // on disk; Apply then parses it like any pasted content.
        col = col.push(
            dir_row(vec![
                import_browse_button(Message::Settings(SettingsMessage::ThemeImportBrowse)),
                Space::new().width(Length::Fill).boxed(),
                crate::widgets::form_cancel_button(Message::Settings(SettingsMessage::ThemeImportClose)),
                Space::new().width(8).boxed(),
                crate::widgets::form_save_button(
                    t("theme_import"),
                    Some(Message::Settings(SettingsMessage::ThemeImportApply)),
                ),
            ])
            .align_y(iced::Alignment::Center).boxed(),
        );

        let card = container(col)
            .padding(24)
            .width(Length::Fixed(560.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
        // Bare card; `widgets::modal_overlay` (the caller) owns centering,
        // the absorbing scrim, and the click-trap.
        card.boxed()
    }
}

impl Oryxis {
    /// A selectable custom terminal theme card with floating edit / delete
    /// icons revealed on hover (project card-action convention).
    pub(crate) fn terminal_custom_theme_card<'a>(
        &'a self,
        idx: usize,
        name: &'a str,
        palette: oryxis_terminal::TerminalPalette,
        is_selected: bool,
    ) -> Element<'a, Message> {
        let card = crate::widgets::terminal_theme_card(
            palette,
            name,
            is_selected,
            Message::Settings(SettingsMessage::TerminalThemeChanged(name.to_string())),
        );
        let mut stack = iced::widget::Stack::<iced::Element<'_, _>>::new().push(card);
        if self.hover.theme_card == Some(idx) {
            let actions = container(
                dir_row(vec![
                    theme_icon_btn(
                        iced_fonts::lucide::pencil(),
                        Message::Settings(SettingsMessage::ThemeEditorEdit(idx)),
                        t("edit"),
                    ),
                    Space::new().width(4).boxed(),
                    theme_icon_btn(
                        iced_fonts::lucide::copy(),
                        Message::Settings(SettingsMessage::ThemeClone(idx)),
                        t("duplicate"),
                    ),
                    Space::new().width(4).boxed(),
                    theme_icon_btn(
                        iced_fonts::lucide::upload(),
                        Message::Settings(SettingsMessage::ThemeExport(idx)),
                        t("theme_export"),
                    ),
                    Space::new().width(4).boxed(),
                    theme_icon_btn(
                        iced_fonts::lucide::trash(),
                        Message::Settings(SettingsMessage::ThemeDeleteRequested(idx)),
                        t("delete"),
                    ),
                ])
                .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Top)
            .padding(6);
            stack = stack.push(actions.boxed());
        }
        MouseArea::new(stack)
            .on_enter(Message::Settings(SettingsMessage::ThemeCardHovered(idx)))
            .on_exit(Message::Settings(SettingsMessage::ThemeCardUnhovered(idx)))
            .boxed()
    }

    /// A built-in terminal theme card in the settings grid with a floating
    /// clone icon revealed on hover, so a preset can be duplicated into an
    /// editable custom theme in one click.
    pub(crate) fn terminal_builtin_theme_card<'a>(
        &'a self,
        idx: usize,
        theme: &oryxis_terminal::TerminalTheme,
        is_selected: bool,
    ) -> Element<'a, Message> {
        let name = theme.name();
        let card = crate::widgets::terminal_theme_card(
            theme.palette(),
            name,
            is_selected,
            Message::Settings(SettingsMessage::TerminalThemeChanged(name.to_string())),
        );
        let mut stack = iced::widget::Stack::<iced::Element<'_, _>>::new().push(card);
        if self.hover.builtin_theme_card == Some(idx) {
            let actions = container(
                dir_row(vec![
                    theme_icon_btn(
                        iced_fonts::lucide::copy(),
                        Message::Settings(SettingsMessage::ThemeCloneBuiltin(idx)),
                        t("duplicate"),
                    ),
                    Space::new().width(4).boxed(),
                    theme_icon_btn(
                        iced_fonts::lucide::upload(),
                        Message::Settings(SettingsMessage::ThemeExportBuiltin(idx)),
                        t("theme_export"),
                    ),
                ])
                .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Top)
            .padding(6);
            stack = stack.push(actions.boxed());
        }
        MouseArea::new(stack)
            .on_enter(Message::Settings(SettingsMessage::ThemeBuiltinCardHovered(idx)))
            .on_exit(Message::Settings(SettingsMessage::ThemeBuiltinCardUnhovered(idx)))
            .boxed()
    }
}

/// "+ New custom theme" card that sits at the end of the terminal theme
/// grid and opens the editor.
pub(crate) fn terminal_theme_add_card<'a>() -> Element<'a, Message> {
    crate::widgets::theme_outline_card(
        iced_fonts::lucide::plus(),
        t("theme_new_custom"),
        OryxisColors::t().accent,
        Message::Settings(SettingsMessage::ThemeEditorNew),
    )
}

/// "Import" card that opens the paste-a-scheme modal.
pub(crate) fn terminal_theme_import_card<'a>() -> Element<'a, Message> {
    crate::widgets::theme_outline_card(
        iced_fonts::lucide::download(),
        t("theme_import"),
        OryxisColors::t().text_secondary,
        Message::Settings(SettingsMessage::ThemeImportOpen),
    )
}

/// "Community themes" card: opens the gallery on the site, where every
/// contributed palette previews in its own colours. Deliberately a link
/// rather than an in-app browser (issue #118): the site page works
/// against the RELEASED app, because Copy there lands a scheme the
/// Import card here already accepts.
pub(crate) fn terminal_theme_community_card<'a>() -> Element<'a, Message> {
    crate::widgets::theme_outline_card(
        iced_fonts::lucide::globe(),
        t("theme_community"),
        OryxisColors::t().accent,
        Message::OpenUrl("https://oryxis.app/themes".to_string()),
    )
}

/// "Load from file" footer button shared by the theme import modals
/// (terminal scheme + UI theme): opens a file picker whose contents land
/// in the modal's paste box.
pub(crate) fn import_browse_button<'a>(msg: Message) -> Element<'a, Message> {
    button(
        dir_row(vec![
            iced_fonts::lucide::folder_open()
                .size(13)
                .color(OryxisColors::t().text_secondary)
                .boxed(),
            Space::new().width(6).boxed(),
            text(t("theme_import_browse"))
                .size(12)
                .color(OryxisColors::t().text_secondary)
                .boxed(),
        ])
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .padding(Padding { top: 7.0, right: 12.0, bottom: 7.0, left: 12.0 })
    .style(|_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => {
                OryxisColors::t().bg_hover
            }
            _ => Color::TRANSPARENT,
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
    })
    .boxed()
}

/// Small floating icon button used for the per-card actions (edit / clone /
/// export / delete), wrapped in a tooltip per the icon-only convention.
fn theme_icon_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    tip: &'a str,
) -> Element<'a, Message> {
    let btn = button(
        container(icon.size(13).color(OryxisColors::t().text_primary))
            .center_x(Length::Fixed(24.0))
            .center_y(Length::Fixed(24.0)),
    )
    .on_press(msg)
    .padding(0)
    .style(|_, status| {
        let bg = match status {
            button::Status::Hovered => OryxisColors::t().bg_hover,
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
    });
    crate::views::terminal::icon_tooltip(btn.boxed(), tip)
}

/// One editable color slot: label, clickable swatch (opens the color-picker
/// popover for this slot), hex input.
fn color_row<'a>(
    label: &'a str,
    slot: ThemeColorSlot,
    hex: &'a str,
) -> Element<'a, Message> {
    let swatch_color = parse_hex_color(hex).unwrap_or(Color::TRANSPARENT);
    let swatch = button(Space::new().width(22).height(22))
        .on_press(Message::Settings(SettingsMessage::ThemeEditorOpenPicker(slot)))
        .padding(0)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(swatch_color)),
            border: Border {
                radius: Radius::from(5.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });
    dir_row(vec![
        text(label)
            .size(13)
            .color(OryxisColors::t().text_secondary)
            .width(Length::Fixed(100.0))
            .boxed(),
        swatch.boxed(),
        Space::new().width(10).boxed(),
        text_input("#RRGGBB", hex)
            .on_input(move |v| Message::Settings(SettingsMessage::ThemeEditorColorChanged(slot, v)))
            .padding(7)
            .size(12)
            .width(Length::Fixed(110.0))
            .style(crate::widgets::rounded_input_style)
            .boxed(),
    ])
    .align_y(iced::Alignment::Center)
    .boxed()
}

/// Compact preset-color grid inside the picker popover.
fn preset_grid<'a>(slot: ThemeColorSlot) -> Element<'a, Message> {
    let mut rows = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(5);
    let mut current = iced::widget::Row::<iced::Element<'_, _>>::new().spacing(5);
    let mut n = 0;
    for hex in crate::os_icon::PRESET_COLORS.iter() {
        let color = parse_hex_color(hex).unwrap_or(Color::TRANSPARENT);
        let sw = button(Space::new().width(18).height(18))
            .on_press(Message::Settings(SettingsMessage::ThemeEditorColorChanged(slot, (*hex).to_string())))
            .padding(0)
            .style(move |_, status| {
                let border = match status {
                    button::Status::Hovered => OryxisColors::t().text_primary,
                    _ => OryxisColors::t().border,
                };
                button::Style {
                    background: Some(Background::Color(color)),
                    border: Border { radius: Radius::from(4.0), color: border, width: 1.0 },
                    ..Default::default()
                }
            });
        current = current.push(sw.boxed());
        n += 1;
        if n % 9 == 0 {
            rows = rows.push(current.boxed());
            current = row![].spacing(5);
        }
    }
    if n % 9 != 0 {
        rows = rows.push(current.boxed());
    }
    rows.boxed()
}

/// Live preview: a sample terminal box painted with the in-progress colors.
fn theme_preview<'a>(form: &'a crate::state::ThemeEditorForm) -> Element<'a, Message> {
    let bg = parse_hex_color(&form.background).unwrap_or(Color::BLACK);
    let fg = parse_hex_color(&form.foreground).unwrap_or(Color::WHITE);
    let green = parse_hex_color(&form.ansi[2]).unwrap_or(fg);
    let blue = parse_hex_color(&form.ansi[4]).unwrap_or(fg);
    let yellow = parse_hex_color(&form.ansi[3]).unwrap_or(fg);
    let red = parse_hex_color(&form.ansi[1]).unwrap_or(fg);

    let mono = iced::Font::MONOSPACE;
    let line = |s: &'a str, c: Color| text(s).size(12).font(mono).color(c);

    let swatch_strip: Vec<Element<'a, Message>> = (0..16usize)
        .map(|i| {
            let c = parse_hex_color(&form.ansi[i]).unwrap_or(Color::TRANSPARENT);
            container(Space::new().width(14).height(14))
                .style(move |_| container::Style {
                    background: Some(Background::Color(c)),
                    border: Border { radius: Radius::from(3.0), ..Default::default() },
                    ..Default::default()
                })
                .boxed()
        })
        .collect();

    container(
        column![
            dir_row(vec![
                line("user", green).boxed(),
                line("@host", blue).boxed(),
                line(":~$ ", fg).boxed(),
                line("git status", fg).boxed(),
            ]),
            line("On branch main", fg),
            dir_row(vec![
                line("  modified: ", fg).boxed(),
                line("src/main.rs", red).boxed(),
            ]),
            dir_row(vec![
                line("  new file: ", fg).boxed(),
                line("themes.rs", green).boxed(),
            ]),
            line("warning: 2 files changed", yellow),
            Space::new().height(12),
            row(swatch_strip).spacing(4),
        ]
        .spacing(4),
    )
    .padding(16)
    .width(Length::Fixed(300.0))
    .style(move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    })
    .boxed()
}

