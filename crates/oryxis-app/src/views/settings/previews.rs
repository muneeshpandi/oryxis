//! Live appearance previews shown inside Settings -> Interface: the
//! tab-strip preview and the dashboard host-card preview. Split out of
//! views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    /// Live preview of the tab strip under the current appearance
    /// settings. Mirrors `active_tab_bg` and the top-bar wash in
    /// `tab_bar.rs` so what the user sees here matches the real strip
    /// as they toggle: fill style (gradient/solid), the accent underline,
    /// the top-bar wash, and the connection status dot. Sample tab labels
    /// are literal demo content (same convention as the font preview).
    pub(crate) fn tab_appearance_preview(&self) -> Element<'_, Message> {
        // The demo tab pretends to be an Ubuntu host: a brand colour
        // clearly distinct from every shipped app accent, so the
        // host-vs-app accent picker and the text toggle visibly change
        // the preview (with the app accent they were invisible no-ops).
        let host_demo = Color::from_rgb8(0xE9, 0x54, 0x20);
        let accent = if self.host_accent_enabled() {
            host_demo
        } else {
            OryxisColors::t().accent
        };
        // Same contrast validation the real strip applies (issue #79)
        // before the accent renders as text or fill.
        let text_accent =
            crate::theme::readable_accent_on(accent, OryxisColors::t().bg_sidebar);
        let label_color = if self.prefs.tab_accent_text {
            text_accent
        } else {
            OryxisColors::t().text_primary
        };
        let solid = self.prefs.tab_fill_style == "solid";
        // Reuse the real strip's fill helper so the preview can never
        // drift from what `tab_bar.rs` actually paints.
        let active_bg = crate::views::tab_bar::active_tab_bg(text_accent, solid);
        // Connection status dot: the same green "connected" cue. Only
        // present (with its trailing gap) when the dot setting is on.
        let mut active_row: Vec<Element<'_, Message>> = Vec::new();
        if self.prefs.show_tab_status_dot {
            active_row.push(
                container(Space::new().width(6).height(6))
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().success)),
                        border: Border { radius: Radius::from(3.0), ..Default::default() },
                        ..Default::default()
                    })
                    .boxed(),
            );
            active_row.push(Space::new().width(6).boxed());
        }
        active_row.push(text("production-web").size(12).color(label_color).boxed());
        let active_tab = container(
            dir_row(active_row).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 7.0, right: 12.0, bottom: 7.0, left: 12.0 })
        .style(move |_| container::Style {
            background: Some(active_bg),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        });
        let idle_tab = container(
            text("staging-db").size(12).color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 7.0, right: 12.0, bottom: 7.0, left: 12.0 });
        // Bottom hairline: 2 px accent when the underline tint is on,
        // else the neutral 1 px chrome border (mirrors `view_main`).
        let (line_h, line_color) = if self.prefs.tab_accent_line {
            (2.0_f32, accent)
        } else {
            (1.0_f32, OryxisColors::t().border)
        };
        let hairline = container(Space::new().width(Length::Fill).height(line_h))
            .style(move |_| container::Style {
                background: Some(Background::Color(line_color)),
                ..Default::default()
            });
        // Top-bar wash, identical direction + mix to the real strip.
        let bar_base = OryxisColors::t().bg_sidebar;
        let bar_bg: Background = if self.prefs.tab_accent_wash {
            let washed = crate::theme::mix(bar_base, accent, 0.16);
            Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
                    .add_stop(0.0, washed)
                    .add_stop(0.9, bar_base),
            ))
        } else {
            Background::Color(bar_base)
        };
        let strip = container(
            dir_row(vec![
                active_tab.boxed(),
                Space::new().width(4).boxed(),
                idle_tab.boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 })
        .style(move |_| container::Style {
            background: Some(bar_bg),
            ..Default::default()
        });
        column![strip, hairline].width(Length::Fill).boxed()
    }

    /// Live preview of the status bar under the current settings, with
    /// sample values. Mirrors `view_status_bar`'s per-element toggles
    /// (connection, latency, grid size, cwd, vitals, version) so what
    /// the user sees here matches the real bar; a toggle flip must move
    /// BOTH renders or the card is lying.
    pub(crate) fn status_bar_preview(&self) -> Element<'_, Message> {
        let top_hairline = container(Space::new().height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            });
        let vital = |label: String, value: &'static str| -> Element<'static, Message> {
            dir_row(vec![
                text(label).size(11).color(OryxisColors::t().text_muted).boxed(),
                Space::new().width(4).boxed(),
                text(value)
                    .size(11)
                    .color(OryxisColors::t().text_secondary)
                    .boxed(),
            ])
            .align_y(iced::Alignment::Center)
            .boxed()
        };
        // Same spacer placement as the real bar: `status_bar_align_left`
        // parks the content on the PHYSICAL left edge (not flipped by
        // RTL, see `view_status_bar`), else the cluster trails.
        let align_left = self.prefs.status_bar_align_left;
        let spacer_leads = align_left && crate::i18n::is_rtl_layout();
        let mut items: Vec<Element<'_, Message>> = Vec::new();
        if spacer_leads {
            items.push(Space::new().width(Length::Fill).boxed());
        }
        if self.prefs.status_show_connection {
            items.push(
                text(format!(
                    "● production-web, {}",
                    crate::i18n::t("status_bar_connected")
                ))
                .size(12)
                .color(OryxisColors::t().success)
                .boxed(),
            );
            if align_left {
                items.push(Space::new().width(16).boxed());
            }
        }
        if !align_left {
            items.push(Space::new().width(Length::Fill).boxed());
        }
        if self.prefs.status_show_latency {
            items.push(vital(crate::i18n::t("status_latency").into(), "23 ms"));
            items.push(Space::new().width(12).boxed());
        }
        if self.prefs.status_show_dimensions {
            items.push(vital(crate::i18n::t("status_dimensions").into(), "120×32"));
            items.push(Space::new().width(12).boxed());
        }
        if self.prefs.status_show_cwd {
            items.push(vital(crate::i18n::t("status_cwd").into(), "~/projects/api"));
            items.push(Space::new().width(12).boxed());
        }
        if self.prefs.monitor_status_bar {
            items.push(vital(crate::i18n::t("monitor_cpu").into(), "12%"));
            items.push(Space::new().width(12).boxed());
            items.push(vital(crate::i18n::t("monitor_mem").into(), "38%"));
            items.push(Space::new().width(12).boxed());
            items.push(vital(crate::i18n::t("monitor_net").into(), "↓1.2M/s ↑340K/s"));
            items.push(Space::new().width(12).boxed());
        }
        if self.prefs.status_show_version {
            items.push(
                text(concat!("Oryxis v", env!("CARGO_PKG_VERSION")))
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .boxed(),
            );
        }
        if align_left && !spacer_leads {
            items.push(Space::new().width(Length::Fill).boxed());
        }
        let bar = container(
            dir_row(items)
                .align_y(iced::Alignment::Center)
                .padding(Padding { top: 3.0, right: 12.0, bottom: 3.0, left: 12.0 }),
        )
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
            ..Default::default()
        });
        column![top_hairline, bar].width(Length::Fill).boxed()
    }

    /// Live preview of a dashboard host card under the current dashboard
    /// settings: the default host icon shape, the optional address line,
    /// and the accent glass wash. Reuses `host_icon` and
    /// `card_accent_wash` so it tracks the real card exactly. Sample host
    /// name / address are literal demo content (like the font preview).
    pub(crate) fn card_appearance_preview(&self) -> Element<'_, Message> {
        let accent = OryxisColors::t().accent;
        let style = crate::widgets::resolve_host_icon_style(None, &self.prefs.default_host_icon);
        let icon = crate::widgets::host_icon(
            style,
            accent,
            "production-web",
            Some(iced_fonts::lucide::server().size(16).color(Color::WHITE).boxed()),
            32.0,
        );
        let mut text_col = column![
            text("production-web").size(13).color(OryxisColors::t().text_primary),
        ];
        if self.prefs.show_host_address {
            text_col = text_col
                .push(Space::new().height(2).boxed())
                .push(
                    text("deploy@10.0.0.4")
                        .size(11)
                        .color(OryxisColors::t().text_muted).boxed(),
                );
        }
        let card = container(
            dir_row(vec![
                icon,
                Space::new().width(10).boxed(),
                text_col.width(Length::Fill).align_x(dir_align_x()).boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(10.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });
        let card_el: Element<'_, Message> = card.boxed();
        if self.prefs.card_accent_glass {
            crate::widgets::card_accent_wash(card_el, accent)
        } else {
            card_el
        }
    }
}
