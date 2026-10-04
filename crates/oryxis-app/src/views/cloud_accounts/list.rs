//! Cards list, the toolbar at the top of the Cloud Accounts panel
//! plus the responsive grid of `CloudProfile` cards. Empty state lives
//! here too. The wizard form panel is mounted on the right when
//! `cloud_form.visible` is on.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{SettingsMessage, CloudMessage, Message, Oryxis, TabsMessage, CARD_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{
    card_grid_columns, dir_align_x, dir_row, distribute_card_grid, panel_section,
    rounded_input_style,
};

impl Oryxis {
    pub(crate) fn view_cloud_accounts(&self) -> Element<'_, Message> {
        let primary: Element<'_, Message> = {
            let fg = OryxisColors::t().button_text;
            button(
                container(
                    dir_row(vec![
                        text("+")
                            .size(13)
                            .font(iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                            })
                            .color(fg)
                            .boxed(),
                        Space::new().width(4).boxed(),
                        text(t("cloud_new_account_btn"))
                            .size(11)
                            .font(iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                            })
                            .color(fg)
                            .boxed(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .center_y(Length::Fixed(24.0))
                .padding(Padding {
                    top: 0.0,
                    right: 14.0,
                    bottom: 0.0,
                    left: 14.0,
                }),
            )
            .on_press(Message::Cloud(CloudMessage::ShowCloudForm(None)))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                    _ => OryxisColors::t().button_bg,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(6.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .boxed()
        };
        // Responsive collapse: search yields first, then folds to an icon;
        // at the narrowest the action moves into the `…` overflow menu.
        // `keynav_toolbar_slot` records each rendered action for the
        // keyboard router (push order == visual order here).
        let (search_collapsed, buttons_overflow) = self.toolbar_tiers();
        self.keynav_toolbar_reset();
        let search_slot = self.vault_search_slot(search_collapsed);
        let search_slot = if search_collapsed {
            self.keynav_toolbar_slot(crate::keynav::ToolbarItem::SearchIcon, search_slot)
        } else {
            search_slot
        };
        let trailing: Element<'_, Message> = if buttons_overflow {
            self.keynav_toolbar_slot(
                crate::keynav::ToolbarItem::Overflow,
                crate::widgets::bounds_reporter(
                    crate::widgets::toolbar_overflow_icon(matches!(
                        self.overlay.as_ref().map(|o| &o.content),
                        Some(crate::state::OverlayContent::ToolbarOverflow)
                    )),
                    self.toolbar_overflow_btn_bounds.clone(),
                ),
            )
        } else {
            self.keynav_toolbar_slot(crate::keynav::ToolbarItem::Primary, primary)
        };
        let toolbar = container(
            dir_row(vec![
                // Search fills the leading space (hidden + Fill spacer when
                // there are no accounts, so the action stays trailing).
                search_slot,
                Space::new().width(10).boxed(),
                trailing,
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding {
            top: 16.0,
            right: 24.0,
            bottom: 16.0,
            left: 24.0,
        })
        .width(Length::Fill);

        let main_content = if !self.any_cloud_provider_installed() {
            // No cloud-provider plugin installed: a static explainer
            // (what Cloud Accounts are for + a route to the Plugins
            // panel to install a provider). Accounts can't function
            // without a provider plugin, so the list and +Account are
            // intentionally replaced rather than shown empty.
            let explainer = container(
                column![
                    container(
                        iced_fonts::lucide::cloud()
                            .size(32)
                            .color(OryxisColors::t().text_muted),
                    )
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border {
                            radius: Radius::from(12.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text(t("cloud_no_provider_title"))
                        .size(20)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text(t("cloud_no_provider_desc"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    crate::widgets::cta_button(
                        t("cloud_no_provider_btn").to_string(),
                        // The route, not the bare section: this button is
                        // pressed from the Cloud panel, and
                        // `ChangeSettingsSection` assumes Settings is
                        // already on screen, so it would select the
                        // section behind a view nobody switched to.
                        Message::Tabs(TabsMessage::OpenSettingsSection(
                            crate::state::SettingsSection::Plugins,
                        )),
                    ),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            // The explainer replaces the toolbar entirely; un-record
            // the items registered above. Same for content rows.
            self.keynav_toolbar_reset();
            self.keynav_clear_content();
            column![explainer]
                .width(Length::Fill)
                .height(Length::Fill)
        } else if self
            .cloud_profiles
            .iter()
            .all(|p| !self.cloud_provider_installed(&p.provider))
        {
            // At least one provider plugin is installed, but no account
            // belongs to an installed provider (none saved, or all
            // saved accounts target a provider whose plugin was
            // removed). Show the regular empty state + toolbar.
            let empty_state = crate::widgets::empty_state(
                iced_fonts::lucide::cloud()
                    .size(32)
                    .color(OryxisColors::t().text_muted)
                    .boxed(),
                t("cloud_empty_title").to_string(),
                t("cloud_empty_desc").to_string(),
                Some((
                    t("cloud_new_account_btn").to_string(),
                    Message::Cloud(CloudMessage::ShowCloudForm(None)),
                )),
            );

            // No toolbar when empty: search is hidden and the "+ Account"
            // lives in the empty-state CTA (avoids an orphaned button).
            // Un-record the toolbar items; none of them render here.
            // Same for content rows.
            self.keynav_toolbar_reset();
            self.keynav_clear_content();
            column![empty_state]
                .width(Length::Fill)
                .height(Length::Fill)
        } else {
            let mut cards: Vec<Element<'_, Message>> = Vec::new();
            // Keyboard-navigation order, collected as the cards render
            // so it always matches the filtered set on screen.
            let mut cloud_nav: Vec<crate::keynav::NavItem> = Vec::new();
            let needle = self.cur_nav().cloud_search.trim().to_lowercase();
            // Hide accounts whose provider plugin isn't installed; they
            // stay in the vault and reappear when the plugin is back.
            // Also apply the toolbar search needle (label / provider match).
            for cp in self
                .cloud_profiles
                .iter()
                .filter(|p| self.cloud_provider_installed(&p.provider))
                .filter(|p| {
                    needle.is_empty()
                        || p.label.to_lowercase().contains(&needle)
                        || p.provider.to_lowercase().contains(&needle)
                })
            {
                // Brand glyph + brand colour from the bundled SVG set.
                // The icon tile keeps a neutral surface bg so the brand
                // colour reads on the glyph itself instead of fighting
                // with a saturated coloured square.
                let (glyph, brand_color) =
                    crate::os_icon::provider_icon(&cp.provider, OryxisColors::t().accent);
                // Match the host/group cards: a filled avatar in the
                // user's chosen icon shape, brand colour fill, white logo,
                // instead of a one-off bordered surface box.
                let host_style = crate::widgets::resolve_host_icon_style(
                    None,
                    &self.prefs.default_host_icon,
                );
                let icon_box = crate::widgets::host_icon(
                    host_style,
                    brand_color,
                    &cp.label,
                    Some(glyph.view(18.0, Color::WHITE)),
                    32.0,
                );

                let provider_label = match cp.provider.as_str() {
                    "aws" => "AWS",
                    "k8s" => "Kubernetes",
                    "gcp" => "GCP",
                    "azure" => "Azure",
                    "aliyun" => "Alibaba Cloud",
                    "tencent" => "Tencent Cloud",
                    other => other,
                };

                let cp_id = cp.id;
                cloud_nav.push(crate::keynav::NavItem::CloudAccount(cp_id));
                let kb_selected = self.kn().selected_in(crate::keynav::FocusZone::Content)
                    == Some(crate::keynav::NavItem::CloudAccount(cp_id));
                // Floating ⋮ kebab in a Stack overlay (trailing corner)
                // so it doesn't take inline width inside the dir_row.
                // Always-mounted with a transparent glyph + no-hover bg
                // when not active so the surrounding MouseArea sees
                // stable child bounds (avoids hover event loop).
                let show_dots = self.hover.cloud_card == Some(cp_id) || kb_selected;
                let rtl = crate::i18n::is_rtl_layout();
                let pad_trailing = 30.0_f32;
                let card_padding = if rtl {
                    Padding { top: 16.0, right: 16.0, bottom: 16.0, left: pad_trailing }
                } else {
                    Padding { top: 16.0, right: pad_trailing, bottom: 16.0, left: 16.0 }
                };

                let card_body = container(
                    dir_row(vec![
                        icon_box,
                        Space::new().width(12).boxed(),
                        column![
                            text(&cp.label)
                                .size(13)
                                .color(OryxisColors::t().text_primary)
                                .wrapping(iced::widget::text::Wrapping::None),
                            Space::new().height(2),
                            text(format!("{} · {}", provider_label, cp.auth_kind))
                                .size(10)
                                .color(OryxisColors::t().text_muted)
                                .wrapping(iced::widget::text::Wrapping::None),
                        ]
                        .width(Length::Fill)
                        .align_x(crate::widgets::dir_align_x())
                        .clip(true)
                        .boxed(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .padding(card_padding)
                .width(Length::Fill)
                .style(move |_: &iced::Theme| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border {
                        radius: Radius::from(10.0),
                        // Accent border on hover, matching host / key cards.
                        color: if show_dots {
                            OryxisColors::t().accent
                        } else {
                            OryxisColors::t().border
                        },
                        width: 1.0,
                    },
                    ..Default::default()
                });

                let dots_glyph_color = if show_dots {
                    OryxisColors::t().text_muted
                } else {
                    Color::TRANSPARENT
                };
                let dots_btn = crate::widgets::card_kebab_button(
                    dots_glyph_color,
                    show_dots,
                    Message::Cloud(CloudMessage::ShowCloudCardMenu(cp_id)),
                );
                let dots_align = if rtl {
                    iced::alignment::Horizontal::Left
                } else {
                    iced::alignment::Horizontal::Right
                };
                let dots_pad = if rtl {
                    Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 8.0 }
                } else {
                    Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 0.0 }
                };
                let dots_overlay = container::<_, iced::Theme>(dots_btn)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(dots_align)
                    .align_y(iced::alignment::Vertical::Center)
                    .padding(dots_pad);
                let card_element: Element<'_, Message> = iced::widget::Stack::<iced::Element<'_, _>>::new()
                    .push(card_body.boxed())
                    .push(dots_overlay.boxed())
                    .boxed();

                let wrapped = MouseArea::new(card_element)
                    .on_enter(Message::Cloud(CloudMessage::CloudCardHovered(cp_id)))
                    .on_exit(Message::Cloud(CloudMessage::CloudCardUnhovered(cp_id)))
                    .on_right_press(Message::Cloud(CloudMessage::ShowCloudCardMenu(cp_id)));

                let card_el: Element<'_, Message> =
                    container(wrapped).width(Length::Fill).clip(true).boxed();
                let card_el = self.card_wash(card_el, brand_color);
                cards.push(self.keynav_ring_content(kb_selected, card_el));
            }

            let nav_width = self.vault_rail_width();
            let panel_width = if self.cloud_form.visible { self.panel_width } else { 0.0 };
            let available =
                (self.cur_window_size().width
                - nav_width
                - self.side_strip_reserve()
                - panel_width
                - 48.0)
                .max(0.0);
            let cols = card_grid_columns(available, CARD_WIDTH, 12.0);
            // Chunk the keyboard order to the same column count the
            // grid renders with.
            self.keynav_set_content_rows(
                cloud_nav.chunks(cols.max(1)).map(|c| c.to_vec()).collect(),
            );
            let cloud_grid = distribute_card_grid(cards, cols, 12.0, 12.0);

            // Cloud Sync settings (auto-refresh / orphan archive) moved
            // to Settings -> Cloud (`view_cloud_sync_settings`); this
            // surface is now just the account grid.
            let grid = scrollable(
                column![cloud_grid].padding(Padding {
                    top: 0.0,
                    right: 24.0,
                    bottom: 24.0,
                    left: 24.0,
                }),
            )
            // Stable id so the keyboard router can keep the selected
            // card scrolled into view.
            .id(iced::widget::Id::new("cloud-accounts-scroll"))
            .height(Length::Fill);

            column![toolbar, grid]
                .width(Length::Fill)
                .height(Length::Fill)
        };

        // The account form panel is hoisted to `view_main`
        // (active_side_panel) so it rises over the sub-nav band.
        main_content.boxed()
    }

    /// Cloud Sync preferences (auto-refresh interval, orphan
    /// auto-archive). Lives in Settings -> Cloud; the cloud *account*
    /// CRUD moved to the top-level `View::Cloud` surface. Interval /
    /// days inputs accept partial typed input and clamp on commit via
    /// the sanitize helper in the dispatcher.
    pub(crate) fn view_cloud_sync_settings(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order. The raw inputs
        // below carry no recording; the slot wraps happen inside the
        // panel column so construction order equals on-screen order.
        // Input ids are static, the fork's widget::Id only takes
        // &'static str.
        self.keynav_settings_reset();
        let refresh_interval_input = text_input(
            "30",
            &self.prefs.cloud_auto_refresh_interval_minutes,
        )
        .id(iced::widget::Id::new("set-cloud-refresh-interval"))
        .on_input(|v| Message::Settings(SettingsMessage::SettingCloudAutoRefreshIntervalChanged(v)))
        .padding(8)
        .width(120)
        .style(rounded_input_style)
        .align_x(dir_align_x());
        let orphan_days_input = text_input(
            "7",
            &self.prefs.cloud_orphan_archive_days,
        )
        .id(iced::widget::Id::new("set-cloud-orphan-days"))
        .on_input(|v| Message::Settings(SettingsMessage::SettingCloudOrphanArchiveDaysChanged(v)))
        .padding(8)
        .width(120)
        .style(rounded_input_style)
        .align_x(dir_align_x());
        let cloud_sync_settings = panel_section(column![
            // Title dropped (redundant with the settings nav label).
            self.nav_toggle_row(
                t("settings_cloud_auto_refresh"),
                self.prefs.cloud_auto_refresh_enabled,
                Message::Settings(SettingsMessage::SettingCloudAutoRefreshToggle),
            ),
            Space::new().height(8),
            dir_row(vec![
                text(t("settings_cloud_auto_refresh_interval"))
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .boxed(),
                Space::new().width(Length::Fill).boxed(),
                self.settings_nav_slot_labeled(
                    t("settings_cloud_auto_refresh_interval"),
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "set-cloud-refresh-interval",
                    )),
                    10.0,
                    refresh_interval_input.boxed(),
                ),
            ])
            .align_y(iced::Alignment::Center),
            Space::new().height(14),
            self.nav_toggle_row(
                t("settings_cloud_auto_archive"),
                self.prefs.cloud_auto_archive_orphans,
                Message::Settings(SettingsMessage::SettingCloudAutoArchiveToggle),
            ),
            Space::new().height(8),
            dir_row(vec![
                text(t("settings_cloud_orphan_archive_days"))
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .boxed(),
                Space::new().width(Length::Fill).boxed(),
                self.settings_nav_slot_labeled(
                    t("settings_cloud_orphan_archive_days"),
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "set-cloud-orphan-days",
                    )),
                    10.0,
                    orphan_days_input.boxed(),
                ),
            ])
            .align_y(iced::Alignment::Center),
        ]);

        scrollable(
            container(cloud_sync_settings)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 })
                .width(Length::Fill),
        )
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-cloud-scroll"))
        .height(Length::Fill)
        .boxed()
    }
}
