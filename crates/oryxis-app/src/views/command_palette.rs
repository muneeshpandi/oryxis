//! Command palette modal (C4): a VS Code-style `Ctrl+Shift+P` fuzzy
//! search over every action. Structurally a sibling of `tab_jump.rs`
//! (single query input + a filtered, keyboard-navigable row list); the
//! rows come from `Oryxis::palette_rows`, each carrying its own resolved
//! `Message` so activation never re-derives by index.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{TabsMessage, Message, Oryxis};
use crate::i18n::t;
use crate::palette::{PaletteCategory, PaletteRow, PALETTE_INPUT_ID};
use crate::theme::{OryxisColors, SYSTEM_UI_SEMIBOLD};
use crate::widgets::{dir_align_x, dir_row};

/// Leading glyph for a category, tooltipped with the category name so
/// the glyph doubles as a legend.
fn category_glyph(cat: PaletteCategory) -> Element<'static, Message> {
    let color = OryxisColors::t().text_muted;
    let g = match cat {
        PaletteCategory::Tabs => iced_fonts::lucide::layout_grid(),
        PaletteCategory::Vault => iced_fonts::lucide::folder(),
        PaletteCategory::Terminal => iced_fonts::lucide::terminal(),
        PaletteCategory::Settings => iced_fonts::lucide::settings(),
        PaletteCategory::Session => iced_fonts::lucide::shield(),
    };
    let glyph: Element<'static, Message> = container(g.size(13).color(color))
        .center_x(Length::Fixed(20.0))
        .center_y(Length::Fixed(20.0))
        .boxed();
    crate::views::terminal::icon_tooltip(glyph, t(cat.label_key()))
}

impl Oryxis {
    pub(crate) fn view_command_palette(&self) -> Element<'_, Message> {
        // Rows are recorded (enabled ones) in visual order; Up/Down move
        // the ring, Enter activates the selection-or-top-match.
        self.modal_nav_reset();
        let rows = self.palette_rows(&self.palette.query);

        // ── Result list ────────────────────────────────────────────────
        let body: Element<'_, Message> = if rows.is_empty() {
            container(
                text(t("command_palette_no_results"))
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(20)
            .boxed()
        } else {
            let mut col = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(2);
            for row in rows {
                col = col.push(self.palette_row(row));
            }
            scrollable(col.padding(Padding {
                top: 0.0,
                right: 6.0,
                bottom: 0.0,
                left: 0.0,
            }))
            .id(iced::widget::Id::new("palette-scroll"))
            .height(Length::Fixed(420.0))
            .boxed()
        };

        // ── Search header ──────────────────────────────────────────────
        let search_input = text_input(t("command_palette_placeholder"), &self.palette.query)
            .id(iced::widget::Id::new(PALETTE_INPUT_ID))
            .on_input(|v| Message::Tabs(TabsMessage::PaletteQueryChanged(v)))
            .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
            .size(13)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x());

        let pill: Element<'_, Message> = container(
            text(t("command_palette_title"))
                .size(11)
                .color(OryxisColors::t().accent)
                .font(SYSTEM_UI_SEMIBOLD),
        )
        .padding(Padding { top: 3.0, right: 8.0, bottom: 3.0, left: 8.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(Color {
                a: 0.15,
                ..OryxisColors::t().accent
            })),
            border: Border {
                radius: Radius::from(10.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .boxed();
        // Rendered from the LIVE binding map like the row chips, so macOS
        // shows Cmd and a rebind stays correct; unbound drops the hint.
        let shortcut_hint: Element<'_, Message> = match self
            .hotkey_bindings
            .get(&crate::hotkeys::HotkeyAction::ShowCommandPalette)
            .and_then(|b| b.badges())
            .map(|badges| badges.join("+"))
        {
            Some(badge) if !badge.is_empty() => text(badge)
                .size(11)
                .color(OryxisColors::t().text_muted)
                .boxed(),
            _ => Space::new().boxed(),
        };

        let search_header = container(
            dir_row(vec![
                iced_fonts::lucide::search()
                    .size(13)
                    .color(OryxisColors::t().text_muted)
                    .boxed(),
                Space::new().width(8).boxed(),
                pill,
                Space::new().width(8).boxed(),
                container(search_input).width(Length::Fill).boxed(),
                Space::new().width(12).boxed(),
                shortcut_hint,
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 4.0, right: 14.0, bottom: 4.0, left: 14.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border {
                radius: Radius::from(8.0),
                ..Default::default()
            },
            ..Default::default()
        });

        let dialog = container(
            column![search_header, Space::new().height(6), body]
                .padding(12)
                .width(Length::Fixed(560.0)),
        )
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
        });

        // Bare card; `widgets::modal_overlay` (the caller) owns centering,
        // the absorbing scrim, and the click-trap.
        dialog.boxed()
    }

    fn palette_row<'a>(&self, row: PaletteRow) -> Element<'a, Message> {
        let colors = OryxisColors::t();
        let label_color = if row.enabled {
            colors.text_primary
        } else {
            colors.text_muted
        };

        // Right-aligned hotkey chip, rendered from the LIVE binding map so
        // a rebind shows correctly.
        let chip: Element<'a, Message> = match row
            .hotkey
            .and_then(|a| self.hotkey_bindings.get(&a))
            // The chip shows the action's primary chord: an action can
            // carry several, and the palette has room for one.
            .and_then(|b| b.badges())
            .map(|badges| badges.join("+"))
        {
            Some(badge) if !badge.is_empty() => container(
                text(badge).size(10).color(colors.text_muted),
            )
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .style(move |_| container::Style {
                background: Some(Background::Color(colors.bg_hover)),
                border: Border {
                    radius: Radius::from(4.0),
                    ..Default::default()
                },
                ..Default::default()
            })
            .boxed(),
            _ => Space::new().boxed(),
        };

        let content = dir_row(vec![
            category_glyph(row.category),
            Space::new().width(8).boxed(),
            text(row.label).size(13).color(label_color).boxed(),
            Space::new().width(Length::Fill).boxed(),
            chip,
        ])
        .align_y(iced::Alignment::Center);

        // Disabled rows list for discoverability but are inert: greyed,
        // no button on_press, and NOT recorded as a keynav slot (the ring
        // skips them), so neither mouse nor keyboard can activate them.
        if !row.enabled {
            return container(content)
                .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
                .width(Length::Fill)
                .boxed();
        }

        let msg = row.message.clone();
        let btn: Element<'a, Message> = button(content)
            .on_press_with(move || Message::Tabs(TabsMessage::PaletteActivate(Box::new(msg.clone()))))
            .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
            .width(Length::Fill)
            .style(move |_, status| {
                let bg = match status {
                    BtnStatus::Hovered | BtnStatus::Pressed => {
                        Background::Color(OryxisColors::t().bg_hover)
                    }
                    _ => Background::Color(Color::TRANSPARENT),
                };
                button::Style {
                    background: Some(bg),
                    border: Border {
                        radius: Radius::from(6.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .boxed();

        self.modal_nav_slot(
            crate::keynav::RowAction::activate(Message::Tabs(TabsMessage::PaletteActivate(Box::new(
                row.message,
            )))),
            6.0,
            false,
            btn,
        )
    }
}
