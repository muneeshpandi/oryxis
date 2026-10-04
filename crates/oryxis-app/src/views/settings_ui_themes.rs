//! Custom UI (chrome) theme editor, rendered as a centered modal. Opened
//! from the "+" card in Settings -> Interface's theme grid (create) or the
//! edit affordance on a custom theme card. Mirrors the terminal theme
//! editor; the 21 chrome colors are addressed by index into
//! `theme::UI_COLOR_FIELDS`.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::{button, column, container, row, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{SettingsMessage, Message, Oryxis};
use crate::i18n::t;
use crate::theme::{OryxisColors, UI_COLOR_FIELDS};
use crate::widgets::{dir_row, parse_hex_color};

fn col(hex: &str, fallback: Color) -> Color {
    parse_hex_color(hex).unwrap_or(fallback)
}

impl Oryxis {
    /// The custom UI theme editor modal.
    pub(crate) fn view_ui_theme_editor_modal(&self) -> Element<'_, Message> {
        let Some(form) = self.ui_theme_editor.as_ref() else {
            return Space::new().boxed();
        };
        let title = if form.editing_id.is_some() {
            t("theme_edit")
        } else {
            t("theme_new_custom")
        };

        let name_input = text_input(t("theme_name"), &form.name)
            .on_input(|v| Message::Settings(SettingsMessage::UiThemeEditorNameChanged(v)))
            .padding(10)
            .size(13)
            .style(crate::widgets::rounded_input_style);

        // Color rows grouped by the UI_COLOR_FIELDS group label.
        let mut slots = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(6);
        let mut last_group = "";
        for (idx, (label, group)) in UI_COLOR_FIELDS.iter().enumerate() {
            if *group != last_group {
                if !last_group.is_empty() {
                    slots = slots.push(Space::new().height(8).boxed());
                }
                slots = slots.push(
                    text(*group).size(12).color(OryxisColors::t().text_secondary).boxed(),
                );
                slots = slots.push(Space::new().height(2).boxed());
                last_group = group;
            }
            slots = slots.push(ui_color_row(label, idx, &form.colors[idx]));
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
            ui_theme_preview(&form.colors),
        ];

        // Shared form chrome (buttons + error slot); the row itself
        // keeps the modal card's trailing alignment.
        let footer = column![
            crate::widgets::form_error(form.error.as_deref()),
            dir_row(vec![
                Space::new().width(Length::Fill).boxed(),
                crate::widgets::form_cancel_button(Message::Settings(SettingsMessage::UiThemeEditorClose)),
                Space::new().width(8).boxed(),
                crate::widgets::form_save_button(t("save"), Some(Message::Settings(SettingsMessage::UiThemeEditorSave))),
            ])
            .align_y(iced::Alignment::Center),
        ]
        .spacing(8);

        let scroll_form = scrollable(
            container(form_col)
                .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 0.0 }),
        )
        .height(Length::Fixed(400.0));

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
            .width(Length::Fixed(740.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
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
        .on_press(Message::Settings(SettingsMessage::UiThemeEditorClose))
        .boxed();
        let mut stack = iced::widget::Stack::<iced::Element<'_, _>>::new()
            .push(backdrop)
            .push(
                container::<_, iced::Theme>(card_trap)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill).boxed(),
            )
            .width(Length::Fill)
            .height(Length::Fill);
        if let Some((idx, anchor)) = self.ui_color_popover {
            stack = stack.push(self.ui_color_popover_view(&form.colors, idx, anchor));
        }
        iced::widget::opaque(stack).boxed()
    }

    fn ui_color_popover_view<'a>(
        &'a self,
        colors: &'a [String; 21],
        idx: usize,
        anchor: iced::Point,
    ) -> Element<'a, Message> {
        let color = col(&colors[idx], Color::BLACK);
        let hex = &colors[idx];
        let card = container(
            column![
                crate::color_picker::color_picker(color, move |hex| {
                    Message::Settings(SettingsMessage::UiThemeColorChanged(idx, hex))
                }),
                Space::new().height(10),
                text_input("#RRGGBB", hex)
                    .on_input(move |v| Message::Settings(SettingsMessage::UiThemeColorChanged(idx, v)))
                    .padding(7)
                    .size(12)
                    .style(crate::widgets::rounded_input_style),
                Space::new().height(10),
                ui_preset_grid(idx),
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
            ..Default::default()
        });
        let card_trap: Element<'_, Message> =
            MouseArea::new(card).on_press(Message::NoOp).boxed();

        const PW: f32 = 236.0;
        const PH: f32 = 330.0;
        let x = anchor.x.min((self.cur_window_size().width - PW).max(0.0)).max(0.0);
        let y = anchor.y.min((self.cur_window_size().height - PH).max(0.0)).max(0.0);
        let positioned = column![Space::new().height(y), row![Space::new().width(x), card_trap]];
        let pop_backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new()).width(Length::Fill).height(Length::Fill),
        )
        .on_press(Message::Settings(SettingsMessage::UiThemeEditorClosePicker))
        .boxed();
        iced::widget::Stack::<iced::Element<'_, _>>::new()
            .push(pop_backdrop)
            .push(positioned.boxed())
            .width(Length::Fill)
            .height(Length::Fill)
            .boxed()
    }
}

