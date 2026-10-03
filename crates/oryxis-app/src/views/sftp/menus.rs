//! SFTP view helpers: menus. Split out of views/sftp/mod.rs.

use super::*;
use iced::widget::{column, row};
pub(crate) fn pane_actions_btn<'a>(toggle_msg: Message) -> Element<'a, Message> {
    crate::widgets::card_kebab_button(
        OryxisColors::t().text_secondary,
        true,
        toggle_msg,
    )
    .boxed()
}

/// The collapsed-filter input card. Positioned + scrimmed by the caller at
/// the `view_sftp` level.
pub(crate) fn filter_card<'a>(side: SftpPaneSide, filter: &'a str) -> Element<'a, Message> {
    let id = match side {
        SftpPaneSide::Left => "sftp-filter-pop-left",
        SftpPaneSide::Right => "sftp-filter-pop-right",
    };
    let input = text_input(t("filter_placeholder"), filter)
        .id(iced::widget::Id::new(id))
        .on_input(move |s| Message::Sftp(SftpMessage::SftpFilter(side, s)))
        .on_submit(Message::Sftp(SftpMessage::SftpToggleFilterSearch(side)))
        .padding(Padding { top: 9.0, right: 12.0, bottom: 9.0, left: 12.0 })
        .size(13)
        .width(Length::Fixed(220.0))
        .style(crate::widgets::rounded_input_style)
        .align_x(dir_align_x());
    let card = container(input)
        .padding(6)
        .width(Length::Fixed(232.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        });
    card.boxed()
}

/// Floating Actions menu for a pane, anchored to the top-right via a
/// container that pushes it to the corner.
/// The actions (`⋮`) menu card. Positioned + scrimmed by the caller at the
/// `view_sftp` level so a click anywhere (including the other pane) closes it.
pub(crate) fn actions_menu_card<'a>(
    side: SftpPaneSide,
    is_remote: bool,
    remote_path: &str,
    local_path: &std::path::Path,
    show_hidden: bool,
    cols: crate::state::SftpColumns,
) -> Element<'a, Message> {
    use crate::state::SftpColumn;
    // Same directory-level actions as the cursor-anchored background menu,
    // shared via `dir_action_items` so the two never drift apart.
    let mut menu_col = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(2).padding(4);
    // `pane_dir` must be SIDE-RESOLVED (the background menu already does
    // this in main_layout.rs): on a Local pane `remote_path` is empty or
    // stale, and Copy path would copy that instead of the local dir.
    let local_dir_display;
    let pane_dir: &str = if is_remote {
        remote_path
    } else {
        local_dir_display = local_path.to_string_lossy().into_owned();
        &local_dir_display
    };
    let dir_ctx = DirActionCtx { pane_dir, local_dir: local_path, show_hidden };
    // The `⋮` menu is mouse-only for now: take the elements, drop the
    // per-row messages the keyboard-navigable row menu records.
    for (_, it) in dir_action_items(side, is_remote, dir_ctx, true) {
        menu_col = menu_col.push(it);
    }
    // Columns section: toggle each optional column. The menu stays open on
    // click so several can be flipped in one pass.
    menu_col = menu_col.push(menu_separator());
    menu_col = menu_col.push(
        container(
            text(t("columns"))
                .size(10)
                .color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 4.0, right: 10.0, bottom: 2.0, left: 10.0 }).boxed(),
    );
    for (label, col, on) in [
        (t("col_modified"), SftpColumn::Modified, cols.modified),
        (t("col_size"), SftpColumn::Size, cols.size),
        (t("col_type"), SftpColumn::Kind, cols.kind),
        (t("col_permissions"), SftpColumn::Permissions, cols.permissions),
        (t("col_owner"), SftpColumn::Owner, cols.owner),
    ] {
        menu_col = menu_col.push(column_toggle_item(side, label, col, on));
    }
    let menu = container(menu_col)
    // Pin the menu to the same width as the rows inside it. Without
    // this, `menu_separator`'s `Length::Fill` propagates up through
    // `column![]` and the outer container, stretching the dropdown
    // across the entire pane.
    .width(Length::Fixed(228.0))
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    });
    menu.boxed()
}

pub(crate) fn menu_separator<'a>() -> Element<'a, Message> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().border)),
            ..Default::default()
        })
        .boxed()
}

/// One row of the Columns section in the actions menu: a check glyph
/// (shown only when the column is visible) plus the column label. Firing
/// `SftpToggleColumn` flips the column without closing the menu.
pub(crate) fn column_toggle_item<'a>(
    side: SftpPaneSide,
    label: &'a str,
    col: crate::state::SftpColumn,
    visible: bool,
) -> Element<'a, Message> {
    let check = iced_fonts::lucide::check().size(12).color(if visible {
        OryxisColors::t().accent
    } else {
        Color::TRANSPARENT
    });
    button(
        row![
            check,
            Space::new().width(10),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::Sftp(SftpMessage::SftpToggleColumn(side, col)))
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 10.0 })
    .width(Length::Fixed(220.0))
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

