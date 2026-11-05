//! The "New symbolic link" dialog (issue #272): a link name, a target,
//! Create / Cancel. The keyboard is the new-entry dialog's: the name
//! field takes focus when it opens, Tab moves between the two fields,
//! Enter creates, Esc cancels.

use super::*;
use iced::widget::column;

/// Widget id of the link-name input, focused when the dialog opens.
pub(crate) const NEW_LINK_NAME_ID: &str = "sftp-new-link-name";
/// Widget id of the target input.
pub(crate) const NEW_LINK_TARGET_ID: &str = "sftp-new-link-target";

pub(crate) fn new_link_modal<'a>(link: &'a crate::state::SftpNewLink) -> Element<'a, Message> {
    let field_label = |label: &'static str| text(label).size(12).color(OryxisColors::t().text_secondary);
    // Off until both fields are usable, the same test the commit makes.
    let create = link.fields().map(|_| Message::Sftp(SftpMessage::SftpNewLinkCommit));
    let dialog = container(
        column![
            text(t("new_symlink")).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(12),
            field_label(t("symlink_name_label")),
            Space::new().height(4),
            text_input(t("symlink_name_placeholder"), &link.name)
                .id(iced::widget::Id::new(NEW_LINK_NAME_ID))
                .on_input(|v| Message::Sftp(SftpMessage::SftpNewLinkName(v)))
                .on_submit(Message::Sftp(SftpMessage::SftpNewLinkCommit))
                .padding(10)
                .size(13)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
            Space::new().height(10),
            field_label(t("symlink_target_label")),
            Space::new().height(4),
            text_input(t("symlink_target_placeholder"), &link.target)
                .id(iced::widget::Id::new(NEW_LINK_TARGET_ID))
                .on_input(|v| Message::Sftp(SftpMessage::SftpNewLinkTarget(v)))
                .on_submit(Message::Sftp(SftpMessage::SftpNewLinkCommit))
                .padding(10)
                .size(13)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
            Space::new().height(6),
            text(t("symlink_target_hint")).size(11).color(OryxisColors::t().text_muted),
            Space::new().height(16),
            crate::widgets::dir_row(vec![
                crate::widgets::styled_button_opt(t("create"), create, OryxisColors::t().accent),
                Space::new().width(8).boxed(),
                crate::widgets::styled_button(
                    t("cancel"),
                    Message::Sftp(SftpMessage::SftpNewLinkCancel),
                    OryxisColors::t().error,
                ),
            ]),
        ]
        .padding(24)
        .width(420)
        .align_x(dir_align_x()),
    )
    .style(|_: &iced::Theme| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(12.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    });

    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                ..Default::default()
            }),
    )
    .on_press(Message::Sftp(SftpMessage::SftpNewLinkCancel))
    .boxed();

    // Clicks inside the card are swallowed, or they would fall through
    // the Stack to the scrim and cancel the dialog.
    let centered = container::<_, iced::Theme>(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::<iced::Element<'_, _>>::new()
        .push(scrim)
        .push(centered.boxed())
        .width(Length::Fill)
        .height(Length::Fill)
        .boxed()
}
