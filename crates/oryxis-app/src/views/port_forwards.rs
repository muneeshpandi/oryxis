//! Port forwards (standalone tunnels) list and editor panel. Each row
//! carries an on/off toggle that opens / tears down a dedicated PTY-less
//! SSH session; the runtime state lives in `Oryxis::active_forwards`.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, checkbox, column, container, pick_list, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::port_forward_rule::{ForwardKind, LocalBind, PortForwardRule};

use crate::app::{SshMessage, NavigationMessage, PortForwardMessage, Message, Oryxis, CARD_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{card_grid_columns, dir_align_x, dir_row, distribute_card_grid};

/// Human-readable one-line summary of a rule, kind-aware. Shared with the
/// delete confirmation, which has to name the rule the way its row does or
/// the user cannot tell whether they picked the right one.
pub(crate) fn forward_summary(rule: &PortForwardRule) -> String {
    match rule.kind {
        ForwardKind::Local => format!(
            "{}:{} \u{2192} {}:{}",
            rule.listen_host, rule.listen_port, rule.target_host, rule.target_port
        ),
        ForwardKind::Remote => format!(
            "{}:{} \u{2190} {}:{}",
            rule.listen_host, rule.listen_port, rule.target_host, rule.target_port
        ),
        ForwardKind::Dynamic => {
            format!("SOCKS {}:{}", rule.listen_host, rule.listen_port)
        }
    }
}

impl Oryxis {
    pub(crate) fn view_port_forwards(&self) -> Element<'_, Message> {
        let primary: Element<'_, Message> = {
            let fg = OryxisColors::t().button_text;
            button(
                container(
                    dir_row(vec![
                        text("+").size(13).font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        }).color(fg).boxed(),
                        Space::new().width(4).boxed(),
                        text(t("port_forward_btn")).size(11).font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        }).color(fg).boxed(),
                    ]).align_y(iced::Alignment::Center),
                )
                .center_y(Length::Fixed(24.0))
                .center_x(Length::Fixed(72.0)),
            )
            .on_press(Message::PortForward(PortForwardMessage::ShowPortForwardPanel))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                    _ => OryxisColors::t().button_bg,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            }).boxed()
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
                        self.cur_overlay().map(|o| &o.content),
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
                search_slot,
                Space::new().width(10).boxed(),
                trailing,
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let status: Element<'_, Message> = if let Some(err) = &self.port_forward_form.error {
            container(text(err.clone()).size(12).color(OryxisColors::t().error).boxed())
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 }).boxed()
        } else {
            Space::new().boxed()
        };

        if self.port_forward_rules.is_empty() {
            let empty_state = crate::widgets::empty_state(
                iced_fonts::lucide::route()
                    .size(32)
                    .color(OryxisColors::t().text_muted)
                    .boxed(),
                t("create_port_forward_title").to_string(),
                t("create_port_forward_desc").to_string(),
                Some((
                    t("new_port_forward").to_string(),
                    Message::PortForward(PortForwardMessage::ShowPortForwardPanel),
                )),
            );

            // No toolbar in the empty state: the search is hidden (nothing
            // to search) and the "+ New" lives in the empty-state CTA, so a
            // toolbar would only show an orphaned action button.
            // Side panel hoisted to `view_main` (active_side_panel).
            // Un-record the toolbar items; none of them render here.
            // Same for content rows.
            self.keynav_toolbar_reset();
            self.keynav_clear_content();
            let main_content = column![status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);
            return main_content.boxed();
        }

        let needle = self.cur_nav().port_forward_search.to_lowercase();
        let mut cards: Vec<Element<'_, Message>> = Vec::new();
        // Keyboard-navigation order, collected as the cards render so
        // it always matches the filtered set on screen.
        let mut pf_nav: Vec<crate::keynav::NavItem> = Vec::new();
        for (idx, rule) in self.port_forward_rules.iter().enumerate() {
            let host_label = self
                .connections
                .iter()
                .find(|c| c.id == rule.host_id)
                .map(|c| c.label.clone())
                .unwrap_or_else(|| t("pf_unknown_host").to_string());
            if !needle.is_empty()
                && !rule.label.to_lowercase().contains(&needle)
                && !forward_summary(rule).to_lowercase().contains(&needle)
                && !host_label.to_lowercase().contains(&needle)
            {
                continue;
            }

            pf_nav.push(crate::keynav::NavItem::PortForward(idx));
            let kb_selected = self.kn().selected_in(crate::keynav::FocusZone::Content)
                == Some(crate::keynav::NavItem::PortForward(idx));

            let active = self.active_forwards.contains_key(&rule.id);
            let starting = self.port_forward_starting.contains(&rule.id);
            // Down but scheduled to be re-attempted (auto_start self-healing):
            // distinct from a plain "off" so the user knows it's working on it.
            let retrying =
                !active && !starting && self.port_forward_retry.contains_key(&rule.id);

            // Status dot: accent-green while up, amber while retrying, muted
            // while down.
            let dot_color = if active {
                OryxisColors::t().success
            } else if retrying {
                OryxisColors::t().warning
            } else {
                OryxisColors::t().text_muted
            };
            let glyph_el: Element<'_, Message> = iced_fonts::lucide::route()
                .size(14)
                .line_height(1.0)
                .color(Color::WHITE)
                .boxed();
            let icon_style = crate::widgets::resolve_host_icon_style(
                None,
                &self.prefs.default_host_icon,
            );
            let icon_box = crate::widgets::host_icon(
                icon_style,
                dot_color,
                &rule.label,
                Some(glyph_el),
                32.0,
            );

            // Trailing on/off toggle. Nested inside the card button (the
            // fork lets the inner press win), so clicking the toggle does
            // not also open the editor.
            let (toggle_label, toggle_msg, toggle_bg, toggle_fg) = if starting {
                (t("pf_starting"), None, OryxisColors::t().bg_surface, OryxisColors::t().text_muted)
            } else if retrying {
                // Clicking cancels the self-healing retry loop (turns it off
                // for good until the user starts it again).
                (
                    t("pf_retrying"),
                    Some(Message::PortForward(PortForwardMessage::StopPortForward(rule.id))),
                    OryxisColors::t().bg_surface,
                    OryxisColors::t().warning,
                )
            } else if active {
                (
                    t("pf_on"),
                    Some(Message::PortForward(PortForwardMessage::StopPortForward(rule.id))),
                    OryxisColors::t().success,
                    Color::WHITE,
                )
            } else {
                (
                    t("pf_off"),
                    Some(Message::PortForward(PortForwardMessage::StartPortForward(rule.id))),
                    OryxisColors::t().bg_surface,
                    OryxisColors::t().text_secondary,
                )
            };
            let mut toggle = button(
                container(text(toggle_label).size(11).color(toggle_fg))
                    .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
            )
            .style(move |_, st| {
                let bg = match st {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => toggle_bg,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
            if let Some(msg) = toggle_msg {
                toggle = toggle.on_press(msg);
            }

            // Hover-revealed kebab, the app-wide card affordance (host,
            // snippet, key and session-group cards all use it). This card
            // used to carry a bare trash glyph instead, which both broke
            // the convention and put the one destructive action on the
            // card's only visible control. The menu carries Edit as well,
            // even though clicking the card already edits: a card whose
            // menu offers only "Delete" reads like deletion is all you can
            // do from here. Stays mounted while its menu is open so the
            // pointer can travel to it.
            const DOTS_SLOT_W: f32 = 22.0;
            let show_dots = self.cur_nav().hover.port_forward_card == Some(idx)
                || self.port_forward_context_menu == Some(idx)
                || kb_selected;
            let dots: Element<'_, Message> = if show_dots {
                crate::widgets::card_kebab_button(
                    OryxisColors::t().text_muted,
                    true,
                    Message::PortForward(PortForwardMessage::ShowPortForwardMenu(idx)),
                )
                .boxed()
            } else {
                Space::new().width(Length::Fixed(DOTS_SLOT_W)).height(Length::Fixed(22.0)).boxed()
            };

            let kind_badge = format!("{}  \u{00B7}  {}", rule.kind, host_label);

            let card_btn = button(
                container(
                    dir_row(vec![
                        icon_box,
                        Space::new().width(8).boxed(),
                        column![
                            text(&rule.label)
                                .size(13)
                                .color(OryxisColors::t().text_primary)
                                .wrapping(iced::widget::text::Wrapping::None),
                            Space::new().height(2),
                            text(forward_summary(rule))
                                .size(10)
                                .color(OryxisColors::t().text_muted)
                                .font(iced::Font::MONOSPACE)
                                .wrapping(iced::widget::text::Wrapping::None),
                            Space::new().height(2),
                            text(kind_badge)
                                .size(9)
                                .color(OryxisColors::t().text_secondary)
                                .wrapping(iced::widget::text::Wrapping::None),
                        ].width(Length::Fill).boxed(),
                        toggle.boxed(),
                        Space::new().width(4).boxed(),
                        dots,
                    ]).align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 8.0, right: 2.0, bottom: 8.0, left: 2.0 }),
            )
            .on_press(Message::PortForward(PortForwardMessage::EditPortForwardRule(idx)))
            .width(Length::Fill)
            .style(move |_, st| {
                let (bg, bc, bw) = match st {
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

            // Right-click opens the same kebab menu, the app-wide rule.
            let wrapped: Element<'_, Message> = MouseArea::new(card_btn)
                .on_enter(Message::PortForward(PortForwardMessage::PortForwardCardHovered(idx)))
                .on_exit(Message::PortForward(PortForwardMessage::PortForwardCardUnhovered(idx)))
                .on_right_press(Message::PortForward(PortForwardMessage::ShowPortForwardMenu(idx)))
                .boxed();
            let card_el: Element<'_, Message> =
                container(wrapped).width(Length::Fill).clip(true).boxed();
            cards.push(crate::widgets::select_ring_opt(
                card_el,
                10.0,
                kb_selected.then(|| OryxisColors::t().accent),
            ));
        }

        let nav_width = self.vault_rail_width();
        let panel_width = if self.panels.port_forward_panel { self.panel_width } else { 0.0 };
        let available = (self.cur_window_size().width
            - nav_width
            - self.side_strip_reserve()
            - panel_width
            - 48.0)
            .max(0.0);
        let cols = card_grid_columns(available, CARD_WIDTH, 12.0);
        // Chunk the keyboard order to the same column count the grid
        // renders with.
        self.keynav_set_content_rows(
            pf_nav.chunks(cols.max(1)).map(|c| c.to_vec()).collect(),
        );
        let grid_widget = distribute_card_grid(cards, cols, 12.0, 12.0);
        let grid = scrollable(
            column![grid_widget].padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        // Stable id so the keyboard router can keep the selected card
        // scrolled into view.
        .id(iced::widget::Id::new("port-forwards-scroll"))
        .height(Length::Fill);

        // Inline search in Classic mode (Workspace puts it on the sub-nav).
        // Search now lives in the toolbar (`vault_search_field`); the
        // legacy below-toolbar search bar collapses to nothing.
        let search_bar: Element<'_, Message> = Space::new().boxed();

        // Side panel hoisted to `view_main` (active_side_panel).
        column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill)
            .boxed()
    }

    pub(crate) fn view_port_forward_panel<'a>(&'a self) -> Element<'a, Message> {
        // Keyboard rows are recorded in visual order (row mode: Up/Down from any input).
        // The pickers below are slot-wrapped at their usage point inside the
        // form column (not at construction) so recording follows the on-screen
        // order: name, kind, host, listen fields, target fields, toggle.
        self.panel_nav_reset();
        let is_editing = self.port_forward_form.editing_id.is_some();
        let title = if is_editing { t("edit_port_forward") } else { t("new_port_forward") };

        let panel_header = container(
            dir_row(vec![
                text(title).size(18).color(OryxisColors::t().text_primary).boxed(),
                Space::new().width(Length::Fill).boxed(),
                crate::widgets::panel_close_button(Message::PortForward(PortForwardMessage::HidePortForwardPanel)).boxed(),
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Kind picker. All three directions are implemented. Focusable
        // select: Tab reaches it, Enter/Space open it, the widget owns
        // arrows/Esc while focused (fork support).
        let kind_options = ForwardKind::ALL.to_vec();
        let kind_picker = pick_list(Some(self.port_forward_form.kind), kind_options, |k: &ForwardKind| k.to_string())
            .on_select(|v| Message::PortForward(PortForwardMessage::PfKindChanged(v)))
            .id(iced::widget::Id::new("panel-pf-kind"))
            .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
            .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style);

        // Host picker. Options are connection labels; the on_select closure
        // resolves the label back to the connection id.
        let host_options: Vec<String> = self.connections.iter().map(|c| c.label.clone()).collect();
        let selected_host_label = self
            .port_forward_form.host_id
            .and_then(|id| self.connections.iter().find(|c| c.id == id))
            .map(|c| c.label.clone());
        let host_lookup: std::collections::HashMap<String, uuid::Uuid> = self
            .connections
            .iter()
            .map(|c| (c.label.clone(), c.id))
            .collect();
        let host_picker = pick_list(selected_host_label, host_options, |s: &String| s.clone())
            .on_select(move |label: String| {
                Message::PortForward(PortForwardMessage::PfHostChanged(host_lookup.get(&label).copied().unwrap_or_default()))
            })
            .id(iced::widget::Id::new("panel-pf-host"))
            .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
            .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style);

        // `id` doubles as the focus id of the keyboard row. `value` and
        // `placeholder` carry the element lifetime: post-refactor
        // `text_input` borrows its fragments instead of copying.
        let label_field = |label: &str, value: &'a str, placeholder: &'a str, id: &'static str, on_input: fn(String) -> Message| {
            column![
                text(label.to_string()).size(12).color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(id)),
                    10.0,
                    text_input(placeholder, value)
                        .id(iced::widget::Id::new(id))
                        .on_input(on_input)
                        .size(13)
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .boxed(),
                ),
            ]
        };

        let mut form = column![
            label_field(t("name"), &self.port_forward_form.label, "my-db-tunnel", "panel-pf-name", |v| Message::PortForward(PortForwardMessage::PfLabelChanged(v))),
            Space::new().height(14),
            text(t("pf_kind")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("panel-pf-kind")),
                10.0,
                kind_picker.boxed(),
            ),
            Space::new().height(14),
            text(t("pf_host")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("panel-pf-host")),
                10.0,
                host_picker.boxed(),
            ),
            Space::new().height(14),
            label_field(t("pf_listen_host"), &self.port_forward_form.listen_host, "127.0.0.1", "panel-pf-listen-host", |v| Message::PortForward(PortForwardMessage::PfListenHostChanged(v))),
            Space::new().height(14),
            label_field(t("pf_listen_port"), &self.port_forward_form.listen_port, "8080", "panel-pf-listen-port", |v| Message::PortForward(PortForwardMessage::PfListenPortChanged(v))),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Target fields hidden for Dynamic (the SOCKS client picks the dest).
        // Their keyboard rows record here too, only when rendered.
        if self.port_forward_form.kind.has_target() {
            form = form
                .push(Space::new().height(14).boxed())
                .push(label_field(t("pf_target_host"), &self.port_forward_form.target_host, "10.0.0.5", "panel-pf-target-host", |v| Message::PortForward(PortForwardMessage::PfTargetHostChanged(v))).boxed())
                .push(Space::new().height(14).boxed())
                .push(label_field(t("pf_target_port"), &self.port_forward_form.target_port, "5432", "panel-pf-target-port", |v| Message::PortForward(PortForwardMessage::PfTargetPortChanged(v))).boxed());
        }

        // Remote bind on 0.0.0.0 needs `GatewayPorts yes` on the server.
        if self.port_forward_form.kind == ForwardKind::Remote && self.port_forward_form.listen_host.trim() == "0.0.0.0" {
            form = form
                .push(Space::new().height(10).boxed())
                .push(text(t("gateway_ports_hint")).size(11).color(OryxisColors::t().warning).boxed());
        }

        // A dynamic SOCKS forward is an unauthenticated proxy. Bound to a
        // non-loopback address it becomes an open proxy into the remote
        // network for anyone who can reach this host. Warn explicitly.
        let exposed = !LocalBind::parse(&self.port_forward_form.listen_host).is_loopback();
        if self.port_forward_form.kind == ForwardKind::Dynamic && exposed {
            form = form
                .push(Space::new().height(10).boxed())
                .push(text(t("socks_open_proxy_hint")).size(11).color(OryxisColors::t().warning).boxed());
        }

        form = form
            .push(Space::new().height(14).boxed())
            .push(self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::PortForward(PortForwardMessage::PfAutoStartToggled(
                    !self.port_forward_form.auto_start,
                ))),
                8.0,
                checkbox(self.port_forward_form.auto_start)
                    .label(t("pf_auto_start"))
                    .on_toggle(|v| Message::PortForward(PortForwardMessage::PfAutoStartToggled(v)))
                    .size(16)
                    .text_size(12)
                    .boxed(),
            ));

        // Document the self-healing behavior so the KeePassXC-key-not-ready
        // ordering problem has an in-product answer.
        if self.port_forward_form.auto_start {
            form = form
                .push(Space::new().height(6).boxed())
                .push(text(t("pf_auto_start_hint")).size(11).color(OryxisColors::t().text_muted).boxed());
        }

        // While editing an existing rule, Delete keeps its own
        // outlined-danger row inside the body (it is not part of the
        // Cancel/Save pair).
        let mut body = column![form];
        if let Some(edit_id) = self.port_forward_form.editing_id
            && let Some(idx) = self.port_forward_rules.iter().position(|r| r.id == edit_id)
        {
            let del_btn = self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::PortForward(PortForwardMessage::RequestDeletePortForwardRule(idx))),
                8.0,
                button(
                    container(text(t("delete")).size(13).color(OryxisColors::t().error))
                        .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                        .width(Length::Fill).center_x(Length::Fill),
                )
                .on_press(Message::PortForward(PortForwardMessage::RequestDeletePortForwardRule(idx)))
                .width(Length::Fill)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::t().error, width: 1.0 },
                    ..Default::default()
                })
                .boxed(),
            );
            body = body.push(Space::new().height(20).boxed());
            body = body.push(del_btn);
        }

        // Shared form chrome: error outside the scrollable, above the
        // footer, so it stays visible regardless of scroll position.
        let panel_error = crate::widgets::form_error(self.port_forward_form.error.as_deref());
        let footer = crate::widgets::form_footer(
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::PortForward(PortForwardMessage::HidePortForwardPanel)),
                6.0,
                crate::widgets::form_cancel_button(Message::PortForward(PortForwardMessage::HidePortForwardPanel)),
            ),
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::PortForward(PortForwardMessage::SavePortForwardRule)),
                6.0,
                crate::widgets::form_save_button(t("save"), Some(Message::PortForward(PortForwardMessage::SavePortForwardRule))),
            ),
        );

        let panel_content = column![
            panel_header,
            scrollable(
                container(body.width(Length::Fill).align_x(dir_align_x()))
                    .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 20.0 }),
            )
            // Shared id: the keyboard router keeps the selected row in view.
            .id(iced::widget::Id::new("side-panel-scroll"))
            .height(Length::Fill),
            panel_error,
            footer,
        ].height(Length::Fill);

        crate::widgets::side_panel_frame(panel_content.boxed(), OryxisColors::t().bg_sidebar, self.panel_width)
    }

    /// Standalone host-key verification modal, used when a backgrounded
    /// action (a manually toggled port forward) hits an unknown / changed
    /// key and there is no connect-progress screen to host the prompt
    /// inline. Reuses the same `SshHostKey*` messages as the terminal flow.
    pub(crate) fn view_host_key_modal(&self) -> Element<'_, Message> {
        let Some(query) = self.pending_host_key.as_ref() else {
            return Space::new().boxed();
        };
        let is_changed = matches!(query.status, oryxis_ssh::HostKeyStatus::Changed { .. });
        let title = if is_changed { t("hk_warning_title") } else { t("hk_unknown_title") };
        let title_color = if is_changed { OryxisColors::t().error } else { OryxisColors::t().warning };

        let mut body = column![
            text(title).size(16).color(title_color),
            Space::new().height(10),
        ];
        if is_changed {
            body = body
                .push(text(t("hk_warning_desc")).size(13).color(OryxisColors::t().error).boxed())
                .push(Space::new().height(8).boxed());
            if let oryxis_ssh::HostKeyStatus::Changed { old_fingerprint } = &query.status {
                body = body
                    .push(
                        text(format!("{} {}", t("hk_old_fingerprint"), old_fingerprint))
                            .size(12)
                            .color(OryxisColors::t().text_muted).boxed(),
                    )
                    .push(Space::new().height(8).boxed());
            }
        }
        body = body
            .push(text(format!("{}:{}", query.hostname, query.port)).size(13).color(OryxisColors::t().text_secondary).boxed())
            .push(Space::new().height(8).boxed())
            .push(text(format!("{} SHA256:", query.key_type)).size(12).color(OryxisColors::t().text_secondary).boxed())
            .push(Space::new().height(4).boxed())
            .push(text(&query.fingerprint).size(13).color(OryxisColors::t().text_primary).font(iced::Font::MONOSPACE).boxed())
            .push(Space::new().height(14).boxed())
            .push(text(t("hk_add_question")).size(13).color(OryxisColors::t().text_secondary).boxed())
            .push(Space::new().height(18).boxed());

        let card = container(column![body, self.host_key_buttons()].width(Length::Fill))
            .width(Length::Fixed(480.0))
            .padding(24)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(12.0) },
                ..Default::default()
            });

        // Bare card; `widgets::modal_overlay` (the caller) centers + scrims.
        card.boxed()
    }

    /// Reject / continue once / add-and-continue, in that order, shared
    /// by the standalone modal above and the connect-progress inline
    /// prompt: two copies of a security prompt's buttons are two chances
    /// for one of them to wire an answer wrong (the proxy-command
    /// prompt's rationale). The three buttons are the modal keynav rows
    /// (`SurfaceFamily::Confirm`), and REJECT is the default row: a
    /// stray Enter must never trust a host key.
    pub(crate) fn host_key_buttons(&self) -> Element<'_, Message> {
        self.modal_nav_reset();
        let close_btn = button(
            container(text(t("close")).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 10.0, right: 20.0, bottom: 10.0, left: 20.0 }),
        )
        .on_press(Message::Ssh(SshMessage::SshHostKeyReject))
        .style(|_, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered | button::Status::Pressed => OryxisColors::t().bg_hover,
                _ => OryxisColors::t().bg_surface,
            })),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });
        let continue_btn = button(
            container(text(t("hk_continue")).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 10.0, right: 20.0, bottom: 10.0, left: 20.0 }),
        )
        .on_press(Message::Ssh(SshMessage::SshHostKeyContinue))
        .style(|_, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered | button::Status::Pressed => OryxisColors::t().bg_hover,
                _ => OryxisColors::t().bg_surface,
            })),
            border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        });
        let accept_fg = crate::theme::contrast_text_for(OryxisColors::t().success);
        let accept_btn = button(
            container(
                text(t("hk_add_and_continue"))
                    .size(13)
                    .font(iced::Font {
                        weight: iced::font::Weight::Semibold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    })
                    .color(accept_fg),
            )
            .padding(Padding { top: 10.0, right: 20.0, bottom: 10.0, left: 20.0 }),
        )
        .on_press(Message::Ssh(SshMessage::SshHostKeyAcceptAndSave))
        .style(|_, status| button::Style {
            background: Some(Background::Color(match status {
                // No dedicated success_hover in the palette; nudge the
                // fill toward the text colour, which lightens on dark
                // themes and darkens on light ones.
                button::Status::Hovered | button::Status::Pressed => crate::theme::mix(
                    OryxisColors::t().success,
                    OryxisColors::t().text_primary,
                    0.15,
                ),
                _ => OryxisColors::t().success,
            })),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        use crate::keynav::RowAction;
        dir_row(vec![
            self.modal_nav_slot_default(
                RowAction::activate(Message::Ssh(SshMessage::SshHostKeyReject)),
                8.0,
                false,
                close_btn.boxed(),
            ),
            Space::new().width(8).boxed(),
            self.modal_nav_slot(
                RowAction::activate(Message::Ssh(SshMessage::SshHostKeyContinue)),
                8.0,
                false,
                continue_btn.boxed(),
            ),
            Space::new().width(Length::Fill).boxed(),
            // Success-filled: the ring needs the contrast colour or it
            // vanishes into the fill.
            self.modal_nav_slot(
                RowAction::activate(Message::Ssh(SshMessage::SshHostKeyAcceptAndSave)),
                8.0,
                true,
                accept_btn.boxed(),
            ),
        ])
        .align_y(iced::Alignment::Center)
        .boxed()
    }

    /// Standalone keyboard-interactive (2FA / OTP) modal, used when a
    /// split-pane connect (which has no connect-progress screen) hits an
    /// Interactive auth challenge. Reuses the same `SshKbi*` messages as
    /// the inline connect-progress prompt. `name` and the prompt labels
    /// are server strings, rendered verbatim, never translated.
    pub(crate) fn view_kbi_modal(&self) -> Element<'_, Message> {
        let Some(kbi) = self.pending_kbi_prompt.as_ref() else {
            return Space::new().boxed();
        };
        let title = if kbi.name.trim().is_empty() {
            t("kbi_title").to_string()
        } else {
            kbi.name.clone()
        };

        let mut body = column![
            text(title).size(16).color(OryxisColors::t().accent),
            Space::new().height(10),
        ];
        if !kbi.instructions.trim().is_empty() {
            body = body
                .push(text(kbi.instructions.clone()).size(13).color(OryxisColors::t().text_secondary).boxed())
                .push(Space::new().height(10).boxed());
        }
        for (i, prompt) in kbi.prompts.iter().enumerate() {
            let value = self.kbi_inputs.get(i).map(|s| s.as_str()).unwrap_or("");
            let mut input = text_input(&prompt.prompt, value)
                .on_input(move |v| Message::Ssh(SshMessage::SshKbiInput(i, v.into())))
                .on_submit(Message::Ssh(SshMessage::SshKbiSubmit))
                .padding(10)
                .size(14);
            if i == 0 {
                input = input.id(iced::widget::Id::new(crate::state::KBI_FIRST_INPUT_ID));
            }
            if !prompt.echo {
                input = input.secure(true);
            }
            body = body
                .push(text(prompt.prompt.clone()).size(12).color(OryxisColors::t().text_muted).boxed())
                .push(Space::new().height(4).boxed())
                .push(input.boxed())
                .push(Space::new().height(12).boxed());
        }
        // Quick-connect prompt (split-pane connect): offer the saved
        // identities / keys as an alternative to answering by hand.
        if let Some(qid) = self.pending_kbi_quick
            && let Some(section) = self.view_quick_auth_switch(qid)
        {
            body = body.push(section).push(Space::new().height(12).boxed());
        }
        body = body.push(Space::new().height(6).boxed());

        let cancel_btn = button(
            container(text(t("cancel")).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 9.0, right: 18.0, bottom: 9.0, left: 18.0 }),
        )
        .on_press(Message::Ssh(SshMessage::SshKbiCancel))
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });
        let submit_fg = crate::theme::contrast_text_for(OryxisColors::t().accent);
        let submit_btn = button(
            container(text(t("kbi_submit")).size(13).color(submit_fg))
                .padding(Padding { top: 9.0, right: 18.0, bottom: 9.0, left: 18.0 }),
        )
        .on_press(Message::Ssh(SshMessage::SshKbiSubmit))
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let buttons = dir_row(vec![
            cancel_btn.boxed(),
            Space::new().width(Length::Fill).boxed(),
            submit_btn.boxed(),
        ])
        .align_y(iced::Alignment::Center);

        let card = container(column![body, buttons].width(Length::Fill))
            .width(Length::Fixed(480.0))
            .padding(24)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(12.0) },
                ..Default::default()
            });

        // Bare card; `widgets::modal_overlay` (the caller) centers + scrims.
        card.boxed()
    }
}