/// One UI color slot: label, clickable swatch (opens the picker), hex input.
fn ui_color_row<'a>(label: &'a str, idx: usize, hex: &'a str) -> Element<'a, Message> {
    let swatch_color = col(hex, Color::TRANSPARENT);
    let swatch = button(Space::new().width(22).height(22))
        .on_press(Message::Settings(SettingsMessage::UiThemeEditorOpenPicker(idx)))
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
            .width(Length::Fixed(140.0))
            .boxed(),
        swatch.boxed(),
        Space::new().width(10).boxed(),
        text_input("#RRGGBB", hex)
            .on_input(move |v| Message::Settings(SettingsMessage::UiThemeColorChanged(idx, v)))
            .padding(7)
            .size(12)
            .width(Length::Fixed(100.0))
            .style(crate::widgets::rounded_input_style)
            .boxed(),
    ])
    .align_y(iced::Alignment::Center)
    .boxed()
}

fn ui_preset_grid<'a>(idx: usize) -> Element<'a, Message> {
    let mut rows = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(5);
    let mut current = iced::widget::Row::<iced::Element<'_, _>>::new().spacing(5);
    let mut n = 0;
    for hex in crate::os_icon::PRESET_COLORS.iter() {
        let color = col(hex, Color::TRANSPARENT);
        let sw = button(Space::new().width(18).height(18))
            .on_press(Message::Settings(SettingsMessage::UiThemeColorChanged(idx, (*hex).to_string())))
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

/// A small mockup of the app chrome painted with the in-progress colors.
fn ui_theme_preview<'a>(c: &'a [String; 21]) -> Element<'a, Message> {
    let bg_primary = col(&c[0], Color::BLACK);
    let bg_sidebar = col(&c[1], Color::BLACK);
    let bg_surface = col(&c[2], Color::BLACK);
    let text_primary = col(&c[5], Color::WHITE);
    let text_secondary = col(&c[6], Color::WHITE);
    let text_muted = col(&c[7], Color::WHITE);
    let accent = col(&c[8], Color::WHITE);
    let success = col(&c[10], Color::WHITE);
    let warning = col(&c[11], Color::WHITE);
    let error = col(&c[12], Color::WHITE);
    let border = col(&c[16], Color::WHITE);
    let button_bg = col(&c[18], accent);
    let button_text = col(&c[20], Color::WHITE);

    let dot = |color: Color| -> Element<'a, Message> {
        container(Space::new().width(10).height(10))
            .style(move |_| container::Style {
                background: Some(Background::Color(color)),
                border: Border { radius: Radius::from(5.0), ..Default::default() },
                ..Default::default()
            })
            .boxed()
    };

    let surface_card = container(
        column![
            text("Connection").size(13).color(text_primary),
            text("user@host.example.com").size(11).color(text_secondary),
            text("last used 2h ago").size(10).color(text_muted),
            Space::new().height(8),
            row![
                container(text("Connect").size(11).color(button_text))
                    .padding(Padding { top: 5.0, right: 12.0, bottom: 5.0, left: 12.0 })
                    .style(move |_| container::Style {
                        background: Some(Background::Color(button_bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }),
                Space::new().width(8),
                dot(success),
                Space::new().width(4),
                dot(warning),
                Space::new().width(4),
                dot(error),
            ]
            .align_y(iced::Alignment::Center),
        ]
        .spacing(3),
    )
    .padding(12)
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(bg_surface)),
        border: Border { radius: Radius::from(8.0), color: border, width: 1.0 },
        ..Default::default()
    });

    let sidebar = container(
        column![
            text("Hosts").size(11).color(accent),
            Space::new().height(6),
            text("SFTP").size(11).color(text_secondary),
            Space::new().height(6),
            text("Settings").size(11).color(text_secondary),
        ],
    )
    .padding(10)
    .width(Length::Fixed(80.0))
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(bg_sidebar)),
        ..Default::default()
    });

    container(
        row![
            sidebar,
            container(surface_card).padding(12).width(Length::Fill),
        ],
    )
    .width(Length::Fixed(320.0))
    .height(Length::Fixed(180.0))
    .style(move |_| container::Style {
        background: Some(Background::Color(bg_primary)),
        border: Border { radius: Radius::from(8.0), color: border, width: 1.0 },
        ..Default::default()
    })
    .boxed()
}