/// Right-click row context menu, items vary by pane side and entry
/// type. When the clicked row is part of a multi-selection (same pane),
/// the menu switches to bulk variants: count-aware Delete; single-only
/// ops (Rename, Edit) hide.
/// Pane context the directory-level actions need: the current directory
/// (target of New folder / New file / Refresh), the local path for
/// "Open in File Manager", and the hidden-files toggle state.
#[derive(Clone, Copy)]
pub(crate) struct DirActionCtx<'a> {
    pub pane_dir: &'a str,
    pub local_dir: &'a std::path::Path,
    pub show_hidden: bool,
}

/// Archive-related context for the row menu, computed by the caller
/// (it depends on pane state + the per-mount tool probe, which the
/// menu builder shouldn't reach into).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RowArchiveCtx {
    /// The pane is inside a browsed archive: the row menu collapses to
    /// copy-out + close, and the background menu to close.
    pub in_zip: bool,
    /// Copy-out has a valid destination (the other pane is a local
    /// pane or a connected remote one, and isn't itself browsing).
    pub copy_out_ready: bool,
    /// The clicked row is a zip that can be virtually browsed.
    pub browsable: bool,
    /// The clicked row is an archive this pane can extract.
    pub extractable: bool,
    /// The pane can create archives of each offered kind.
    pub compress_zip: bool,
    pub compress_tgz: bool,
}

