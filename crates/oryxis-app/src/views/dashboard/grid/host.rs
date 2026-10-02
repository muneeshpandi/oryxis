//! Dashboard grid: host cards. Split out of views/dashboard/grid/mod.rs.

use super::*;
impl Oryxis {
    /// Host cards for the dashboard grid, in the resolved display order.
    pub(crate) fn dashboard_host_cards(&self) -> Vec<(Element<'_, Message>, Color, DashNavItem)> {
        let host_order = self.dashboard_host_order();
        // One terms pass for every card: the redactor below runs per
        // label and must not rebuild the hostname list per row.
        let privacy_terms = self.privacy_terms();
        host_order
            .into_iter()
            .map(|idx| {
                let (element, color) = self.dashboard_host_card(idx, &privacy_terms);
                (element, color, DashNavItem::Host(idx))
            })
            .collect()
    }

    /// One host card, exactly as the grid / list renders it (icon
    /// badge, privacy redaction, hover kebab, right-click menu,
    /// connect on press). Shared with the tree view mode, which
    /// indents the same card per level.
    pub(crate) fn dashboard_host_card<'a>(
        &'a self,
        idx: usize,
        privacy_terms: &[String],
    ) -> (Element<'a, Message>, Color) {
        let conn = &self.connections[idx];
        // Privacy Mode also redacts the LABEL (issue #78): labels
        // routinely embed the hostname or IP, which made the card
        // leak what the subtitle mask hides. Same hover reveal as
        // the address; the icon badge takes the redacted label too
        // so Initials style can't leak the leading letters.
        let display_label = if self.privacy_active(conn) && self.hover.card != Some(idx) {
            crate::widgets::redact_for_display(&conn.label, privacy_terms, self.privacy_classes())
        } else {
            conn.label.clone()
        };
        let is_connected = self.tabs.iter().any(|t| t.label == conn.label);
        let auth_label = crate::util::auth_method_label(&conn.auth_method);
        // The login the dial will use (folder default, identity), not
        // the raw field: it names the card and hints the OS badge. A
        // host inheriting its user from the folder showed root here
        // while dialling as the folder said (issue #242).
        let username = self.effective_username(conn);
        // Address shown only when the (off-by-default) setting is on,
        // so addresses stay out of screenshots / screen shares by
        // default. Port 22 is the SSH default, so it's always omitted.
        let subtitle = if self.prefs.show_host_address {
            use oryxis_core::models::connection::ConnectionProtocol;
            // Shared with the tab strip's second line, so the two
            // surfaces render the same address for the same host.
            let address = crate::util::host_address_label(conn, username.as_deref());
            // Privacy Mode masks the address behind muted blocks,
            // revealed when the card is hovered. The auth method label
            // is not sensitive, so it stays readable.
            let address = if self.privacy_active(conn) && self.hover.card != Some(idx) {
                crate::widgets::mask_blocks(&address)
            } else {
                address
            };
            // Serial has no auth method to append; the line params
            // shown in `address` are the whole subtitle. A remote
            // desktop shows its kind (RDP/VNC) instead of an SSH auth
            // method.
            match conn.protocol {
                // Serial, Raw and Local have no auth method to append:
                // what `address` already shows (line params, endpoint,
                // shell) is the whole subtitle.
                ConnectionProtocol::Serial
                | ConnectionProtocol::Raw
                | ConnectionProtocol::Local => address,
                ConnectionProtocol::RemoteDesktop => {
                    format!("{} · {}", address, conn.rd_kind)
                }
                _ => format!("{} · {}", address, auth_label),
            }
        } else {
            auth_label.to_string()
        };

        // Resolve icon + brand color from detected OS (if any). Disconnected
        // hosts use the app accent; connected ones use the brand color or
        // success green as fallback.
        let default_fallback = if is_connected {
            OryxisColors::t().success
        } else {
            OryxisColors::t().accent
        };
        let (os_glyph, icon_color) = crate::os_icon::resolve_for(
            conn.detected_os.as_deref(),
            conn.custom_icon.as_deref(),
            conn.custom_color.as_deref(),
            username.as_deref(),
            default_fallback,
        );
        // Fixed 32x32 badge. Shape and color come from the per-host
        // override (icon_style + color) when set; otherwise fall back
        // to the global default_host_icon setting and the OS-derived
        // brand color. Initials style ignores the glyph and renders
        // the leading letters of the label instead.
        let host_style = crate::widgets::resolve_host_icon_style(
            conn.icon_style.as_deref(),
            &self.prefs.default_host_icon,
        );
        let badge_color = conn.custom_color.as_deref()
            .or(conn.color.as_deref())
            .and_then(crate::widgets::parse_hex_color)
            .unwrap_or(icon_color);
        let glyph_el: Element<'_, Message> = os_glyph.view(18.0, Color::WHITE);
        let icon_box = crate::widgets::host_icon(
            host_style,
            badge_color,
            &display_label,
            Some(glyph_el),
            32.0,
        );

        // Floating ⋮ kebab: lives in a Stack overlay on the trailing
        // corner so it doesn't take inline width inside the dir_row.
        // The card reserves a fixed trailing pad so subtitles never
        // collide with the overlay, geometry stays constant regardless
        // of hover state. The button itself is always mounted (so the
        // surrounding MouseArea sees stable child bounds, no hover
        // event loop) and just toggles its glyph color + hover bg.
        let show_dots = self.hover.card == Some(idx)
            || (self.card_context_menu.is_some()
                && self.card_context_menu == self.connections.get(idx).map(|c| c.id));
        // Multi-selection (issue #230): the card wears the selected
        // fill and accent border whenever it is in the selection, and its
        // leading check appears ONLY while the multi-select mode is on.
        // The check is the mode's chrome, not a rule of the grid: outside
        // the mode the selection already reads through the card's own
        // fill, and a box on every host of a list nobody put into
        // selection mode is noise. It is deliberately not tied to hover
        // either: the check and the gaps around it take 34 px of row
        // width, so a hover-driven one would slide every label sideways
        // under a moving cursor.
        let selected = self.dash_selection.contains(conn.id);
        let checking = self.dash_multi_select;
        let dragging_this = self
            .card_drag
            .as_ref()
            .is_some_and(|d| d.active && d.ids.contains(&conn.id));
        let rtl = crate::i18n::is_rtl_layout();
        let pad_trailing = 24.0_f32;
        let card_padding = if rtl {
            Padding { top: 8.0, right: 2.0, bottom: 8.0, left: pad_trailing }
        } else {
            Padding { top: 8.0, right: pad_trailing, bottom: 8.0, left: 2.0 }
        };

        // Cloud-origin badge: small brand glyph that used to sit
        // inline with the label (and got clipped on long names).
        // Moved to the LEADING edge of the subtitle row so it
        // never competes with the title for horizontal space.
        // Stored as (brand_key, badge_color, is_orphan) so the
        // glyph can be re-resolved at the use site instead of
        // moved out of a shared tuple (`BrandIcon::view` consumes
        // self and `BrandIcon` doesn't impl Clone).
        let cloud_decoration: Option<(&'static str, Color, bool)> =
            conn.cloud_ref.as_ref().map(|cr| {
                let provider = self
                    .cloud_profiles
                    .iter()
                    .find(|p| p.id == cr.profile_id)
                    .map(|p| p.provider.as_str())
                    .unwrap_or("cloud");
                let brand_key = crate::os_icon::provider_brand_key(provider);
                let is_orphan = cr.orphaned_at.is_some();
                let (_brand_glyph, brand_color_default) = crate::os_icon::provider_icon(
                    brand_key,
                    OryxisColors::t().accent,
                );
                let badge_color = if is_orphan {
                    OryxisColors::t().text_muted
                } else {
                    brand_color_default
                };
                (brand_key, badge_color, is_orphan)
            });

        let label_color = match &cloud_decoration {
            Some((_, _, true)) => OryxisColors::t().text_muted,
            _ => OryxisColors::t().text_primary,
        };
        let label_el: Element<'_, Message> = if let Some((_, _, true)) = &cloud_decoration {
            // Orphan: keep the pill next to the label so the user
            // sees it at the title's eye level.
            let muted = OryxisColors::t().text_muted;
            let pill = container(
                text(t("host_orphan_label"))
                    .size(9)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding {
                top: 1.0,
                right: 6.0,
                bottom: 1.0,
                left: 6.0,
            })
            .style(move |_| container::Style {
                background: Some(Background::Color(Color { a: 0.10, ..muted })),
                border: Border {
                    radius: Radius::from(6.0),
                    color: Color { a: 0.30, ..muted },
                    width: 1.0,
                },
                ..Default::default()
            });
            dir_row(vec![
                text(display_label.clone())
                    .size(13)
                    .color(label_color)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
                Space::new().width(6).into(),
                pill.into(),
            ])
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            text(display_label.clone())
                .size(13)
                .color(label_color)
                .wrapping(iced::widget::text::Wrapping::None)
                .into()
        };

        // Subtitle row carries the brand badge on its leading edge
        // when this host is cloud-sourced. Manual hosts get just
        // the subtitle text (no leading gap).
        let subtitle_el: Element<'_, Message> = match &cloud_decoration {
            Some((brand_key, color, _)) => {
                let glyph = crate::os_icon::custom_icon_glyph(brand_key);
                dir_row(vec![
                    glyph.view(10.0, *color),
                    Space::new().width(6).into(),
                    text(subtitle)
                        .size(10)
                        .color(OryxisColors::t().text_muted)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .into(),
                ])
                .align_y(iced::Alignment::Center)
                .into()
            }
            None => text(subtitle)
                .size(10)
                .color(OryxisColors::t().text_muted)
                .wrapping(iced::widget::text::Wrapping::None)
                .into(),
        };

        // The row's leading cells. The selection check lives IN the row,
        // in front of the icon badge (issue #230): it never covers the
        // host's own icon, it lines up with the icon the way a list
        // control's checkbox does, and the inset keeps it clear of the
        // card's rounded corner. Its press is its own button - iced hands
        // an event to the innermost widget first and `Button::update`
        // forwards to its content before reacting, so the card never sees
        // a press that landed on the check.
        let mut row_cells: Vec<Element<'_, Message>> = Vec::new();
        if checking {
            row_cells.push(Space::new().width(6).into());
            row_cells.push(crate::widgets::card_select_check(
                selected,
                Message::Tabs(TabsMessage::CardSelectToggle(conn.id)),
            ));
            row_cells.push(Space::new().width(10).into());
        }
        row_cells.push(icon_box);
        row_cells.push(Space::new().width(8).into());
        row_cells.push(
            iced::widget::Column::with_children(vec![
                label_el,
                Space::new().height(2).into(),
                subtitle_el,
            ])
            .width(Length::Fill)
            .align_x(crate::widgets::dir_align_x())
            .clip(true)
            .into(),
        );

        // A card in an active drag loses its press: the fork's `button`
        // publishes on a release only while `on_press` is set in that
        // frame, so a drag abandoned back over its own card cannot dial
        // the host (the guard is the tree, not the timing between the
        // global release and the button).
        let card_btn = button(
            container(dir_row(row_cells).align_y(iced::Alignment::Center))
                .padding(card_padding),
        )
        .on_press_maybe(
            (!dragging_this).then_some(Message::Tabs(TabsMessage::CardPressed(idx))),
        )
        .width(Length::Fill)
        .style(move |_, status| {
            let bg = match status {
                _ if selected => OryxisColors::t().bg_selected,
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                BtnStatus::Pressed => OryxisColors::t().bg_selected,
                _ => OryxisColors::t().bg_surface,
            };
            // Same rounded card in grid and list mode: list mode is just
            // a single column with a small gap (History-style rows), so
            // each card stays independently rounded (radius matches the
            // accent wash) instead of a connected divider list. The
            // keyboard-selection highlight is drawn as an outer ring in
            // the assembly, not here.
            let (bc, bw) = match status {
                _ if selected => (OryxisColors::t().accent, 1.5),
                BtnStatus::Hovered => (OryxisColors::t().accent, 1.5),
                BtnStatus::Pressed => (OryxisColors::t().accent, 2.0),
                _ => (OryxisColors::t().border, 1.0),
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(10.0), color: bc, width: bw },
                ..Default::default()
            }
        });

        let dots_glyph_color = if show_dots {
            OryxisColors::t().text_muted
        } else {
            Color::TRANSPARENT
        };
        let dots_btn = crate::widgets::card_kebab_button(
            dots_glyph_color,
            show_dots,
            Message::Tabs(TabsMessage::ShowCardMenu(idx)),
        );
        let card_element =
            crate::widgets::card_trailing_overlay(card_btn.into(), dots_btn.into());
        // Which card a press lands on, for the global press handler to
        // arm a drag from: the button captures the press, and a
        // scrolled grid makes draw-time rects wrong by the scroll
        // offset, so the answer is recorded where iced translates the
        // cursor (the SFTP rows' lesson, issue #127).
        let card_element =
            crate::widgets::press_hit_reporter(card_element, self.card_press.clone(), conn.id);

        // Wrap in MouseArea for hover tracking and right-click
        let wrapped = MouseArea::new(card_element)
            .on_enter(Message::Tabs(TabsMessage::CardHovered(idx)))
            .on_exit(Message::Tabs(TabsMessage::CardUnhovered(idx)))
            .on_right_press(Message::Tabs(TabsMessage::ShowCardMenu(idx)));

        (
            Element::from(container(wrapped).width(Length::Fill).clip(true)),
            badge_color,
        )
    }
}