/// A selectable app-theme card painted with the theme's own colors (name in
/// the theme foreground, accent + semantic dots on the right), mirroring the
/// terminal theme card so the card itself previews the theme. Used for both
/// built-in and custom UI themes in the Interface picker.
pub(crate) fn app_theme_card<'a>(
    name: &'a str,
    colors: &crate::theme::ThemeColors,
    is_active: bool,
    on_press: Message,
) -> Element<'a, Message> {
    // Same chassis as the terminal theme card; the dots are the chrome's
    // accent + semantic colors.
    let dots = vec![colors.accent, colors.success, colors.warning, colors.error];
    let tags = crate::theme_tags::card_tag_line(&crate::theme_tags::ui_theme_traits(colors));
    crate::widgets::theme_preview_card(
        name,
        &tags,
        colors.bg_primary,
        colors.text_primary,
        dots,
        is_active,
        on_press,
    )
}

/// "+ New custom theme" card for the Interface theme grid.
pub(crate) fn ui_theme_add_card<'a>() -> Element<'a, Message> {
    crate::widgets::theme_outline_card(
        iced_fonts::lucide::plus(),
        t("theme_new_custom"),
        OryxisColors::t().accent,
        Message::Settings(SettingsMessage::UiThemeEditorNew),
    )
}

/// "Import" card for the Interface theme grid: opens the paste-or-load
/// modal for the Oryxis UI theme JSON.
pub(crate) fn ui_theme_import_card<'a>() -> Element<'a, Message> {
    crate::widgets::theme_outline_card(
        iced_fonts::lucide::download(),
        t("theme_import"),
        OryxisColors::t().text_secondary,
        Message::Settings(SettingsMessage::UiThemeImportOpen),
    )
}

/// "Community themes" card: opens the gallery on the site, where every
/// contributed palette previews in its own colours. Deliberately a link
/// rather than an in-app browser (issue #118): the site page works
/// against the RELEASED app, because Copy there lands a scheme the
/// Import card here already accepts.
pub(crate) fn ui_theme_community_card<'a>() -> Element<'a, Message> {
    crate::widgets::theme_outline_card(
        iced_fonts::lucide::globe(),
        t("theme_community"),
        OryxisColors::t().accent,
        Message::OpenUrl("https://oryxis.app/themes".to_string()),
    )
}