// One argument over the lint's limit; the natural regroup (fold the
// cross-pane flags into a ctx struct like RowArchiveCtx) is a refactor
// for the menu's owner, not worth blocking the workspace clippy gate.
// `app` (first arg) records each row into the modal keynav layer so the
// SFTP row menu is keyboard-navigable (arrows move, Enter fires, Esc
// closes); the menu view calls `modal_nav_reset()` before this.
#[allow(clippy::too_many_arguments)]
pub(crate) fn row_context_menu_box<'a>(
    app: &crate::app::Oryxis,
    menu: &crate::state::SftpRowMenu,
    cross_pane_ready: bool,
    source_is_remote: bool,
    other_is_remote: bool,
    other_label: Option<String>,
    selection_count_same_pane: usize,
    archive: RowArchiveCtx,
    dir_ctx: DirActionCtx<'_>,
) -> Element<'a, Message> {
    let multi = selection_count_same_pane > 1;
    let mut items = column![].spacing(2).padding(4);
    let accent = OryxisColors::t().accent;
    let secondary = OryxisColors::t().text_secondary;
    let danger = OryxisColors::t().error;
    // Build one actionable row, record it for the keyboard router (in
    // call order == display order), and ring it when selected.
    let slot = |icon: iced::widget::Text<'a>, label: String, msg: Message, color: Color| {
        sftp_menu_slot(app, msg.clone(), menu_item_owned_tinted(icon, label, msg, color))
    };
    // Inside a browsed archive the listing is virtual and read-only:
    // the whole menu collapses to copy-out (rows) + leave (both).
    if archive.in_zip {
        if !menu.is_background && archive.copy_out_ready {
            let msg = Message::Sftp(SftpMessage::SftpZipCopyOut(menu.side, menu.path.clone(), menu.is_dir));
            items = items.push(sftp_menu_slot(
                app,
                msg.clone(),
                menu_item_tinted(iced_fonts::lucide::download(), t("archive_copy_out"), msg, accent),
            ));
        }
        let msg = Message::Sftp(SftpMessage::SftpZipClose(menu.side));
        items = items.push(sftp_menu_slot(
            app,
            msg.clone(),
            menu_item_tinted(iced_fonts::lucide::x(), t("archive_close"), msg, secondary),
        ));
        return context_menu_shell(items);
    }
    // Background right-click (empty area): only directory-level actions,
    // no per-entry target exists. Same items as the pane's `⋮` menu, plus
    // the per-host landing folder, whose natural target IS the directory
    // the user is looking at.
    if menu.is_background {
        for (msg, it) in dir_action_items(menu.side, source_is_remote, dir_ctx, true) {
            items = items.push(match msg {
                Some(m) => sftp_menu_slot(app, m, it),
                None => it,
            });
        }
        for (msg, it) in initial_path_items(app, menu.side, source_is_remote, &menu.path) {
            items = items.push(match msg {
                Some(m) => sftp_menu_slot(app, m, it),
                None => it,
            });
        }
        return context_menu_shell(items);
    }
    // Cross-pane action, picked by the source and the opposite pane's
    // natures: Local -> remote uploads, remote -> Local downloads,
    // remote -> remote relays. Only offered when the other pane is a
    // ready destination (connected remote, or a Local pane).
    if !source_is_remote && other_is_remote {
        // Upload to the (remote) other pane.
        if cross_pane_ready {
            if multi {
                items = items.push(slot(
                    iced_fonts::lucide::upload(),
                    t("upload_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1),
                    Message::Sftp(SftpMessage::SftpUploadSelection),
                    accent,
                ));
            } else {
                let upload_msg = if menu.is_dir {
                    Message::Sftp(SftpMessage::SftpUploadFolder(std::path::PathBuf::from(&menu.path)))
                } else {
                    // Route even a single file through the batch queue so the
                    // transfer shows the progress strip + per-file panel
                    // (SftpUpload alone creates no TransferState, hence no
                    // on-screen indicator).
                    Message::Sftp(SftpMessage::SftpUploadBatch(vec![std::path::PathBuf::from(&menu.path)]))
                };
                let upload_label = match &other_label {
                    Some(h) => t("upload_to_host").replacen("{host}", h, 1),
                    None => t("upload_to_host").replacen("{host}", t("the_other_host"), 1),
                };
                items = items.push(slot(
                    iced_fonts::lucide::upload(),
                    upload_label,
                    upload_msg,
                    accent,
                ));
            }
        }
        // Open the local file in the OS default editor.
        if !multi && !menu.is_dir {
            items = items.push(slot(
                iced_fonts::lucide::pencil(),
                crate::i18n::t("edit").to_string(),
                Message::Sftp(SftpMessage::SftpOpenLocal(std::path::PathBuf::from(&menu.path))),
                secondary,
            ));
        }
    } else if source_is_remote && !other_is_remote {
        // Download to the (Local) other pane.
        if cross_pane_ready {
            if multi {
                items = items.push(slot(
                    iced_fonts::lucide::download(),
                    t("download_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1),
                    Message::Sftp(SftpMessage::SftpDownloadSelection),
                    accent,
                ));
            } else {
                let download_msg = if menu.is_dir {
                    Message::Sftp(SftpMessage::SftpDownloadFolder(menu.path.clone()))
                } else {
                    Message::Sftp(SftpMessage::SftpDownload(menu.path.clone()))
                };
                items = items.push(slot(
                    iced_fonts::lucide::download(),
                    t("download_to_local").to_string(),
                    download_msg,
                    accent,
                ));
            }
        }
        // Same transfer, destination picked by hand. Deliberately outside
        // the `cross_pane_ready` gate: this one brings its own
        // destination, so it works even when the other pane is not a
        // usable one. The `sftp_ask_download_dir` setting makes the entry
        // above ask too; this is how you ask just this once.
        {
            let pick_msg = if multi {
                SftpMessage::SftpDownloadSelection
            } else if menu.is_dir {
                SftpMessage::SftpDownloadFolder(menu.path.clone())
            } else {
                SftpMessage::SftpDownload(menu.path.clone())
            };
            items = items.push(slot(
                iced_fonts::lucide::folder_down(),
                t("sftp_download_to").to_string(),
                Message::Sftp(SftpMessage::SftpDownloadTo(Box::new(pick_msg))),
                secondary,
            ));
        }
        if !multi && !menu.is_dir {
            for (msg, it) in open_family_items(menu, secondary) {
                items = items.push(match msg {
                    Some(m) => sftp_menu_slot(app, m, it),
                    None => it,
                });
            }
        }
    } else if source_is_remote && other_is_remote {
        // Relay to the other (remote) host. Single-item only for now,
        // multi falls back to per-item relays via the row each.
        if cross_pane_ready {
            let label = match &other_label {
                Some(h) => t("relay_to_remote").replacen("{host}", h, 1),
                None => t("relay_to_remote").replacen("{host}", t("the_other_host"), 1),
            };
            let relay_msg = if menu.is_dir {
                Message::Sftp(SftpMessage::SftpRelayFolder(menu.side, menu.path.clone()))
            } else {
                Message::Sftp(SftpMessage::SftpRelay(menu.side, menu.path.clone()))
            };
            items = items.push(slot(
                iced_fonts::lucide::arrow_right_left(),
                label,
                relay_msg,
                accent,
            ));
            // Move: the same transfer, with the source removed once every
            // file is verified on the other host. Deliberately a separate
            // entry rather than a modifier on the relay, so a move is
            // always something the user asked for by name.
            let move_label = match &other_label {
                Some(h) => t("move_to_remote").replacen("{host}", h, 1),
                None => t("move_to_remote").replacen("{host}", t("the_other_host"), 1),
            };
            let move_msg = if menu.is_dir {
                Message::Sftp(SftpMessage::SftpRelayMoveFolder(
                    menu.side,
                    menu.path.clone(),
                ))
            } else {
                Message::Sftp(SftpMessage::SftpRelayMove(menu.side, menu.path.clone()))
            };
            items = items.push(slot(
                iced_fonts::lucide::corner_up_right(),
                move_label,
                move_msg,
                accent,
            ));
        }
        if !multi && !menu.is_dir {
            for (msg, it) in open_family_items(menu, secondary) {
                items = items.push(match msg {
                    Some(m) => sftp_menu_slot(app, m, it),
                    None => it,
                });
            }
        }
    }
    // Archive actions. Browse / Extract act on the clicked archive
    // (single selection); Compress packs the clicked row or the whole
    // selection containing it. Remote availability comes from the
    // per-mount tool probe (bsdtar/unzip/zip), local from in-process
    // codecs, both resolved by the caller into `archive`.
    if !multi && archive.browsable {
        items = items.push(slot(
            iced_fonts::lucide::folder_search(),
            t("archive_browse").to_string(),
            Message::Sftp(SftpMessage::SftpZipOpen(menu.side, menu.path.clone())),
            accent,
        ));
    }
    if !multi && !menu.is_dir && archive.extractable {
        items = items.push(slot(
            iced_fonts::lucide::package_open(),
            t("archive_extract_here").to_string(),
            Message::Sftp(SftpMessage::SftpArchiveExtract(menu.side, menu.path.clone())),
            secondary,
        ));
    }
    if archive.compress_zip {
        items = items.push(slot(
            iced_fonts::lucide::archive(),
            t("archive_compress_zip").to_string(),
            Message::Sftp(SftpMessage::SftpArchiveCompress(
                menu.side,
                oryxis_archive::names::ArchiveKind::Zip,
                menu.path.clone(),
            )),
            secondary,
        ));
    }
    if archive.compress_tgz {
        items = items.push(slot(
            iced_fonts::lucide::archive(),
            t("archive_compress_tgz").to_string(),
            Message::Sftp(SftpMessage::SftpArchiveCompress(
                menu.side,
                oryxis_archive::names::ArchiveKind::TarGz,
                menu.path.clone(),
            )),
            secondary,
        ));
    }
    // Reveal in the OS file manager, local pane only (no notion of an
    // "explorer" for a remote host). Single selection: a folder opens in
    // place, a file opens its folder with the file selected.
    if !source_is_remote && !multi {
        items = items.push(slot(
            iced_fonts::lucide::folder_open(),
            crate::i18n::open_in_file_manager_label().to_string(),
            Message::Sftp(SftpMessage::SftpRevealInExplorer(std::path::PathBuf::from(&menu.path), menu.is_dir)),
            secondary,
        ));
    }
    // Copy the full path(s) to the clipboard. The stored path is already
    // side-formatted (POSIX for remote, OS-native for local); the bulk
    // variant emits one path per line.
    if multi {
        items = items.push(slot(
            iced_fonts::lucide::clipboard_copy(),
            t("copy_n_paths").replacen("{n}", &selection_count_same_pane.to_string(), 1),
            Message::Sftp(SftpMessage::SftpCopySelectionPaths(menu.side)),
            secondary,
        ));
    } else {
        items = items.push(slot(
            iced_fonts::lucide::clipboard_copy(),
            t("copy_path").to_string(),
            Message::Sftp(SftpMessage::SftpCopyPath(menu.path.clone())),
            secondary,
        ));
    }
    if multi {
        items = items.push(slot(
            iced_fonts::lucide::copy(),
            t("duplicate_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1),
            Message::Sftp(SftpMessage::SftpDuplicateSelection),
            secondary,
        ));
    } else {
        let duplicate_msg = if menu.is_dir {
            Message::Sftp(SftpMessage::SftpDuplicateFolder(menu.side, menu.path.clone()))
        } else {
            Message::Sftp(SftpMessage::SftpDuplicate(menu.side, menu.path.clone()))
        };
        items = items.push(slot(
            iced_fonts::lucide::copy(),
            t("duplicate").to_string(),
            duplicate_msg,
            secondary,
        ));
        items = items.push(slot(
            iced_fonts::lucide::pencil(),
            t("rename").to_string(),
            Message::Sftp(SftpMessage::SftpStartRename(menu.side, menu.path.clone())),
            secondary,
        ));
        items = items.push(slot(
            iced_fonts::lucide::cog(),
            t("properties").to_string(),
            Message::Sftp(SftpMessage::SftpShowProperties(menu.side, menu.path.clone(), menu.is_dir)),
            secondary,
        ));
    }
    let delete_label = if multi {
        t("delete_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1)
    } else {
        t("delete").to_string()
    };
    let delete_msg = if multi {
        Message::Sftp(SftpMessage::SftpAskDeleteSelection)
    } else {
        Message::Sftp(SftpMessage::SftpAskDelete(menu.side, menu.path.clone(), menu.is_dir))
    };
    items = items.push(slot(
        iced_fonts::lucide::trash(),
        delete_label,
        delete_msg,
        danger,
    ));

    // Directory-level actions appended below the per-entry block, like
    // FileZilla's row menu (create folder/file + refresh act on the
    // pane's current directory, not the clicked entry).
    items = items.push(menu_separator());
    for (msg, it) in dir_action_items(menu.side, source_is_remote, dir_ctx, false) {
        items = items.push(match msg {
            Some(m) => sftp_menu_slot(app, m, it),
            None => it,
        });
    }
    // Landing folder: on a row menu the target is the clicked FOLDER, not
    // the pane's directory, so a user can pin a subfolder without entering
    // it first. File rows have no landing folder to set.
    if menu.is_dir && !multi {
        for (msg, it) in initial_path_items(app, menu.side, source_is_remote, &menu.path) {
            items = items.push(match msg {
                Some(m) => sftp_menu_slot(app, m, it),
                None => it,
            });
        }
    }

    context_menu_shell(items)
}

/// Record + ring one SFTP context-menu row so the keyboard router
/// (`ModalSurface::SftpRowMenu`) can move to it and Enter fires its
/// message. Menu rows have a 4px corner radius; the accent ring reads
/// fine on their transparent / hover background.
fn sftp_menu_slot<'a>(
    app: &crate::app::Oryxis,
    msg: Message,
    el: Element<'a, Message>,
) -> Element<'a, Message> {
    app.modal_nav_slot(crate::keynav::RowAction::activate(msg), 4.0, false, el)
}

/// Directory-level actions for the current pane: New folder, New file,
/// Refresh, and (when `full`) Show hidden + Open in File Manager. `full`
/// is set for the background / `⋮` menus where these are the whole menu;
/// the row menu appends only the create + refresh trio. Open in File
/// Manager stays local-only (no OS explorer for a remote host); the
/// create/refresh/hidden actions apply to both panes.
///
/// Each entry is `(Some(msg), row)` for an actionable row or
/// `(None, separator)` for a divider, so a keyboard-navigable caller (the
/// SFTP row menu) can record the messages in display order while a
/// mouse-only caller (the `⋮` menu) just takes the elements.
/// The per-host "SFTP landing folder" entries for a remote directory
/// target: set it to `dir`, and clear it when the host already has one.
/// Empty for a local pane, for a non-directory target, and for a remote
/// pane whose host isn't a saved connection (nothing to store it on).
pub(crate) fn initial_path_items<'a>(
    app: &crate::app::Oryxis,
    side: SftpPaneSide,
    is_remote: bool,
    dir: &str,
) -> Vec<(Option<Message>, Element<'a, Message>)> {
    if !is_remote {
        return Vec::new();
    }
    let Some(conn) = app
        .sftp
        .pane(side)
        .host_label
        .as_ref()
        .and_then(|label| app.connections.iter().find(|c| &c.label == label))
    else {
        return Vec::new();
    };
    let mut out: Vec<(Option<Message>, Element<'a, Message>)> = Vec::new();
    // Already the landing folder: offer only the clear, so the two entries
    // never read as contradictory.
    let already = conn.sftp_initial_path.as_deref() == Some(dir);
    if !already {
        let set = Message::Sftp(SftpMessage::SftpSetInitialPath(side, dir.to_string()));
        out.push((
            Some(set.clone()),
            menu_item(iced_fonts::lucide::folder_check(), t("sftp_set_initial_path"), set),
        ));
    }
    if conn.sftp_initial_path.is_some() {
        let clear = Message::Sftp(SftpMessage::SftpClearInitialPath(side));
        out.push((
            Some(clear.clone()),
            menu_item(iced_fonts::lucide::folder_x(), t("sftp_clear_initial_path"), clear),
        ));
    }
    // Leading separator only when something follows it; `None` keeps it out
    // of the keyboard walk (a separator is not a row).
    if !out.is_empty() {
        out.insert(0, (None, menu_separator()));
    }
    out
}

/// The Open / Edit family for a single remote file (issues #84, #114).
///
/// "Open / Edit" (the OS file association) stays a top-level, one-click
/// row because it is the common case. Everything that picks a specific
/// application hides behind an expandable "Open with" row, so the menu
/// keeps its length: the group's own row toggles `open_group` WITHOUT
/// closing the menu, the same trick the Columns toggles use.
///
/// Every entry downloads a temp copy, spawns the application and
/// registers the background watch that confirms each save.
fn open_family_items<'a>(
    menu: &crate::state::SftpRowMenu,
    tint: Color,
) -> Vec<(Option<Message>, Element<'a, Message>)> {
    // One entry, not "Edit": a remote file may well be an image or a PDF,
    // where the local application opens rather than edits it. Same
    // background watch either way, so a save that never comes costs
    // nothing.
    let open = Message::Sftp(SftpMessage::SftpStartEdit(menu.side, menu.path.clone()));
    let toggle = Message::Sftp(SftpMessage::SftpToggleOpenGroup);
    let mut items: Vec<(Option<Message>, Element<'a, Message>)> = vec![
        (
            Some(open.clone()),
            menu_item_owned_tinted(
                iced_fonts::lucide::external_link(),
                crate::i18n::t("sftp_open_edit").to_string(),
                open,
                tint,
            ),
        ),
        (
            Some(toggle.clone()),
            menu_item_owned_tinted(
                if menu.open_group {
                    iced_fonts::lucide::chevron_down()
                } else {
                    iced_fonts::lucide::chevron_right()
                },
                crate::i18n::t("sftp_open_with_group").to_string(),
                toggle,
                tint,
            ),
        ),
    ];
    if !menu.open_group {
        return items;
    }
    let with = |opener: crate::state::SftpEditOpener| {
        Message::Sftp(SftpMessage::SftpStartEditWith(
            menu.side,
            menu.path.clone(),
            opener,
        ))
    };
    let mut sub: Vec<(Message, iced::widget::Text<'a>, String)> = vec![
        (
            with(crate::state::SftpEditOpener::ConfiguredEditor),
            iced_fonts::lucide::pen_line(),
            crate::i18n::t("sftp_open_with_editor").to_string(),
        ),
        (
            Message::Sftp(SftpMessage::SftpPickEditorFor(menu.side, menu.path.clone())),
            iced_fonts::lucide::app_window(),
            crate::i18n::t("sftp_open_with_other").to_string(),
        ),
    ];
    // The OS application picker has no stable cross-desktop CLI on Linux;
    // the entry only shows where it actually works. "Other application..."
    // above is the cross-platform stand-in.
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        sub.push((
            with(crate::state::SftpEditOpener::AskOs),
            iced_fonts::lucide::layout_grid(),
            crate::i18n::t("sftp_open_with").to_string(),
        ));
    }
    // Setting the default from here is the point of the group: the
    // reporter's ask was to pick the editor where it is used, not only in
    // Settings > SFTP. Same message the settings row's Browse fires.
    sub.push((
        Message::Settings(crate::app::SettingsMessage::SettingSftpDefaultEditorBrowse),
        iced_fonts::lucide::settings(),
        crate::i18n::t("sftp_set_default_editor").to_string(),
    ));
    for (msg, icon, label) in sub {
        items.push((
            Some(msg.clone()),
            menu_sub_item(icon, label, msg, tint),
        ));
    }
    items
}

