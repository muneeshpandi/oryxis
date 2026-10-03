//! Dashboard grid: session cards. Split out of views/dashboard/grid/mod.rs.

use super::*;
impl Oryxis {
    /// One session-group card: primary click opens the saved arrangement;
    /// hovering reveals floating edit / delete icons (the per-card action
    /// convention). Distinct `boxes` glyph + the group's own color set it
    /// apart from host cards.
    pub(crate) fn session_group_card<'a>(
        &'a self,
        idx: usize,
        group: &'a oryxis_core::models::SessionGroup,
    ) -> (Element<'a, Message>, Color) {
        let rtl = crate::i18n::is_rtl_layout();
        let hovered = self.hover.session_group_card == Some(idx);
        let bg_color = group
            .color
            .as_deref()
            .and_then(crate::os_icon::parse_hex_color)
            .unwrap_or_else(|| OryxisColors::t().accent);

        // Custom icon when the user picked one, else the default group glyph.
        let glyph = group
            .icon_style
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(crate::os_icon::custom_icon_glyph)
            .unwrap_or(BrandIcon::Glyph(iced_fonts::lucide::boxes()));
        let host_style = crate::widgets::resolve_host_icon_style(
            None,
            &self.prefs.default_host_icon,
        );
        let icon_box = crate::widgets::host_icon(
            host_style,
            bg_color,
            &group.label,
            Some(glyph.view(18.0, Color::WHITE)),
            32.0,
        );

        let panes = count_leaves(&group.layout);
        let subtitle = format!("{} {}", panes, t("session_group_panes"));
        let label_el = text::<iced::Theme>(group.label.clone())
            .size(13)
            .color(OryxisColors::t().text_primary)
            .wrapping(iced::widget::text::Wrapping::None);
        let subtitle_el = text::<iced::Theme>(subtitle)
            .size(10)
            .color(OryxisColors::t().text_muted)
            .wrapping(iced::widget::text::Wrapping::None);

        // Same padding as the manual-folder card: 2 px leading so the
        // icon lines up with folder / host cards in the shared grid,
        // 24 px trailing to reserve the kebab / chevron overlay slot.
        let card_padding = if rtl {
            Padding { top: 8.0, right: 2.0, bottom: 8.0, left: 24.0 }
        } else {
            Padding { top: 8.0, right: 24.0, bottom: 8.0, left: 2.0 }
        };
        let card_btn = button(
            container(
                dir_row(vec![
                    icon_box,
                    Space::new().width(8).boxed(),
                    iced::widget::Column::<iced::Element<'_, _>>::with_children(vec![
                        label_el.boxed(),
                        Space::new().height(2).boxed(),
                        subtitle_el.boxed(),
                    ])
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .clip(true)
                    .boxed(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .padding(card_padding),
        )
        .on_press(Message::SessionGroup(SessionGroupMessage::OpenSessionGroup(idx)))
        .width(Length::Fill)
        .style(move |_, status| {
            let (bg, bc, bw) = match status {
                BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(10.0), color: bc, width: bw },
                ..Default::default()
            }
        });

        // `⋮` kebab → context menu (Open / Edit / Duplicate / Delete), same
        // as the host card. Shown on hover or while this card's menu is open.
        let menu_open = matches!(
            self.overlay.as_ref().map(|o| &o.content),
            Some(crate::state::OverlayContent::SessionGroupActions(i)) if *i == idx
        );
        let show_dots = hovered || menu_open;
        let dots_glyph_color = if show_dots {
            OryxisColors::t().text_muted
        } else {
            Color::TRANSPARENT
        };
        let dots_btn = crate::widgets::card_kebab_button(
            dots_glyph_color,
            show_dots,
            Message::SessionGroup(SessionGroupMessage::ShowSessionGroupMenu(idx)),
        );
        // Idle shows a muted chevron (this card opens into a restored
        // session, the same "opens a container" affordance the host-group
        // folders use); hover / menu-open swaps it for the ⋮ kebab.
        let trailing: Element<'a, Message> = if show_dots {
            dots_btn.boxed()
        } else {
            let chevron = if rtl {
                iced_fonts::lucide::chevron_left()
            } else {
                iced_fonts::lucide::chevron_right()
            };
            // Center the idle chevron in the same 22×22 box the hover
            // ⋮ uses, so idle and hover share a center (no jitter),
            // matching the manual-folder card.
            container(chevron.size(14).color(OryxisColors::t().text_muted))
                .center_x(Length::Fixed(22.0))
                .center_y(Length::Fixed(22.0))
                .boxed()
        };
        let card_element = crate::widgets::card_trailing_overlay(card_btn.boxed(), trailing);

        let wrapped = MouseArea::new(card_element)
            .on_enter(Message::SessionGroup(SessionGroupMessage::SessionGroupCardHovered(idx)))
            .on_exit(Message::SessionGroup(SessionGroupMessage::SessionGroupCardUnhovered(idx)))
            .on_right_press(Message::SessionGroup(SessionGroupMessage::ShowSessionGroupMenu(idx)));

        (
            container(wrapped).width(Length::Fill).clip(true).boxed(),
            bg_color,
        )
    }
}