impl Oryxis {
    /// A custom UI theme card with hover edit / delete affordances.
    pub(crate) fn ui_theme_custom_card<'a>(
        &'a self,
        idx: usize,
        name: &'a str,
        colors: &crate::theme::ThemeColors,
        is_active: bool,
    ) -> Element<'a, Message> {
        let card = app_theme_card(
            name,
            colors,
            is_active,
            Message::Settings(SettingsMessage::AppThemeChanged(name.to_string())),
        );
        let mut stack = iced::widget::Stack::<iced::Element<'_, _>>::new().push(card);
        if self.cur_nav().hover.ui_theme_card == Some(idx) {
            let actions = container(
                dir_row(vec![
                    ui_icon_btn(
                        iced_fonts::lucide::pencil(),
                        Message::Settings(SettingsMessage::UiThemeEditorEdit(idx)),
                        t("edit"),
                    ),
                    Space::new().width(4).boxed(),
                    ui_icon_btn(
                        iced_fonts::lucide::copy(),
                        Message::Settings(SettingsMessage::UiThemeClone(idx)),
                        t("duplicate"),
                    ),
                    Space::new().width(4).boxed(),
                    ui_icon_btn(
                        iced_fonts::lucide::upload(),
                        Message::Settings(SettingsMessage::UiThemeExport(idx)),
                        t("theme_export"),
                    ),
                    Space::new().width(4).boxed(),
                    ui_icon_btn(
                        iced_fonts::lucide::trash(),
                        Message::Settings(SettingsMessage::UiThemeDeleteRequested(idx)),
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
            .on_enter(Message::Settings(SettingsMessage::UiThemeCardHovered(idx)))
            .on_exit(Message::Settings(SettingsMessage::UiThemeCardUnhovered(idx)))
            .boxed()
    }

    /// A built-in app theme card in the Interface grid with a floating
    /// clone icon revealed on hover, so a preset can be duplicated into an
    /// editable custom UI theme in one click.
    pub(crate) fn ui_builtin_theme_card<'a>(
        &'a self,
        idx: usize,
        name: &'a str,
        colors: &crate::theme::ThemeColors,
        is_active: bool,
    ) -> Element<'a, Message> {
        let card = app_theme_card(
            name,
            colors,
            is_active,
            Message::Settings(SettingsMessage::AppThemeChanged(name.to_string())),
        );
        let mut stack = iced::widget::Stack::<iced::Element<'_, _>>::new().push(card);
        if self.cur_nav().hover.builtin_ui_theme_card == Some(idx) {
            let actions = container(
                dir_row(vec![
                    ui_icon_btn(
                        iced_fonts::lucide::copy(),
                        Message::Settings(SettingsMessage::UiThemeCloneBuiltin(idx)),
                        t("duplicate"),
                    ),
                    Space::new().width(4).boxed(),
                    ui_icon_btn(
                        iced_fonts::lucide::upload(),
                        Message::Settings(SettingsMessage::UiThemeExportBuiltin(idx)),
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
            .on_enter(Message::Settings(SettingsMessage::UiThemeBuiltinCardHovered(idx)))
            .on_exit(Message::Settings(SettingsMessage::UiThemeBuiltinCardUnhovered(idx)))
            .boxed()
    }

    /// Import-a-UI-theme modal: paste (or load from file) the Oryxis UI
    /// theme JSON; on import the parsed colors open in the editor for
    /// review. Mirrors `view_theme_import_modal`.
    pub(crate) fn view_ui_theme_import_modal(&self) -> Element<'_, Message> {
        let name_input = text_input(t("theme_name"), &self.ui_theme_import_name)
            .on_input(|v| Message::Settings(SettingsMessage::UiThemeImportNameChanged(v)))
            .padding(10)
            .size(13)
            .style(crate::widgets::rounded_input_style);

        let paste = container(
            iced::widget::text_editor(&self.ui_theme_import_content)
                .on_action(|v| Message::Settings(SettingsMessage::UiThemeImportContentAction(v)))
                .padding(10)
                .height(Length::Fixed(220.0))
                .font(iced::Font::MONOSPACE)
                .size(11),
        );

        let mut col = column![
            text(t("theme_import_title")).size(18).color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(t("theme_import_ui_hint")).size(12).color(OryxisColors::t().text_muted),
            Space::new().height(16),
            text(t("theme_name")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            name_input,
            Space::new().height(12),
            paste,
        ]
        .spacing(0);
        col = col.push(Space::new().height(8).boxed());
        col = col.push(crate::widgets::form_error(self.ui_theme_import_error.as_deref()));
        col = col.push(Space::new().height(8).boxed());
        col = col.push(
            dir_row(vec![
                crate::views::settings_themes::import_browse_button(
                    Message::Settings(SettingsMessage::UiThemeImportBrowse),
                ),
                Space::new().width(Length::Fill).boxed(),
                crate::widgets::form_cancel_button(Message::Settings(SettingsMessage::UiThemeImportClose)),
                Space::new().width(8).boxed(),
                crate::widgets::form_save_button(
                    t("theme_import"),
                    Some(Message::Settings(SettingsMessage::UiThemeImportApply)),
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

/// Small floating icon button used for the per-card actions, wrapped in a
/// tooltip per the icon-only convention.
fn ui_icon_btn<'a>(
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
            border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        }
    });
    crate::views::terminal::icon_tooltip(btn.boxed(), tip)
}

