//! Host badge helper + the "Select a host" modal for the SFTP-sync
//! backup host. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

/// OS-icon avatar for a host, matching the dashboard card and the SFTP
/// file-browser picker. Output lifetime is tied to `conn` (the glyph and
/// label borrow it); `default_icon` only feeds the owned style lookup.
pub(crate) fn host_badge<'a>(
    conn: &'a oryxis_core::models::connection::Connection,
    default_icon: &str,
    size: f32,
) -> Element<'a, Message> {
    let (glyph, default_color) =
        crate::os_icon::resolve_icon(conn.detected_os.as_deref(), OryxisColors::t().accent);
    let badge_style =
        crate::widgets::resolve_host_icon_style(conn.icon_style.as_deref(), default_icon);
    let badge_color = conn
        .custom_color
        .as_deref()
        .or(conn.color.as_deref())
        .and_then(crate::widgets::parse_hex_color)
        .unwrap_or(default_color);
    let glyph_el: Element<'a, Message> = glyph.view(size * 0.58, Color::WHITE);
    crate::widgets::host_icon(badge_style, badge_color, &conn.label, Some(glyph_el), size)
}

/// Which Sync form the host picker is choosing for. The two never
/// coexist (`Modal::SyncHostPicker` covers both), and everything that
/// differs between them is a message or a search buffer, so the dialog
/// is built once and parametrized here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HostPickerTarget {
    /// The SFTP snapshot transport's backup host.
    SftpSync,
    /// The relay deploy's target host (E3).
    RelayDeploy,
}

impl HostPickerTarget {
    fn search(self, app: &Oryxis) -> &str {
        match self {
            Self::SftpSync => &app.sync.sftp.picker_search,
            Self::RelayDeploy => &app.sync.relay_deploy.picker_search,
        }
    }
    fn pick(self, id: uuid::Uuid) -> Message {
        match self {
            Self::SftpSync => Message::Sync(SyncMessage::SftpHostChanged(id)),
            Self::RelayDeploy => Message::Sync(SyncMessage::DeployHostChanged(id)),
        }
    }
    fn close(self) -> Message {
        match self {
            Self::SftpSync => Message::Sync(SyncMessage::SftpHostPickerClose),
            Self::RelayDeploy => Message::Sync(SyncMessage::DeployHostPickerClose),
        }
    }
    fn search_changed(self, v: String) -> Message {
        match self {
            Self::SftpSync => Message::Sync(SyncMessage::SftpHostPickerSearch(v)),
            Self::RelayDeploy => Message::Sync(SyncMessage::DeployHostPickerSearch(v)),
        }
    }
}

/// The "Select a host" modal the Sync settings open. Mirrors the SFTP
/// file-browser picker: a searchable list of saved hosts, each row an
/// OS badge + label + address. Rendered as a dimming scrim plus a
/// centered dialog; the caller stacks it over the settings page.
///
/// Keyboard: it is `Modal::SyncHostPicker`, so Esc closes it and the
/// rows record on the modal ring (Up/Down, Enter picks); the search
/// field keeps the caret (`modal_surface_has_input`).
pub(super) fn sync_host_picker_modal(app: &Oryxis, target: HostPickerTarget) -> Element<'_, Message> {
    app.modal_nav_reset();
    let q = target.search(app).to_lowercase();
    let mut list = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(2);
    for conn in app.connections.iter().filter(|c| {
        q.is_empty()
            || c.label.to_lowercase().contains(&q)
            || c.hostname.to_lowercase().contains(&q)
    }) {
        let badge = host_badge(conn, &app.prefs.default_host_icon, 24.0);
        let row_btn = button(
            dir_row(vec![
                badge,
                Space::new().width(10).boxed(),
                column![
                    text(conn.label.clone())
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                    text(conn.hostname.clone())
                        .size(10)
                        .color(OryxisColors::t().text_muted),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x())
                .boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .on_press(target.pick(conn.id))
        .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
        .width(Length::Fill)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(6.0),
                    ..Default::default()
                },
                ..Default::default()
            }
        });
        list = list.push(app.modal_nav_slot(
            crate::keynav::RowAction::activate(target.pick(conn.id)),
            6.0,
            false,
            row_btn.boxed(),
        ));
    }

    let dialog = container(
        column![
            dir_row(vec![
                text(t("select_a_host"))
                    .size(15)
                    .color(OryxisColors::t().text_primary)
                    .boxed(),
                Space::new().width(Length::Fill).boxed(),
                crate::widgets::panel_close_button(target.close()).boxed(),
            ])
            .align_y(iced::Alignment::Center)
            .width(Length::Fill),
            Space::new().height(8),
            text_input(t("search_hosts"), target.search(app))
                .on_input(move |v| target.search_changed(v))
                .size(13)
                .padding(10)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x()),
            Space::new().height(8),
            scrollable(list).height(Length::Fixed(360.0)),
        ]
        .padding(20)
        .width(Length::Fixed(440.0))
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

    let scrim: Element<'_, Message> = iced::widget::opaque(
        iced::widget::MouseArea::new(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                    ..Default::default()
                }),
        )
        .on_press(target.close()),
    ).boxed();

    let centered = container::<_, iced::Theme>(iced::widget::MouseArea::new(dialog).on_press(Message::NoOp))
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