/// A row nested under an expanded menu group: same shape as
/// `menu_item_owned_tinted`, indented so the grouping reads without a
/// second card.
fn menu_sub_item<'a>(
    icon: iced::widget::Text<'a>,
    label: String,
    msg: Message,
    tint: Color,
) -> Element<'a, Message> {
    button(
        row![
            Space::new().width(14),
            icon.size(12).color(tint),
            Space::new().width(10),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 10.0 })
    .width(Length::Fixed(220.0))
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

pub(crate) fn dir_action_items<'a>(
    side: SftpPaneSide,
    is_remote: bool,
    ctx: DirActionCtx<'_>,
    full: bool,
) -> Vec<(Option<Message>, Element<'a, Message>)> {
    let refresh_msg = if is_remote {
        Message::Sftp(SftpMessage::SftpNavigateRemote(side, ctx.pane_dir.to_string()))
    } else {
        Message::Sftp(SftpMessage::SftpRefreshLocal(side))
    };
    let new_folder = Message::Sftp(SftpMessage::SftpStartNewEntry(side, SftpEntryKind::Folder));
    let new_file = Message::Sftp(SftpMessage::SftpStartNewEntry(side, SftpEntryKind::File));
    let mut items: Vec<(Option<Message>, Element<'a, Message>)> = vec![
        (
            Some(new_folder.clone()),
            menu_item(iced_fonts::lucide::folder_plus(), t("new_folder"), new_folder),
        ),
        (
            Some(new_file.clone()),
            menu_item(iced_fonts::lucide::file_plus(), t("new_file"), new_file),
        ),
    ];
    if full {
        items.push((None, menu_separator()));
    }
    items.push((
        Some(refresh_msg.clone()),
        menu_item(iced_fonts::lucide::rotate_cw(), t("refresh"), refresh_msg),
    ));
    if full {
        // Copy the pane's current directory path. `pane_dir` is already
        // side-formatted by the caller (remote path or local display).
        let copy_msg = Message::Sftp(SftpMessage::SftpCopyPath(ctx.pane_dir.to_string()));
        items.push((
            Some(copy_msg.clone()),
            menu_item(iced_fonts::lucide::clipboard_copy(), t("copy_path"), copy_msg),
        ));
        let hidden_label =
            if ctx.show_hidden { t("hide_hidden_files") } else { t("show_hidden_files") };
        let hidden_msg = Message::Sftp(SftpMessage::SftpToggleHidden(side));
        items.push((
            Some(hidden_msg.clone()),
            menu_item(iced_fonts::lucide::eye(), hidden_label, hidden_msg),
        ));
        if !is_remote {
            items.push((None, menu_separator()));
            let reveal_msg =
                Message::Sftp(SftpMessage::SftpRevealInExplorer(ctx.local_dir.to_path_buf(), true));
            items.push((
                Some(reveal_msg.clone()),
                menu_item(
                    iced_fonts::lucide::folder_open(),
                    crate::i18n::open_in_file_manager_label(),
                    reveal_msg,
                ),
            ));
        }
    }
    items
}

