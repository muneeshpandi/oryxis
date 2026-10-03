//! Host editor: the SSH Network rows (host chaining, proxy, port
//! forwards, keepalive, address family, auto-title, algorithm overrides).
use super::*;
use iced::widget::column;

impl Oryxis {
    pub(super) fn hp_row_chaining(&self, is_ssh: bool) -> Element<'_, Message> {
        // ── Section: Advanced Options ──
        // Chain summary for the "Host Chaining" row: the hop labels
        // joined in order (bastion > db-proxy > ...), or "disabled"
        // when empty. Hops pointing at a since-deleted host resolve to
        // a placeholder rather than vanishing, so the count stays
        // honest until the user opens the editor and prunes them.
        let chain_summary = if self.editor_form.jump_chain.is_empty() {
            t("disabled").to_string()
        } else {
            self.editor_form
                .jump_chain
                .iter()
                .map(|id| {
                    self.connections
                        .iter()
                        .find(|c| c.id == *id)
                        .map(|c| c.label.clone())
                        .unwrap_or_else(|| t("unknown").to_string())
                })
                .collect::<Vec<_>>()
                .join(" › ")
        };
        // Single "Host Chaining" entry point (SSH > Network). Clicking
        // opens the chain editor (Termius-style multi-hop). Replaces the
        // old read-only row + separate single-host "Jump Host" picker.
        let row_chaining: Element<'_, Message> = if is_ssh {
            self.panel_nav_slot(
            crate::keynav::RowAction::activate(Message::Editor(EditorMessage::OpenChainEditor)),
            6.0,
            container(
                button(
                    dir_row(vec![
                        iced_fonts::lucide::link().size(14).color(OryxisColors::t().text_muted).boxed(),
                        Space::new().width(10).boxed(),
                        text(t("host_chaining")).size(13).color(OryxisColors::t().text_secondary).boxed(),
                        Space::new().width(Length::Fill).boxed(),
                        text(chain_summary)
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .boxed(),
                        Space::new().width(8).boxed(),
                        iced_fonts::lucide::chevron_right().size(12).color(OryxisColors::t().text_muted).boxed(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::Editor(EditorMessage::OpenChainEditor))
                .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 0.0 })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }
                })
            ).boxed(),
            )
        } else {
            empty()
        };
        row_chaining
    }

    pub(super) fn hp_pf_items(&self, is_ssh: bool) -> Element<'_, Message> {
        // ── Section: Port Forwarding ──
        let pf_items: Element<'_, Message> = if is_ssh {
        let mut pf_items = column![
            dir_row(vec![
                iced_fonts::lucide::arrow_right_left().size(14).color(OryxisColors::t().text_muted).boxed(),
                Space::new().width(10).boxed(),
                text(t("port_forwarding")).size(13).color(OryxisColors::t().text_secondary).boxed(),
                Space::new().width(Length::Fill).boxed(),
                self.panel_nav_slot(
                    crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorAddPortForward)),
                    4.0,
                    button(text("+").size(14).color(OryxisColors::t().text_primary))
                        .on_press(Message::Editor(EditorMessage::EditorAddPortForward))
                        .style(|_, _| button::Style {
                            background: Some(Background::Color(OryxisColors::t().bg_hover)),
                            border: Border { radius: Radius::from(4.0), ..Default::default() },
                            text_color: OryxisColors::t().text_primary,
                            ..Default::default()
                        })
                        .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
                        .boxed(),
                ),
            ]).align_y(iced::Alignment::Center),
        ];
        // Say what these ARE. There are two port-forward systems in the
        // app and nothing said so, which is how you end up with a rule
        // you cannot find (owner, 2026-08-02): the ones typed here ride
        // this host's shell session and die with it, and they are local
        // (-L) only. The standalone rules on the Port Forwarding screen
        // are the ones with -R / -D, a bind address and auto-start, and
        // they open a session of their own.
        pf_items = pf_items.push(Space::new().height(4).boxed());
        pf_items = pf_items.push(
            text(t("host_port_forward_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted).boxed(),
        );

        for (i, pf) in self.editor_form.port_forwards.iter().enumerate() {
            let idx = i;
            pf_items = pf_items.push(Space::new().height(8).boxed());
            // The three per-rule inputs stay mouse-only: the fork's
            // Id::new takes &'static str, so dynamic rows cannot carry
            // unique focus ids. The remove button is the keyboard row.
            pf_items = pf_items.push(
                dir_row(vec![
                    text_input("8080", &pf.local_port)
                        .on_input(move |v| Message::Editor(EditorMessage::EditorPortFwdLocalPortChanged(idx, v)))
                        .padding(6)
                        .width(70)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                    text(" -> ").size(12).color(OryxisColors::t().text_muted).boxed(),
                    text_input("localhost", &pf.remote_host)
                        .on_input(move |v| Message::Editor(EditorMessage::EditorPortFwdRemoteHostChanged(idx, v)))
                        .padding(6)
                        .width(Length::Fill)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                    text(":").size(12).color(OryxisColors::t().text_muted).boxed(),
                    text_input("3306", &pf.remote_port)
                        .on_input(move |v| Message::Editor(EditorMessage::EditorPortFwdRemotePortChanged(idx, v)))
                        .padding(6)
                        .width(70)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorRemovePortForward(idx))),
                        4.0,
                        button(text("\u{00D7}").size(11).color(OryxisColors::t().error))
                            .on_press(Message::Editor(EditorMessage::EditorRemovePortForward(idx)))
                            .style(|_, _| button::Style {
                                background: None,
                                border: Border::default(),
                                text_color: OryxisColors::t().error,
                                ..Default::default()
                            })
                            .padding(Padding { top: 2.0, right: 4.0, bottom: 2.0, left: 4.0 })
                            .boxed(),
                    ),
                ]).align_y(iced::Alignment::Center).spacing(4).boxed(),
            );
        }
        // Standalone rules that travel through THIS host, listed
        // read-only. It answers "where do I use the ones I created?"
        // where the user goes looking, and it is the only place the two
        // systems are visible side by side. Editing them stays on their
        // own screen, one click away.
        if let Some(id) = self.editor_form.editing_id {
            let mine: Vec<&oryxis_core::models::port_forward_rule::PortForwardRule> = self
                .port_forward_rules
                .iter()
                .filter(|r| r.host_id == id)
                .collect();
            if !mine.is_empty() {
                pf_items = pf_items.push(Space::new().height(12).boxed());
                pf_items = pf_items.push(
                    text(t("host_standalone_forwards"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary).boxed(),
                );
                for rule in mine {
                    let live = self.active_forwards.contains_key(&rule.id);
                    pf_items = pf_items.push(Space::new().height(4).boxed());
                    pf_items = pf_items.push(
                        dir_row(vec![
                            text(if live { "\u{25CF}" } else { "\u{25CB}" })
                                .size(9)
                                .color(if live {
                                    OryxisColors::t().success
                                } else {
                                    OryxisColors::t().text_muted
                                })
                                .boxed(),
                            Space::new().width(6).boxed(),
                            text(rule.label.clone())
                                .size(11)
                                .color(OryxisColors::t().text_secondary)
                                .boxed(),
                            Space::new().width(8).boxed(),
                            text(crate::views::port_forwards::forward_summary(rule))
                                .size(10)
                                .font(iced::Font::MONOSPACE)
                                .color(OryxisColors::t().text_muted)
                                .boxed(),
                        ])
                        .align_y(iced::Alignment::Center).boxed(),
                    );
                }
                pf_items = pf_items.push(Space::new().height(6).boxed());
                pf_items = pf_items.push(self.panel_nav_slot(
                    crate::keynav::RowAction::activate(Message::Navigation(
                        crate::app::NavigationMessage::ChangeView(
                            crate::state::View::PortForwarding,
                        ),
                    )),
                    4.0,
                    crate::widgets::styled_button_opt(
                        crate::i18n::t("host_manage_forwards"),
                        Some(Message::Navigation(
                            crate::app::NavigationMessage::ChangeView(
                                crate::state::View::PortForwarding,
                            ),
                        )),
                        OryxisColors::t().accent,
                    ),
                ));
            }
        }
        pf_items.boxed()
        } else {
            empty()
        };
        pf_items
    }

    pub(super) fn hp_row_keepalive(&self, is_ssh: bool) -> Element<'_, Message> {
        // Per-host keepalive override (SSH > Network). Empty placeholder
        // reflects the global default so the user sees what "inherit"
        // means; "0" disables keepalive on this host.
        let row_keepalive: Element<'_, Message> = if is_ssh {
            container(
            dir_row(vec![
                iced_fonts::lucide::activity().size(14).color(OryxisColors::t().text_muted).boxed(),
                Space::new().width(10).boxed(),
                column![
                    text(t("host_keepalive")).size(13).color(OryxisColors::t().text_secondary),
                    Space::new().height(2),
                    text(t("host_keepalive_desc")).size(11).color(OryxisColors::t().text_muted),
                ].width(Length::Fill).boxed(),
                Space::new().width(12).boxed(),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-keepalive")),
                    10.0,
                    text_input(
                        &self.prefs.keepalive_interval,
                        &self.editor_form.keepalive_interval,
                    )
                        .id(iced::widget::Id::new("editor-keepalive"))
                        .on_input(|v| Message::Editor(EditorMessage::EditorKeepaliveChanged(v)))
                        .on_submit_maybe(self.hp_submit())
                        .padding(6)
                        .width(100)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                ),
            ]).align_y(iced::Alignment::Center)
            )
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 }).boxed()
        } else {
            empty()
        };
        row_keepalive
    }

    pub(super) fn hp_row_mac_address(&self, show: bool) -> Element<'_, Message> {
        // Wake-on-LAN MAC (every protocol with a network endpoint; the
        // serial form hides it). Empty = no MAC, which hides the card's
        // Wake on LAN action. The placeholder is a MAC literal, not a
        // translatable string.
        let row_mac: Element<'_, Message> = if show {
            container(
            dir_row(vec![
                iced_fonts::lucide::zap().size(14).color(OryxisColors::t().text_muted).boxed(),
                Space::new().width(10).boxed(),
                column![
                    text(t("host_mac_address")).size(13).color(OryxisColors::t().text_secondary),
                    Space::new().height(2),
                    text(t("host_mac_address_desc")).size(11).color(OryxisColors::t().text_muted),
                ].width(Length::Fill).boxed(),
                Space::new().width(12).boxed(),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-mac-address")),
                    10.0,
                    text_input(
                        "AA:BB:CC:DD:EE:FF",
                        &self.editor_form.mac_address,
                    )
                        .id(iced::widget::Id::new("editor-mac-address"))
                        .on_input(|v| Message::Editor(EditorMessage::EditorMacAddressChanged(v)))
                        .on_submit_maybe(self.hp_submit())
                        .padding(6)
                        .width(160)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                ),
            ]).align_y(iced::Alignment::Center)
            )
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 }).boxed()
        } else {
            empty()
        };
        row_mac
    }

    pub(super) fn hp_row_address_family(&self, is_ssh: bool, is_telnet: bool) -> Element<'_, Message> {
        // Per-host address-family preference (SSH > Network, and the
        // reduced Telnet form: both dial TCP): Auto keeps resolver
        // order, IPv4/IPv6 filter the resolved addresses (PuTTY's
        // Connection panel setting, which PuTTY applies to Telnet too).
        // Typed pick_list over the enum; IPv4/IPv6 are universal
        // labels, so Display feeds it.
        let row_address_family: Element<'_, Message> = if is_ssh || is_telnet {
            use oryxis_core::models::connection::AddressFamily;
            panel_option_row(
                iced_fonts::lucide::globe(),
                t("host_address_family"),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-addr-family")),
                    crate::widgets::INPUT_RADIUS,
                    pick_list(
                        Some(self.editor_form.address_family),
                        vec![AddressFamily::Auto, AddressFamily::V4, AddressFamily::V6],
                        |f: &AddressFamily| f.to_string(),
                    )
                    .on_select(|v| Message::Editor(EditorMessage::EditorAddressFamilyChanged(v)))
                    .id(iced::widget::Id::new("editor-pick-addr-family"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(120)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .boxed(),
                ),
            )
        } else {
            empty()
        };
        row_address_family
    }

    pub(super) fn hp_row_auto_title(&self, is_ssh: bool) -> Element<'_, Message> {
        // Per-host auto-title (OSC 0/2) override: Default (inherit global) /
        // Show (always use the shell title) / Hide (always keep this host's
        // curated label).
        let auto_title_selected = match self.editor_form.auto_title {
            Some(true) => t("host_auto_title_show"),
            Some(false) => t("host_auto_title_hide"),
            None => t("host_auto_title_default"),
        }
        .to_string();
        let auto_title_options = vec![
            t("host_auto_title_default").to_string(),
            t("host_auto_title_show").to_string(),
            t("host_auto_title_hide").to_string(),
        ];
        // Focusable select (Tab + Enter/Space, widget-owned keys).
        let row_auto_title: Element<'_, Message> = if is_ssh {
            panel_option_row(
            iced_fonts::lucide::file_text(),
            t("host_auto_title"),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-auto-title")),
                crate::widgets::INPUT_RADIUS,
                pick_list(Some(auto_title_selected), auto_title_options, |s: &String| s.clone())
                    .on_select(|v| Message::Editor(EditorMessage::EditorAutoTitleChanged(v)))
                    .id(iced::widget::Id::new("editor-pick-auto-title"))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(120)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .boxed(),
            ),
            )
        } else {
            empty()
        };
        row_auto_title
    }

    /// Per-host legacy-algorithm overrides: one block per negotiation
    /// category (ciphers / kex / MACs / host keys). Each is `Auto` (the
    /// safe russh default, untouched) until toggled off, which reveals a
    /// checklist seeded from the defaults so the user can add the cbc /
    /// 3des / sha1 / dh-group1 entries a legacy server needs.
    pub(super) fn algo_overrides_section(&self) -> Element<'_, Message> {
        use crate::state::AlgoCategory;
        use iced::widget::checkbox;
        let c = OryxisColors::t();
        let mut col = column![
            text(t("algo_overrides")).size(13).color(c.text_secondary),
            Space::new().height(2),
            text(t("algo_overrides_desc")).size(11).color(c.text_muted),
        ];
        for cat in AlgoCategory::ALL {
            let is_auto = self.editor_form.algo_list(cat).is_none();
            col = col.push(Space::new().height(10).boxed());
            // Explicit "Auto / Custom" picker per category; choosing Custom
            // reveals the algorithm checklist below. Left/Right cycle the
            // two modes from the keyboard.
            let auto_label = t("algo_auto");
            let selected = if is_auto { auto_label } else { t("algo_custom") }.to_string();
            let mode_options = vec![auto_label.to_string(), t("algo_custom").to_string()];
            let mk_mode = move |s: String| Message::Editor(EditorMessage::EditorAlgoSetAuto(cat, s == auto_label));
            // Focusable select; the id must be unique per category so
            // Tab focuses exactly one of the four mode pickers.
            let mode_id: &'static str = match cat {
                crate::state::AlgoCategory::Cipher => "editor-pick-algo-cipher",
                crate::state::AlgoCategory::Kex => "editor-pick-algo-kex",
                crate::state::AlgoCategory::Mac => "editor-pick-algo-mac",
                crate::state::AlgoCategory::HostKey => "editor-pick-algo-hostkey",
            };
            col = col.push(panel_option_row(
                iced_fonts::lucide::shield(),
                t(cat.label_key()),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(mode_id)),
                    crate::widgets::INPUT_RADIUS,
                    pick_list(Some(selected), mode_options, |s: &String| s.clone())
                        .on_select(mk_mode)
                        .id(iced::widget::Id::new(mode_id))
                        .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                        .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                        .width(120)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .boxed(),
                ),
            ));
            if !is_auto {
                let selected: Vec<String> =
                    self.editor_form.algo_list(cat).clone().unwrap_or_default();
                let mut checks = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(4);
                for algo in cat.supported() {
                    let name = algo.to_string();
                    let checked = selected.iter().any(|n| n == algo);
                    // Each algorithm checkbox is its own keyboard row;
                    // Enter/Space flips it like a click.
                    checks = checks.push(self.panel_nav_slot(
                        crate::keynav::RowAction::activate(Message::Editor(EditorMessage::EditorAlgoToggle(
                            cat,
                            name.clone(),
                        ))),
                        4.0,
                        checkbox(checked)
                            .label(algo)
                            .on_toggle(move |_| Message::Editor(EditorMessage::EditorAlgoToggle(cat, name.clone())))
                            .size(15)
                            .text_size(12)
                            .boxed(),
                    ));
                }
                col = col.push(container(checks).padding(Padding {
                    top: 4.0,
                    right: 0.0,
                    bottom: 4.0,
                    left: 16.0,
                }).boxed());
            }
        }
        col.boxed()
    }

    /// Build the Proxy rows (no card wrapper, the caller nests them in
    /// the SSH > Network subgroup). The picker mixes the static proxy
    /// types (None / SOCKS5 / SOCKS4 / HTTP / Command) with the user's
    /// saved `ProxyIdentity` entries, selecting an identity hides the
    /// inline fields and shows a readonly summary instead.
    pub(super) fn build_proxy_rows(&self) -> iced::widget::Column<iced::Element<'_, Message>> {
        let kind = self.editor_form.proxy_kind;

        // Compose the picker option list. Identity entries come from
        // `self.proxy_identities` so the user can pick any saved
        // config without leaving the host editor.
        let mut options: Vec<ProxyKind> = ProxyKind::STATIC.to_vec();
        for pi in &self.proxy_identities {
            options.push(ProxyKind::Identity(pi.id));
        }

        // Capture the identities by reference so the closure can render
        // the user-chosen label for `Identity(_)` entries instead of
        // the generic Display fallback. The borrow lives as long as
        // `self`, which covers the returned Element, so no clone of
        // the Vec is needed per render.
        let identities = &self.proxy_identities;
        // Inline row (label left, picker right) mirroring the Auth Method
        // row, so the two pickers read as the same control family. The
        // type-dependent fields still stack below.
        // Focusable select (Tab + Enter/Space, widget-owned keys).
        let picker: Element<'_, Message> = panel_option_row(
            iced_fonts::lucide::route(),
            crate::i18n::t("proxy_type"),
            self.panel_nav_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new("editor-pick-proxy-kind")),
                crate::widgets::INPUT_RADIUS,
                pick_list(Some(kind), options, move |k: &ProxyKind| match k {
                    ProxyKind::Identity(id) => identities
                        .iter()
                        .find(|pi| pi.id == *id)
                        .map(|pi| format!("📌 {}", pi.label))
                        .unwrap_or_else(|| crate::i18n::t("proxy_type_identity_deleted").into()),
                    other => other.to_string(),
                })
                .on_select(|v| Message::Editor(EditorMessage::EditorProxyKindChanged(v)))
                .id(iced::widget::Id::new("editor-pick-proxy-kind"))
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(140)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style)
                .boxed(),
            ),
        );

        let mut col = column![picker];

        // Saved-identity selection: show a small readonly summary so
        // the user can see what they picked without flipping screens.
        // The actual identity edits live under Settings → Proxies.
        if let ProxyKind::Identity(id) = kind {
            let summary = identities
                .iter()
                .find(|pi| pi.id == id)
                .map(|pi| {
                    let kind_label = match &pi.proxy_type {
                        oryxis_core::models::connection::ProxyType::Socks5 => "SOCKS5",
                        oryxis_core::models::connection::ProxyType::Socks4 => "SOCKS4",
                        oryxis_core::models::connection::ProxyType::Http => "HTTP",
                        oryxis_core::models::connection::ProxyType::Command(_) => "CMD",
                    };
                    let user_part = pi
                        .username
                        .as_deref()
                        .map(|u| format!(" ({u})"))
                        .unwrap_or_default();
                    format!("{kind_label}, {}:{}{}", pi.host, pi.port, user_part)
                })
                .unwrap_or_else(|| crate::i18n::t("proxy_type_identity_deleted").into());
            col = col.push(Space::new().height(8).boxed()).push(
                text(summary).size(12).color(OryxisColors::t().text_muted).boxed(),
            );
            return col;
        }

        if kind == ProxyKind::None {
            return col;
        }

        if kind == ProxyKind::Command {
            col = col
                .push(Space::new().height(8).boxed())
                .push(panel_field(
                    crate::i18n::t("proxy_command"),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::input(iced::widget::Id::new("editor-proxy-command")),
                        10.0,
                        text_input(
                            crate::i18n::t("proxy_command_placeholder"),
                            &self.editor_form.proxy_command,
                        )
                        .id(iced::widget::Id::new("editor-proxy-command"))
                        .on_input(|v| Message::Editor(EditorMessage::EditorProxyCommandChanged(v)))
                        .on_submit_maybe(self.hp_submit())
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                    ),
                ));
            return col;
        }

        if kind.needs_endpoint() {
            col = col
                .push(Space::new().height(8).boxed())
                .push(panel_field(
                    crate::i18n::t("proxy_host"),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::input(iced::widget::Id::new("editor-proxy-host")),
                        10.0,
                        text_input(
                            crate::i18n::t("proxy_host_placeholder"),
                            &self.editor_form.proxy_host,
                        )
                        .id(iced::widget::Id::new("editor-proxy-host"))
                        .on_input(|v| Message::Editor(EditorMessage::EditorProxyHostChanged(v)))
                        .on_submit_maybe(self.hp_submit())
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                    ),
                ))
                .push(Space::new().height(8).boxed())
                .push(panel_field(
                    crate::i18n::t("proxy_port"),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::input(iced::widget::Id::new("editor-proxy-port")),
                        10.0,
                        text_input("1080", &self.editor_form.proxy_port)
                            .id(iced::widget::Id::new("editor-proxy-port"))
                            .on_input(|v| Message::Editor(EditorMessage::EditorProxyPortChanged(v)))
                            .on_submit_maybe(self.hp_submit())
                            .padding(6)
                            .width(70)
                            .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                            .boxed(),
                    ),
                ))
                .push(Space::new().height(8).boxed())
                .push(panel_field(
                    crate::i18n::t("proxy_username"),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::input(iced::widget::Id::new("editor-proxy-username")),
                        10.0,
                        text_input(
                            crate::i18n::t("proxy_username_placeholder"),
                            &self.editor_form.proxy_username,
                        )
                        .id(iced::widget::Id::new("editor-proxy-username"))
                        .on_input(|v| Message::Editor(EditorMessage::EditorProxyUsernameChanged(v)))
                        .on_submit_maybe(self.hp_submit())
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                    ),
                ));
        }

        if kind.supports_password() {
            // Mirror the main connection-password UX: show a hint when
            // the encrypted column already holds a value, and let the
            // user clear or replace it via the touched flag.
            let placeholder: &str = if self.editor_form.has_existing_proxy_password
                && !self.editor_form.proxy_password.touched()
            {
                crate::i18n::t("proxy_password_existing")
            } else {
                crate::i18n::t("proxy_password_placeholder")
            };
            // Keyboard rows: the field, then its reveal eye.
            self.panel_nav_record(crate::keynav::RowAction::input(
                iced::widget::Id::new("editor-proxy-password"),
            ));
            col = col
                .push(Space::new().height(8).boxed())
                .push(panel_field(
                    crate::i18n::t("proxy_password"),
                    crate::widgets::password_input_with_eye_nav(
                        placeholder,
                        self.editor_form.proxy_password.as_str(),
                        |v| Message::Editor(EditorMessage::EditorProxyPasswordChanged(v.into())),
                        self.hp_submit(),
                        self.editor_form.proxy_password_visible,
                        Message::Editor(EditorMessage::EditorToggleProxyPasswordVisibility),
                        10.0,
                        Some(iced::widget::Id::new("editor-proxy-password")),
                        |eye| self.panel_nav_slot(
                            crate::keynav::RowAction::activate(
                                Message::Editor(
                                    EditorMessage::EditorToggleProxyPasswordVisibility,
                                ),
                            ),
                            6.0,
                            eye,
                        ),
                    ),
                ));
        }

        col
    }

}
