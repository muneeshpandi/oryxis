//! Read-only reader for a saved AI conversation.
//!
//! Opened from the History timeline, where saved conversations sit next to
//! the recordings. Reading, not resuming: the terminal the conversation was
//! held against is gone and its captured context is stale, so this offers
//! no way to send another message, exactly as a recording is re-watched
//! rather than re-entered.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{HistoryMessage, Message, Oryxis};
use crate::state::ChatViewer;
use crate::theme::OryxisColors;

impl Oryxis {
    pub(crate) fn view_chat_viewer<'a>(&'a self, viewer: &'a ChatViewer) -> Element<'a, Message> {
        let theme = OryxisColors::t();

        let header = crate::widgets::dir_row(vec![
            iced_fonts::lucide::bot()
                .size(14)
                .color(theme.accent)
                .boxed(),
            Space::new().width(8).boxed(),
            text(viewer.label.clone())
                .size(13)
                .color(theme.text_primary)
                .boxed(),
            Space::new().width(8).boxed(),
            text(format!(
                "{} {}",
                viewer.messages.len(),
                crate::i18n::t("chat_turns")
            ))
            .size(11)
            .color(theme.text_muted)
            .boxed(),
            Space::new().width(Length::Fill).boxed(),
            close_button(),
        ])
        .align_y(iced::Alignment::Center);

        let mut turns = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(10);
        for msg in &viewer.messages {
            turns = turns.push(turn_view(msg));
        }

        container(
            column![
                container(header).padding(Padding {
                    top: 10.0,
                    right: 14.0,
                    bottom: 10.0,
                    left: 14.0,
                }),
                container(scrollable(
                    container(turns).padding(Padding {
                        top: 4.0,
                        right: 14.0,
                        bottom: 14.0,
                        left: 14.0,
                    })
                ))
                .height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            ..Default::default()
        })
        .boxed()
    }
}

/// One saved turn: a coloured role caption over its text, with the tool
/// exchange rendered as the command and its output when the turn carried
/// one.
fn turn_view<'a>(msg: &'a oryxis_vault::ChatMessageEntry) -> Element<'a, Message> {
    let theme = OryxisColors::t();
    // The caption colour is the only signal of who said what, so it uses
    // the same semantic colours the live bubbles do.
    let (caption, colour) = match msg.role.as_str() {
        "user" => (crate::i18n::t("chat_role_you"), theme.accent),
        "assistant" => (crate::i18n::t("chat_role_assistant"), theme.success),
        "tool" => (crate::i18n::t("chat_role_command"), theme.warning),
        "error" => (crate::i18n::t("chat_role_error"), theme.error),
        _ => (crate::i18n::t("chat_role_note"), theme.text_muted),
    };

    let mut col = column![
        text(caption).size(10).color(colour),
        Space::new().height(3),
    ];
    if !msg.content.is_empty() {
        col = col.push(text(msg.content.clone()).size(12).color(theme.text_primary).boxed());
    }

    // The tool exchange was stored as JSON so the reader can lay it out the
    // way the live bubble does rather than as one flat blob.
    if let Some(json) = &msg.tool_json
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(json)
        && let Some(output) = v["output"].as_str()
        && !output.is_empty()
    {
        col = col.push(Space::new().height(4).boxed());
        col = col.push(
            container(text(output.to_string()).size(11).color(theme.text_secondary))
                .padding(8)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                    border: Border {
                        radius: Radius::from(6.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }).boxed(),
        );
    }

    container(col).width(Length::Fill).boxed()
}

fn close_button<'a>() -> Element<'a, Message> {
    button(
        container(
            text(crate::i18n::t("close"))
                .size(11)
                .font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(OryxisColors::t().text_muted),
        )
        .center_y(Length::Fixed(24.0))
        .padding(Padding {
            top: 0.0,
            right: 14.0,
            bottom: 0.0,
            left: 14.0,
        }),
    )
    .on_press(Message::History(HistoryMessage::CloseChatConversation))
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color {
                a: 0.15,
                ..OryxisColors::t().error
            },
            _ => Color::TRANSPARENT,
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
    })
    .boxed()
}