/// Shared shell for the cursor-anchored SFTP context menus (row +
/// background): fixed width so the `Length::Fill` separators don't
/// stretch the popover, plus the surface/border/shadow styling.
pub(crate) fn context_menu_shell<'a>(
    items: iced::widget::Column<iced::Element<'a, Message>>,
) -> Element<'a, Message> {
    container(items)
        .width(Length::Fixed(ROW_CONTEXT_MENU_WIDTH))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        })
        .boxed()
}

/// Compute the approximate height of the row context menu given the
/// current target, keeps the layout-level clamp accurate so the menu
/// never spills off the bottom or right edge of the window.
pub(crate) fn row_context_menu_height(
    app: &crate::app::Oryxis,
    menu: &crate::state::SftpRowMenu,
    cross_pane_ready: bool,
    source_is_remote: bool,
    other_is_remote: bool,
    selection_count_same_pane: usize,
    archive: RowArchiveCtx,
) -> f32 {
    // Zip-browse mode: copy-out (rows with a ready destination) +
    // Close archive.
    if archive.in_zip {
        let items = if !menu.is_background && archive.copy_out_ready {
            2.0
        } else {
            1.0
        };
        return items * 30.0 + 8.0;
    }
    // Background menu: directory actions only. New folder + New file +
    // Refresh + Copy path + Show hidden (5), plus Open in File Manager
    // on a local pane (6), plus ~2 thin separators.
    if menu.is_background {
        let items = if source_is_remote { 5.0 } else { 6.0 };
        let separators = if source_is_remote { 1.0 } else { 2.0 };
        // Landing-folder rows (+ their own separator) when the pane's host
        // can carry one; counted from the same helper that builds them.
        let extra = initial_path_items(app, menu.side, source_is_remote, &menu.path);
        let extra_rows = extra.iter().filter(|(m, _)| m.is_some()).count() as f32;
        let extra_seps = if extra.is_empty() { 0.0 } else { 1.0 };
        return (items + extra_rows) * 30.0 + (separators + extra_seps) * 4.0 + 8.0;
    }
    let multi = selection_count_same_pane > 1;
    // Always present: Copy path + Duplicate + Rename + Properties +
    // Delete (single), or Copy N paths + Duplicate + Delete (multi).
    let mut count = if multi { 3.0 } else { 5.0 };
    // Cross-pane action (Upload / Download / Relay) when the other pane
    // is a ready destination.
    if cross_pane_ready {
        count += 1.0;
    }
    // Edit-in-place / open-local for a single remote-source file, or a
    // single local file when uploading.
    if !multi && !menu.is_dir {
        let editable = source_is_remote || other_is_remote;
        if editable {
            count += 1.0;
        }
    }
    // Archive actions: Browse (single zip) + Extract (single archive
    // file) + Compress to zip / tar.gz.
    if !multi && archive.browsable {
        count += 1.0;
    }
    if !multi && !menu.is_dir && archive.extractable {
        count += 1.0;
    }
    if archive.compress_zip {
        count += 1.0;
    }
    if archive.compress_tgz {
        count += 1.0;
    }
    // "Open in File Manager" for a single local-pane entry.
    if !source_is_remote && !multi {
        count += 1.0;
    }
    // Appended directory actions (New folder + New file + Refresh) plus
    // their leading separator.
    count += 3.0;
    // Landing-folder rows on a folder target, same helper as the builder.
    let mut extra_seps = 0.0;
    if menu.is_dir && !multi {
        let extra = initial_path_items(app, menu.side, source_is_remote, &menu.path);
        count += extra.iter().filter(|(m, _)| m.is_some()).count() as f32;
        if !extra.is_empty() {
            extra_seps = 1.0;
        }
    }
    // Each item ~30px (padding 6+6 + ~12px text + 2px gap) plus 8px
    // container padding, plus one thin separator above the dir actions.
    count * 30.0 + (1.0 + extra_seps) * 4.0 + 8.0
}

