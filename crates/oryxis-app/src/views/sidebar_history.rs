//! History sidebar tab: the focused host's captured command history, a
//! "Frequent" top-3 shortlist over a most-recent list, with the same
//! hover-revealed floating row actions as the Snippets tab (Paste / Run /
//! Delete). Rows re-insert like snippets: click = paste without Enter.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::{column, container, text, MouseArea, Space};
use iced::{Background, Border, Element, Length, Padding};

use super::terminal::chat_header_btn;
use crate::app::{CommandHistoryMessage, Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

/// Frequency floor for the "Frequent" shortlist: a command used once is
/// recent, not frequent.
const FREQUENT_MIN_USES: i64 = 2;

impl Oryxis {
    pub(crate) fn history_tab_content(&self) -> Element<'_, Message> {
        // No saved host focused: nothing to key history on.
        if self.command_history_host.is_none() {
            return sidebar_placeholder(t("history_unavailable"));
        }
        if self.command_history.is_empty() {
            return sidebar_placeholder(t("history_empty"));
        }

        // Focus target for SelectTerminalSidebarTab / the sidebar
        // hotkey (entering History lands the keyboard here), and an
        // input row in the Tab walk.
        let search = self.sidebar_nav_slot(
            crate::keynav::SidebarRow::input(iced::widget::Id::new("sidebar-history-search")),
            crate::state::TerminalSidebarTab::History,
            crate::widgets::INPUT_RADIUS,
            iced::widget::text_input(t("search"), &self.cmd_history_search)
                .id(iced::widget::Id::new("sidebar-history-search"))
                .on_input(|v| Message::CommandHistory(CommandHistoryMessage::CmdHistorySearchChanged(v)))
                .padding(8)
                .size(13)
                .style(crate::widgets::rounded_input_style)
                .boxed(),
        );
        // Export the captured commands to a plain-text file (offline
        // reference / support sharing). Recorded after the search,
        // matching the display order.
        let export_btn = self.sidebar_nav_slot(
            crate::keynav::SidebarRow::button(Message::CommandHistory(CommandHistoryMessage::ExportCommandHistory)),
            crate::state::TerminalSidebarTab::History,
            6.0,
            action_btn(
                iced_fonts::lucide::file_down(),
                Message::CommandHistory(CommandHistoryMessage::ExportCommandHistory),
                t("history_export_tip"),
            ),
        );
        let header = container(
            dir_row(vec![search, Space::new().width(6).boxed(), export_btn])
                .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 10.0, right: 12.0, bottom: 8.0, left: 12.0 })
        .width(Length::Fill);

        let needle = self.cmd_history_search.to_lowercase();
        let mut list = iced::widget::Column::<iced::Element<'_, _>>::new()
            .spacing(6)
            .padding(Padding { top: 0.0, right: 12.0, bottom: 12.0, left: 12.0 });

        // `hovered_history_card` indexes display positions (frequent rows
        // first, then recent), so each rendered row gets a unique index
        // even though frequent entries also appear in the recent list.
        let mut pos = 0usize;

        // Frequent shortlist: top 3 by use count. Hidden while searching,
        // the search results below are the whole answer then.
        if needle.is_empty() {
            let mut frequent: Vec<&oryxis_vault::CommandHistoryEntry> = self
                .command_history
                .iter()
                .filter(|e| e.use_count >= FREQUENT_MIN_USES)
                .collect();
            frequent.sort_by(|a, b| {
                b.use_count
                    .cmp(&a.use_count)
                    .then(b.last_used_at.cmp(&a.last_used_at))
            });
            if !frequent.is_empty() {
                list = list.push(section_label(t("history_frequent")));
                for entry in frequent.into_iter().take(3) {
                    list = list.push(self.recorded_history_row(entry, pos));
                    pos += 1;
                }
                list = list.push(section_label(t("history_recent")));
            }
        }

        let mut any = false;
        for entry in &self.command_history {
            if !needle.is_empty() && !entry.command.to_lowercase().contains(&needle) {
                continue;
            }
            any = true;
            list = list.push(self.recorded_history_row(entry, pos));
            pos += 1;
        }
        if !any {
            list = list.push(sidebar_placeholder(t("no_matches")));
        }

        // Shared id with the Snippets list (only one renders): the
        // sidebar keynav router snaps the ringed row into view by it.
        let body = iced::widget::scrollable(list)
            .id(crate::keynav::sidebar_scroll_id(crate::state::TerminalSidebarTab::History))
            .width(Length::Fill)
            .height(Length::Fill);
        column![header, body]
            .width(Length::Fill)
            .height(Length::Fill)
            .boxed()
    }

    /// One history row, recorded into the sidebar keynav layer. The
    /// floating actions stay hover-only (owner call: a ringed row
    /// showing them reads as a stuck-hover bug); the ring border is
    /// the keyboard affordance, with Enter = run (owner call),
    /// Shift+Enter = paste without the newline, Delete = remove
    /// (through its confirm).
    fn recorded_history_row<'a>(
        &'a self,
        entry: &'a oryxis_vault::CommandHistoryEntry,
        pos: usize,
    ) -> Element<'a, Message> {
        let tab = crate::state::TerminalSidebarTab::History;
        let row = history_row(entry, pos, self.hover.history_card == Some(pos));
        self.sidebar_nav_slot(
            crate::keynav::SidebarRow::item(
                Message::CommandHistory(CommandHistoryMessage::RunHistoryCommand(entry.id)),
                Message::CommandHistory(CommandHistoryMessage::PasteHistoryCommand(entry.id)),
                Message::CommandHistory(CommandHistoryMessage::RequestDeleteHistoryCommand(entry.id)),
            ),
            tab,
            8.0,
            row,
        )
    }
}

