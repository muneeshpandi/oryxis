//! SFTP view helpers: breadcrumb. Split out of views/sftp/mod.rs.

use super::*;
use iced::widget::row;
/// Build a clickable breadcrumb for a remote POSIX path. The root is
/// the only `/` rendered, subsequent segments are added with separators
/// in between, never *after* the root crumb itself, which avoids the
/// `/ / home` doubling that crept in when separators were emitted at the
/// start of every iteration.
pub(crate) fn remote_breadcrumb<'a>(side: SftpPaneSide, path: &str) -> Element<'a, Message> {
    let mut row = iced::widget::Row::<iced::Element<'_, _>>::new().align_y(iced::Alignment::Center).spacing(2);
    row = row.push(crumb_remote(side, "/", "/"));
    let mut accumulated = String::new();
    let mut first_segment = true;
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        accumulated.push('/');
        accumulated.push_str(segment);
        if !first_segment {
            row = row.push(text("/").size(11).color(OryxisColors::t().text_muted).boxed());
        }
        first_segment = false;
        row = row.push(crumb_remote(side, segment, &accumulated));
    }
    row.boxed()
}

/// Build a clickable breadcrumb for a local filesystem path. On Windows
/// the first crumb is the drive letter and clicking it opens the drive
/// picker dropdown. The Unix root chip swallows the next separator so
/// the visual reads `/ home / user` instead of `/ / home / user`. On
/// Windows the implicit `RootDir` component after the drive prefix is
/// skipped (its job is taken by the drive chip itself).
pub(crate) fn local_breadcrumb<'a>(side: SftpPaneSide, path: &std::path::Path) -> Element<'a, Message> {
    // Pick the separator from the path's flavor: real Windows drives
    // (`C:\`, `D:\`) get `\`; everything else (Unix paths, WSL UNC like
    // `\\wsl$\Ubuntu\…`, bare network shares) keeps the Unix `/` since
    // either the user is on Linux or they're navigating into a Linux
    // filesystem from Windows.
    let separator = if is_windows_disk_path(path) { "\\" } else { "/" };
    let mut row = iced::widget::Row::<iced::Element<'_, _>>::new().align_y(iced::Alignment::Center).spacing(2);
    let mut accumulated = std::path::PathBuf::new();
    let mut first = true;
    let mut last_was_root_or_drive = false;
    let mut had_drive = false;
    for component in path.components() {
        let (label, is_drive, is_root) = match component {
            std::path::Component::Prefix(p) => {
                had_drive = true;
                (p.as_os_str().to_string_lossy().into_owned(), true, false)
            }
            std::path::Component::RootDir => {
                // Skip the implicit root component on Windows, the drive
                // chip already represents the volume root.
                if had_drive {
                    accumulated.push(component.as_os_str());
                    last_was_root_or_drive = true;
                    continue;
                }
                ("/".to_string(), false, true)
            }
            std::path::Component::Normal(s) => (s.to_string_lossy().into_owned(), false, false),
            std::path::Component::CurDir | std::path::Component::ParentDir => continue,
        };
        accumulated.push(component.as_os_str());
        if !first && !last_was_root_or_drive {
            row = row.push(text(separator).size(11).color(OryxisColors::t().text_muted).boxed());
        }
        first = false;
        last_was_root_or_drive = is_root || is_drive;
        if is_drive {
            // Drive-letter chip toggles the drives dropdown so the user
            // can jump to another mount without typing.
            row = row.push(
                button(
                    row![
                        iced_fonts::lucide::hard_drive()
                            .size(11)
                            .color(OryxisColors::t().accent),
                        Space::new().width(4),
                        text(label).size(11).color(OryxisColors::t().text_secondary),
                        Space::new().width(2),
                        iced_fonts::lucide::chevron_down()
                            .size(9)
                            .color(OryxisColors::t().text_muted),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::Sftp(SftpMessage::SftpToggleDrives(side)))
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        ..Default::default()
                    }
                }).boxed(),
            );
        } else {
            row = row.push(local_crumb(side, label, accumulated.clone()));
        }
    }
    row.boxed()
}

pub(crate) fn crumb_remote<'a>(side: SftpPaneSide, label: &str, full: &str) -> Element<'a, Message> {
    let label = label.to_string();
    let full = full.to_string();
    button(text(label).size(11).color(OryxisColors::t().text_secondary))
        .on_press(Message::Sftp(SftpMessage::SftpNavigateRemote(side, full)))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }
        })
        .boxed()
}

pub(crate) fn local_crumb<'a>(side: SftpPaneSide, label: String, full: std::path::PathBuf) -> Element<'a, Message> {
    button(text(label).size(11).color(OryxisColors::t().text_secondary))
        .on_press(Message::Sftp(SftpMessage::SftpNavigateLocal(side, full)))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }
        })
        .boxed()
}

/// Breadcrumb while zip-browsing: the archive's real parent directory
/// (clicking those crumbs leaves the archive, the navigation handlers
/// close browse mode on any non-synthetic target), then an accent
/// archive chip (jumps to the archive root), then the inner segments.
pub(crate) fn zip_breadcrumb<'a>(
    side: SftpPaneSide,
    is_remote: bool,
    zip: &crate::state::ZipBrowse,
) -> Element<'a, Message> {
    let parent: Element<'a, Message> = if is_remote {
        let parent_dir = match zip.return_remote_path.as_str() {
            "" => "/".to_string(),
            p => p.to_string(),
        };
        remote_breadcrumb(side, &parent_dir)
    } else {
        local_breadcrumb(side, &zip.return_local_path)
    };
    let mut row = iced::widget::Row::<iced::Element<'_, _>>::new()
        .align_y(iced::Alignment::Center)
        .spacing(2)
        .push(parent)
        .push(text("/").size(11).color(OryxisColors::t().text_muted).boxed())
        .push(zip_crumb(side, zip.archive_name.clone(), String::new(), true));
    let mut accumulated = String::new();
    for segment in zip.inner.split('/').filter(|s| !s.is_empty()) {
        if !accumulated.is_empty() {
            accumulated.push('/');
        }
        accumulated.push_str(segment);
        row = row.push(text("/").size(11).color(OryxisColors::t().text_muted).boxed());
        row = row.push(zip_crumb(side, segment.to_string(), accumulated.clone(), false));
    }
    row.boxed()
}

/// One crumb inside the archive: navigates the VIRTUAL tree. The
/// archive chip itself is accent-tinted so the "you are inside an
/// archive" state is visible at a glance.
fn zip_crumb<'a>(
    side: SftpPaneSide,
    label: String,
    inner: String,
    is_archive_chip: bool,
) -> Element<'a, Message> {
    let color = if is_archive_chip {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_secondary
    };
    let content: Element<'a, Message> = if is_archive_chip {
        crate::widgets::dir_row(vec![
            iced_fonts::lucide::archive().size(11).color(color).boxed(),
            Space::new().width(4).boxed(),
            text(label).size(11).color(color).boxed(),
        ])
        .align_y(iced::Alignment::Center)
        .boxed()
    } else {
        text(label).size(11).color(color).boxed()
    };
    button(content)
        .on_press(Message::Sftp(SftpMessage::SftpZipNavigate(side, inner)))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }
        })
        .boxed()
}