/// Width is fixed because every item uses the same `menu_item` width.
pub(crate) const ROW_CONTEXT_MENU_WIDTH: f32 = 220.0;

/// Owned-label variant of `menu_item` for cases where the label is
/// computed at runtime (e.g. "Delete N items" with a dynamic count).
/// Owned-label variant that lets the caller pick the icon tint
/// used for destructive (red) and primary (accent / success) actions
/// to match the host-card context menu's color coding.
pub(crate) fn menu_item_owned_tinted<'a>(
    icon: iced::widget::Text<'a>,
    label: String,
    msg: Message,
    tint: Color,
) -> Element<'a, Message> {
    button(
        row![
            icon.size(12).color(tint),
            Space::new().width(10),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 10.0 })
    .width(Length::Fixed(220.0))
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

pub(crate) fn menu_item<'a>(
    icon: iced::widget::Text<'a>,
    label: &'a str,
    msg: Message,
) -> Element<'a, Message> {
    menu_item_tinted(icon, label, msg, OryxisColors::t().text_secondary)
}

/// Like `menu_item` but with an explicit icon tint (red for delete,
/// accent for primary actions, etc.).
pub(crate) fn menu_item_tinted<'a>(
    icon: iced::widget::Text<'a>,
    label: &'a str,
    msg: Message,
    tint: Color,
) -> Element<'a, Message> {
    button(
        row![
            icon.size(12).color(tint),
            Space::new().width(10),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 10.0 })
    .width(Length::Fixed(220.0))
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