/// Muted section caption ("Frequent" / "Recent").
fn section_label(label: &str) -> Element<'_, Message> {
    container(
        text(label)
            .size(11)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_muted),
    )
    .padding(Padding { top: 4.0, right: 0.0, bottom: 2.0, left: 2.0 })
    .boxed()
}

/// Centered muted text for an empty History tab state.
fn sidebar_placeholder(label: &str) -> Element<'_, Message> {
    container(text(label).size(12).color(OryxisColors::t().text_muted))
        .center_x(Length::Fill)
        .padding(Padding { top: 40.0, right: 12.0, bottom: 0.0, left: 12.0 })
        .width(Length::Fill)
        .boxed()
}

/// An icon action with a tooltip (same chrome as the snippet-row actions).
fn action_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    tip: &'a str,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        chat_header_btn(icon, msg),
        container(text(tip).size(11).color(OryxisColors::t().text_primary))
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Top,
    )
    .boxed()
}

/// One history row: the command (first line, ellipsized) with a muted use
/// count, floating Paste / Run / Delete actions on hover, and click-to-paste
/// on the row itself (the snippet re-insert convention).
fn history_row<'a>(
    entry: &'a oryxis_vault::CommandHistoryEntry,
    pos: usize,
    hovered: bool,
) -> Element<'a, Message> {
    let c = OryxisColors::t();
    let first = entry.command.lines().next().unwrap_or("");
    let multiline = entry.command.lines().nth(1).is_some();
    let preview: String = {
        let head: String = first.chars().take(48).collect();
        if multiline || first.chars().count() > 48 {
            format!("{head}…")
        } else {
            head
        }
    };
    let mut info_row: Vec<Element<'a, Message>> = vec![
        text(preview)
            .size(12)
            .font(iced::Font::MONOSPACE)
            .color(c.text_primary)
            .width(Length::Fill)
            .boxed(),
    ];
    if entry.use_count > 1 {
        info_row.push(Space::new().width(6).boxed());
        info_row.push(
            text(format!("\u{00d7}{}", entry.use_count))
                .size(11)
                .color(c.text_muted)
                .boxed(),
        );
    }
    let card = container(dir_row(info_row).align_y(iced::Alignment::Center))
        .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

    let row_el: Element<'a, Message> = if hovered {
        let actions = container(
            dir_row(vec![
                action_btn(
                    iced_fonts::lucide::clipboard_copy(),
                    Message::CommandHistory(CommandHistoryMessage::PasteHistoryCommand(entry.id)),
                    t("snippet_paste"),
                ),
                action_btn(
                    iced_fonts::lucide::play(),
                    Message::CommandHistory(CommandHistoryMessage::RunHistoryCommand(entry.id)),
                    t("snippet_run"),
                ),
                action_btn(
                    iced_fonts::lucide::trash(),
                    Message::CommandHistory(CommandHistoryMessage::RequestDeleteHistoryCommand(entry.id)),
                    t("delete"),
                ),
            ])
            .spacing(2)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 3.0, right: 5.0, bottom: 3.0, left: 5.0 })
        .style(|_: &iced::Theme| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_selected)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });
        let overlay = container::<_, iced::Theme>(actions)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Center)
            .padding(Padding { top: 0.0, right: 6.0, bottom: 0.0, left: 0.0 });
        iced::widget::Stack::<iced::Element<'_, _>>::new().push(card.boxed()).push(overlay.boxed()).boxed()
    } else {
        card.boxed()
    };

    MouseArea::new(row_el)
        .on_enter(Message::CommandHistory(CommandHistoryMessage::HistoryCardHovered(pos)))
        .on_exit(Message::CommandHistory(CommandHistoryMessage::HistoryCardUnhovered(pos)))
        .on_press(Message::CommandHistory(CommandHistoryMessage::PasteHistoryCommand(entry.id)))
        .boxed()
}
