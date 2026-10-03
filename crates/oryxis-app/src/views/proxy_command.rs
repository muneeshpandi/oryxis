//! The command-proxy approval prompt: the dial is about to run a line
//! from the vault as a local process and wants this device's answer.
//!
//! Two render sites, for the same reason the host-key prompt has two:
//! a connect with a progress screen hosts it inline
//! (`connection_progress.rs`), and every other dial (a split pane, a
//! manually toggled port forward, an SFTP mount, a backup, the
//! remote-desktop launcher) has no such screen, so `root_view` stacks
//! the standalone card. Unlike the host-key pair, the two share their
//! body and buttons rather than each spelling them out: this prompt
//! decides whether a process runs, and two copies of that wording are
//! two chances for one of them to describe it wrong.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::{button, column, container, text, Column, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, SshMessage};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

/// What the prompt says, given the query.
///
/// `endpoint` is pre-rendered by the caller because the inline site
/// routes it through the progress screen's privacy redaction and the
/// standalone one has no progress to redact against.
///
/// The command itself is shown verbatim and is deliberately NOT
/// redacted under Privacy Mode: approving a line nobody can read is not
/// approval, it is a click.
pub(crate) fn proxy_command_body(
    query: &oryxis_ssh::ProxyCommandQuery,
    endpoint: &str,
) -> Column<iced::Element<'static, Message>> {
    column![]
        .push(
            text(t("proxy_cmd_desc").replace("{host}", endpoint))
                .size(13)
                .color(OryxisColors::t().text_secondary).boxed(),
        )
        .push(Space::new().height(12).boxed())
        .push(
            container(
                text(query.command.clone())
                    .size(13)
                    .font(iced::Font::MONOSPACE)
                    .color(OryxisColors::t().text_primary),
            )
            .width(Length::Fill)
            .padding(10)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }).boxed(),
        )
        .push(Space::new().height(12).boxed())
        .push(
            text(t("proxy_cmd_warning"))
                .size(12)
                .color(OryxisColors::t().text_muted).boxed(),
        )
        .push(Space::new().height(12).boxed())
        .push(
            text(t("proxy_cmd_question"))
                .size(13)
                .color(OryxisColors::t().text_secondary).boxed(),
        )
}

impl Oryxis {
    /// Refuse / run once / always run, in that order.
    ///
    /// Refusing is the leading button and the accent is on "always", which
    /// mirrors the host-key prompt next door; Esc refuses either way
    /// (`Modal::ProxyCommand` in `ESC_ORDER`). The three buttons are the
    /// modal keynav rows (`SurfaceFamily::Confirm`), recorded here so both
    /// render sites share the wiring, and REFUSE is the default row: a
    /// stray Enter must never spawn a local process.
    pub(crate) fn proxy_command_buttons(&self) -> Element<'_, Message> {
        self.modal_nav_reset();
        let plain = |label: &'static str, msg: Message, outlined: bool| {
            button(
                container(text(t(label)).size(13).color(OryxisColors::t().text_primary)).padding(
                    Padding { top: 10.0, right: 20.0, bottom: 10.0, left: 20.0 },
                ),
            )
            .on_press(msg)
            .style(move |_, status| button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered | button::Status::Pressed => OryxisColors::t().bg_hover,
                    _ => OryxisColors::t().bg_surface,
                })),
                border: Border {
                    radius: Radius::from(8.0),
                    color: if outlined {
                        OryxisColors::t().border
                    } else {
                        Color::TRANSPARENT
                    },
                    width: if outlined { 1.0 } else { 0.0 },
                },
                ..Default::default()
            })
        };

        let always_fg = crate::theme::contrast_text_for(OryxisColors::t().accent);
        let always = button(
            container(
                text(t("proxy_cmd_always"))
                    .size(13)
                    .font(iced::Font {
                        weight: iced::font::Weight::Semibold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    })
                    .color(always_fg),
            )
            .padding(Padding { top: 10.0, right: 20.0, bottom: 10.0, left: 20.0 }),
        )
        .on_press(Message::Ssh(SshMessage::SshProxyCommandAlways))
        .style(|_, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered | button::Status::Pressed => {
                    OryxisColors::t().accent_hover
                }
                _ => OryxisColors::t().accent,
            })),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        use crate::keynav::RowAction;
        dir_row(vec![
            self.modal_nav_slot_default(
                RowAction::activate(Message::Ssh(SshMessage::SshProxyCommandReject)),
                8.0,
                false,
                plain(
                    "proxy_cmd_deny",
                    Message::Ssh(SshMessage::SshProxyCommandReject),
                    false,
                )
                .boxed(),
            ),
            Space::new().width(8).boxed(),
            self.modal_nav_slot(
                RowAction::activate(Message::Ssh(SshMessage::SshProxyCommandOnce)),
                8.0,
                false,
                plain(
                    "proxy_cmd_once",
                    Message::Ssh(SshMessage::SshProxyCommandOnce),
                    true,
                )
                .boxed(),
            ),
            Space::new().width(Length::Fill).boxed(),
            // Accent-filled: the ring needs the contrast colour or it
            // vanishes into the fill.
            self.modal_nav_slot(
                RowAction::activate(Message::Ssh(SshMessage::SshProxyCommandAlways)),
                8.0,
                true,
                always.boxed(),
            ),
        ])
        .align_y(iced::Alignment::Center)
        .boxed()
    }

    /// Standalone card, for every dial that has no connect-progress
    /// screen to host the prompt inline. Stacked by `root_view` under
    /// the same `connecting.is_none()` gate the host-key modal uses, so
    /// the two can never both claim the screen.
    pub(crate) fn view_proxy_command_modal(&self) -> Element<'_, Message> {
        let Some(query) = self.pending_proxy_command.as_ref() else {
            return Space::new().boxed();
        };
        let endpoint = format!("{}:{}", query.target_host, query.target_port);
        let card = container(
            column![
                text(t("proxy_cmd_title"))
                    .size(16)
                    .color(OryxisColors::t().warning),
                Space::new().height(10),
                proxy_command_body(query, &endpoint),
                Space::new().height(18),
                self.proxy_command_buttons(),
            ]
            .width(Length::Fill),
        )
        .width(Length::Fixed(520.0))
        .padding(24)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
            border: Border {
                color: OryxisColors::t().border,
                width: 1.0,
                radius: Radius::from(12.0),
            },
            ..Default::default()
        });

        // Bare card; `widgets::modal_overlay` (the caller) centers + scrims.
        card.boxed()
    }
}