/// Combo-box arrow on the trailing edge of the path bar: opens this
/// pane's visited-directory history (issue #85). Only rendered once the
/// pane has history, so it never opens an empty list.
pub(crate) fn path_history_button<'a>(
    side: SftpPaneSide,
    open: bool,
) -> Element<'a, Message> {
    let glyph = if open {
        iced_fonts::lucide::chevron_up()
    } else {
        iced_fonts::lucide::chevron_down()
    };
    let btn = button(
        container(glyph.size(11).color(OryxisColors::t().text_secondary))
            .center_x(Length::Fixed(20.0))
            .center_y(Length::Fixed(18.0)),
    )
    .on_press(Message::Sftp(SftpMessage::SftpPathHistoryToggle(side)))
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
            _ if open => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    });
    crate::views::terminal::icon_tooltip(btn.boxed(), t("sftp_path_history"))
}

/// Visited-directory dropdown for the path bar: most recent first,
/// clicking an entry navigates there. Closed via the scrim.
pub(crate) fn path_history_overlay<'a>(
    side: SftpPaneSide,
    history: &'a [String],
) -> Element<'a, Message> {
    let mut col = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(2).padding(4);
    for path in history {
        col = col.push(
            button(
                row![
                    iced_fonts::lucide::history()
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                    Space::new().width(8),
                    text(path.clone())
                        .size(12)
                        .color(OryxisColors::t().text_primary),
                ]
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::Sftp(SftpMessage::SftpPathHistoryPick(
                side,
                path.clone(),
            )))
            .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 10.0 })
            .width(Length::Fixed(360.0))
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
    }
    // A long history scrolls instead of running off the pane.
    let menu = container(
        iced::widget::scrollable::<_, _, iced::Theme>(col).height(Length::Fixed(
            (history.len() as f32 * 30.0 + 8.0).min(320.0),
        )),
    )
    .style(|_: &iced::Theme| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    });
    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new()).width(Length::Fill).height(Length::Fill),
    )
    .on_press(Message::Sftp(SftpMessage::SftpPathHistoryClose))
    .boxed();
    // Anchored under the path bar, hugging the pane's trailing edge
    // (where the arrow that opened it sits). The path bar is a
    // `dir_row`, so under RTL the arrow sits on the physical LEFT and
    // the dropdown must follow it there.
    let rtl = crate::i18n::is_rtl_layout();
    let (align, pad) = if rtl {
        (
            iced::alignment::Horizontal::Left,
            Padding { top: 70.0, right: 0.0, bottom: 0.0, left: 14.0 },
        )
    } else {
        (
            iced::alignment::Horizontal::Right,
            Padding { top: 70.0, right: 14.0, bottom: 0.0, left: 0.0 },
        )
    };
    let positioned = container::<_, iced::Theme>(menu)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(align)
        .align_y(iced::alignment::Vertical::Top)
        .padding(pad);
    iced::widget::Stack::<iced::Element<'_, _>>::new().push(scrim).push(positioned.boxed()).boxed()
}

/// Drive picker dropdown for Windows local pane. Lists `C:`, `D:`, etc.
/// based on what's actually mounted. Closed via the scrim.
pub(crate) fn drives_menu_overlay<'a>(side: SftpPaneSide) -> Element<'a, Message> {
    let drives = list_windows_drives_cached();
    let mut col = iced::widget::Column::<iced::Element<'_, _>>::new().spacing(2).padding(4);
    if drives.is_empty() {
        col = col.push(
            container(text(t("no_drives_detected")).size(11).color(OryxisColors::t().text_muted))
                .padding(8).boxed(),
        );
    } else {
        for drive in drives {
            let drive_path: std::path::PathBuf = format!("{}\\", drive).into();
            col = col.push(
                button(
                    row![
                        iced_fonts::lucide::hard_drive()
                            .size(12)
                            .color(OryxisColors::t().accent),
                        Space::new().width(8),
                        text(drive.clone()).size(12).color(OryxisColors::t().text_primary),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::Sftp(SftpMessage::SftpNavigateLocal(side, drive_path)))
                .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 10.0 })
                .width(Length::Fixed(160.0))
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
        }
    }
    let menu = container(col).style(|_: &iced::Theme| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    });
    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new()).width(Length::Fill).height(Length::Fill),
    )
    .on_press(Message::Sftp(SftpMessage::SftpCloseMenus))
    .boxed();
    let positioned = container::<_, iced::Theme>(menu)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Left)
        .align_y(iced::alignment::Vertical::Top)
        .padding(Padding { top: 70.0, right: 0.0, bottom: 0.0, left: 14.0 });
    iced::widget::Stack::<iced::Element<'_, _>>::new().push(scrim).push(positioned.boxed()).boxed()
}

/// True when the path's first component is a real Windows volume
/// (`C:\`, `D:\`, including the `\\?\C:\` verbatim form). UNC paths
/// like `\\server\share` or `\\wsl$\Ubuntu` return false, those are
/// served by Unix-style filesystems where `/` reads more naturally.
pub(crate) fn is_windows_disk_path(path: &std::path::Path) -> bool {
    matches!(
        path.components().next(),
        Some(std::path::Component::Prefix(p))
            if matches!(
                p.kind(),
                std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
            )
    )
}

/// Cached front for `list_windows_drives`. The raw probe touches the
/// filesystem for every drive letter (A: through Z:) and stats
/// `wsl.exe`, far too heavy to run per frame while the drive popover
/// is open, so the result is reused for a few seconds. Plugging or
/// unplugging a drive shows up on the next refresh window.
pub(crate) fn list_windows_drives_cached() -> Vec<String> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    const TTL: Duration = Duration::from_secs(5);
    static CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
    // Shrug off poisoning: the cache is a plain probe result, safe to
    // reuse, and a panic inside view() while unwinding another panic
    // would turn a recoverable crash into an abort.
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((probed_at, drives)) = guard.as_ref()
        && probed_at.elapsed() < TTL
    {
        return drives.clone();
    }
    let drives = list_windows_drives();
    *guard = Some((Instant::now(), drives.clone()));
    drives
}

/// Enumerate available drive letters on Windows. Empty on non-Windows
/// hosts (the dropdown isn't rendered there). When running under WSL,
/// surface `\\wsl.localhost` as a synthetic root so the user can hop
/// between WSL distros without dropping to a terminal.
pub(crate) fn list_windows_drives() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        let mut drives = Vec::new();
        for letter in b'A'..=b'Z' {
            let path = format!("{}:\\", letter as char);
            if std::path::Path::new(&path).exists() {
                drives.push(format!("{}:", letter as char));
            }
        }
        // WSL distros live under \\wsl.localhost (or the legacy
        // \\wsl$). `Path::exists()` on a UNC root returns false until
        // the SMB redirector lazily mounts it, so we detect WSL via
        // `wsl.exe` in System32, present iff the user has WSL
        // installed at all. We expose `\\wsl$` as the entry point
        // because it's the alias that always resolves; navigating into
        // it lists distros as folders.
        let wsl_exe = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
            .join("System32")
            .join("wsl.exe");
        if wsl_exe.exists() {
            drives.push(r"\\wsl$".to_string());
        }
        drives
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}
