//! tmux sidebar tab: the tmux sessions running on the focused pane's
//! host (issue #116), with create, attach and kill.
//!
//! The listing, the create and the kill run tmux itself on an exec
//! channel over the pane's live session, so nothing is installed on the
//! host. Attach is the one action that reaches the user's own shell,
//! typed into the pane this tab sits beside on their click.
//!
//! Row actions follow the card convention: the kill icon floats in a
//! `Stack` over the row and appears on hover, so it reserves no inline
//! width and the row content never shifts.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, text, MouseArea, Space, Stack};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, TmuxMessage};
use crate::i18n::t;
use crate::state::TerminalSidebarTab;
use crate::theme::OryxisColors;
use crate::tmux::model::{TmuxSession, TmuxStatus};
use crate::widgets::dir_row;

impl Oryxis {
    pub(crate) fn tmux_tab_content<'a>(
        &'a self,
        tab: &'a crate::state::TerminalTab,
    ) -> Element<'a, Message> {
        // Disconnected mid-view (the tab button hides next frame).
        let Some(tab_idx) = self.active_tab else {
            return placeholder(t("files_no_session"));
        };
        if tab.active().session.as_ref().and_then(|s| s.ssh()).is_none() {
            return placeholder(t("files_no_session"));
        }
        let pane_id = tab.active().id;
        let entry = self.tmux.get(&pane_id);
        let status = entry.map(|e| &e.status).unwrap_or(&TmuxStatus::Idle);

        let mut body = column![]
            .spacing(8)
            .padding(Padding { top: 10.0, right: 10.0, bottom: 12.0, left: 10.0 })
            .width(Length::Fill);

        body = body.push(self.tmux_header(pane_id));

        match status {
            TmuxStatus::Idle | TmuxStatus::Loading => {
                body = body.push(hint(t("tmux_listing")));
            }
            TmuxStatus::NoTmux => {
                // tmux is genuinely absent. Say so plainly instead of
                // offering a New session button that could only fail.
                body = body.push(hint(t("tmux_not_installed")));
            }
            TmuxStatus::Failed(e) => {
                body = body.push(hint(e));
            }
            TmuxStatus::Ready(sessions) => {
                // The sessions are ONE child of `body`, so the new-session
                // input below keeps its index however many there are: a
                // listing lands on its own (a refresh, an attach elsewhere)
                // and a count change used to shift the input and drop the
                // name being typed into it.
                let mut list = column![].spacing(8).width(Length::Fill);
                if sessions.is_empty() {
                    list = list.push(hint(t("tmux_no_sessions")));
                }
                let confirming = entry.and_then(|e| e.confirm_kill.as_deref());
                for (idx, session) in sessions.iter().enumerate() {
                    list = list.push(if confirming == Some(session.name.as_str()) {
                        self.tmux_kill_confirm(pane_id, session)
                    } else {
                        self.tmux_session_row(tab_idx, pane_id, idx, session)
                    });
                }
                body = body.push(list).push(self.tmux_new_session_row(pane_id));
            }
        }

        // Inline error from the last action, in the host's own wording
        // (a duplicate name, a session that vanished meanwhile).
        if let Some(err) = entry.and_then(|e| e.error.as_deref()) {
            body = body.push(
                container(text(err.to_string()).size(11).color(OryxisColors::t().error))
                    .padding(Padding { top: 2.0, right: 4.0, bottom: 0.0, left: 4.0 }),
            );
        }

        iced::widget::scrollable(body)
            .id(crate::keynav::sidebar_scroll_id(crate::state::TerminalSidebarTab::Tmux))
            .height(Length::Fill)
            .into()
    }

    /// Title line + Refresh. Refresh is the first navigable row of the
    /// body, which is where `FocusSidebarList` lands.
    fn tmux_header(&self, pane_id: uuid::Uuid) -> Element<'_, Message> {
        let refresh = crate::views::terminal::icon_tooltip(
            button(
                iced_fonts::lucide::refresh_cw()
                    .size(13)
                    .color(OryxisColors::t().text_secondary),
            )
            .on_press(Message::Tmux(TmuxMessage::Refresh(pane_id)))
            .padding(Padding { top: 4.0, right: 6.0, bottom: 4.0, left: 6.0 })
            .style(icon_btn_style)
            .into(),
            t("tmux_refresh"),
        );
        dir_row(vec![
            text(t("tmux_sessions"))
                .size(11)
                .color(OryxisColors::t().text_secondary)
                .into(),
            Space::new().width(Length::Fill).into(),
            self.sidebar_nav_slot(
                crate::keynav::SidebarRow::button(Message::Tmux(TmuxMessage::Refresh(pane_id))),
                TerminalSidebarTab::Tmux,
                6.0,
                refresh,
            ),
        ])
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .into()
    }

    /// One session row: the whole row attaches, the floating trash asks
    /// to kill.
    ///
    /// `tab_idx` is captured HERE, while the row is built, not resolved
    /// when the message lands: the attach writes into a pane, and a tab
    /// switch between click and delivery would otherwise type the line
    /// into someone else's shell.
    fn tmux_session_row<'a>(
        &'a self,
        tab_idx: usize,
        pane_id: uuid::Uuid,
        idx: usize,
        session: &'a TmuxSession,
    ) -> Element<'a, Message> {
        // The session this pane is believed to be showing (issue #159):
        // its row is highlighted and inert, because the attach line is
        // typed INTO the pane, i.e. into this very session, and would
        // land in whatever program runs there.
        let current = self
            .tmux
            .get(&pane_id)
            .is_some_and(|e| e.attached_to.as_deref() == Some(session.name.as_str()));
        let attach = Message::Tmux(TmuxMessage::Attach(
            tab_idx,
            pane_id,
            session.name.clone(),
        ));
        let accent = OryxisColors::t().accent;
        let muted = OryxisColors::t().text_muted;

        // "3 windows" plus the attached marker, which is what tells the
        // user this session already has a client somewhere; the pane's
        // own session says so in its own words.
        let mut meta: Vec<Element<'a, Message>> = vec![
            text(t("tmux_windows").replace("{n}", &session.windows.to_string()))
                .size(10)
                .color(muted)
                .into(),
        ];
        if current {
            meta.push(Space::new().width(6).into());
            meta.push(text(t("tmux_attached_here")).size(10).color(accent).into());
        } else if session.is_attached() {
            meta.push(Space::new().width(6).into());
            meta.push(text(t("tmux_attached")).size(10).color(accent).into());
        }
        if let Some(group) = &session.group {
            meta.push(Space::new().width(6).into());
            meta.push(text(format!("({group})")).size(10).color(muted).into());
        }
        // Work still running inside the session (issue #159 follow-up):
        // the pane commands that are not a shell at its prompt, so a
        // detached session with a long job says so instead of looking
        // idle. First command plus a count when several panes are busy.
        if let Some(first) = session.running.first() {
            let display = if session.running.len() > 1 {
                format!("{first} +{}", session.running.len() - 1)
            } else {
                first.clone()
            };
            meta.push(Space::new().width(6).into());
            meta.push(
                text(t("tmux_running").replace("{cmd}", &display))
                    .size(10)
                    .color(OryxisColors::t().warning)
                    .into(),
            );
        }

        let row_body = button(
            column![
                text(session.name.clone())
                    .size(12)
                    .color(OryxisColors::t().text_primary),
                dir_row(meta).align_y(iced::Alignment::Center),
            ]
            .spacing(2)
            .width(Length::Fill),
        )
        .on_press_maybe((!current).then(|| attach.clone()))
        .padding(Padding { top: 6.0, right: 30.0, bottom: 6.0, left: 8.0 })
        .width(Length::Fill)
        .style(if current { current_row_style } else { row_btn_style });

        // Floating kill, revealed on hover so it reserves no inline
        // width (card-action convention).
        let mut stack = Stack::new().push(row_body);
        if self.hover.tmux_row == Some(idx) {
            stack = stack.push(
                container(crate::views::terminal::icon_tooltip(
                    button(
                        iced_fonts::lucide::trash()
                            .size(12)
                            .color(OryxisColors::t().error),
                    )
                    .on_press(Message::Tmux(TmuxMessage::AskKill(
                        pane_id,
                        session.name.clone(),
                    )))
                    .padding(Padding { top: 4.0, right: 6.0, bottom: 4.0, left: 6.0 })
                    .style(icon_btn_style)
                    .into(),
                    t("tmux_kill"),
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(if crate::i18n::is_rtl_layout() {
                    iced::alignment::Horizontal::Left
                } else {
                    iced::alignment::Horizontal::Right
                })
                .align_y(iced::alignment::Vertical::Center)
                .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 4.0 }),
            );
        }

        // The row joins the keyboard walk with Enter = attach and the
        // menu binding = kill, so the destructive action is reachable
        // without the mouse but never the default. The current row's
        // Enter is a no-op like its click (the handler guards too);
        // kill stays reachable.
        let navigable = self.sidebar_nav_slot(
            crate::keynav::SidebarRow::button(if current { Message::NoOp } else { attach })
                .with_menu(Message::Tmux(TmuxMessage::AskKill(
                    pane_id,
                    session.name.clone(),
                ))),
            TerminalSidebarTab::Tmux,
            6.0,
            stack.into(),
        );

        MouseArea::new(navigable)
            .on_enter(Message::Tmux(TmuxMessage::RowHovered(idx)))
            .on_exit(Message::Tmux(TmuxMessage::RowExit(idx)))
            .into()
    }

    /// The kill confirmation, in place of the row it replaces: a kill is
    /// unrecoverable, so it never fires from a single click.
    fn tmux_kill_confirm<'a>(
        &'a self,
        pane_id: uuid::Uuid,
        session: &'a TmuxSession,
    ) -> Element<'a, Message> {
        let confirm = Message::Tmux(TmuxMessage::ConfirmKill(pane_id));
        let cancel = Message::Tmux(TmuxMessage::CancelKill(pane_id));
        container(
            column![
                text(t("tmux_kill_confirm").replace("{name}", &session.name))
                    .size(11)
                    .color(OryxisColors::t().text_primary),
                dir_row(vec![
                    self.sidebar_nav_slot(
                        crate::keynav::SidebarRow::button(confirm.clone()),
                        TerminalSidebarTab::Tmux,
                        6.0,
                        crate::widgets::styled_button(
                            t("tmux_kill"),
                            confirm,
                            OryxisColors::t().error,
                        ),
                    ),
                    Space::new().width(6).into(),
                    self.sidebar_nav_slot(
                        crate::keynav::SidebarRow::button(cancel.clone()),
                        TerminalSidebarTab::Tmux,
                        6.0,
                        crate::widgets::styled_button(
                            t("cancel"),
                            cancel,
                            OryxisColors::t().text_secondary,
                        ),
                    ),
                ])
                .align_y(iced::Alignment::Center),
            ]
            .spacing(6),
        )
        .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().error,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
    }

    /// Name field + create. An empty name is valid: tmux numbers the
    /// session itself, exactly as `tmux new -d` does.
    fn tmux_new_session_row(&self, pane_id: uuid::Uuid) -> Element<'_, Message> {
        // The fork's `text_input` BORROWS its value, so this has to be
        // the field the state owns, not a local clone: a clone dies at
        // the end of this function and the element outlives it.
        let value = self
            .tmux
            .get(&pane_id)
            .map(|e| e.new_name.as_str())
            .unwrap_or("");
        let input_id = crate::tmux::new_name_input_id();
        // `Submitted`, not `Create`: the fork's `text_input` fires
        // `on_submit` on ANY Enter, focused or not, so an Enter typed
        // into the terminal reaches this binding too. The handler
        // resolves real focus before creating (issue #160: every
        // command run with the tab open minted a session on the host).
        let field = iced::widget::text_input(t("tmux_new_placeholder"), value)
            .id(input_id.clone())
            .on_input(move |v| Message::Tmux(TmuxMessage::NewNameChanged(pane_id, v)))
            .on_submit(Message::Tmux(TmuxMessage::Submitted(pane_id)))
            .size(12)
            .padding(Padding { top: 5.0, right: 8.0, bottom: 5.0, left: 8.0 });
        let create = Message::Tmux(TmuxMessage::Create(pane_id));
        dir_row(vec![
            self.sidebar_nav_slot(
                crate::keynav::SidebarRow::input(input_id),
                TerminalSidebarTab::Tmux,
                6.0,
                container(field).width(Length::Fill).into(),
            ),
            Space::new().width(6).into(),
            self.sidebar_nav_slot(
                crate::keynav::SidebarRow::button(create.clone()),
                TerminalSidebarTab::Tmux,
                6.0,
                crate::views::terminal::icon_tooltip(
                    button(
                        iced_fonts::lucide::plus()
                            .size(13)
                            .color(OryxisColors::t().accent),
                    )
                    .on_press(create)
                    .padding(Padding { top: 5.0, right: 7.0, bottom: 5.0, left: 7.0 })
                    .style(icon_btn_style)
                    .into(),
                    t("tmux_new_session"),
                ),
            ),
        ])
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .into()
    }
}

/// The row of the session this pane is showing (issue #159): a steady
/// selected tint, no hover feedback, because the row is inert. The
/// background is the "you are here" marker BabakAmini asked for.
fn current_row_style(_: &iced::Theme, _: BtnStatus) -> button::Style {
    button::Style {
        background: Some(Background::Color(OryxisColors::t().bg_selected)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    }
}

/// Session row background: transparent at rest, tinted on hover and
/// press, like every other clickable row.
fn row_btn_style(_: &iced::Theme, status: BtnStatus) -> button::Style {
    let bg = match status {
        BtnStatus::Hovered => OryxisColors::t().bg_hover,
        BtnStatus::Pressed => OryxisColors::t().bg_selected,
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    }
}

/// Square icon affordance (refresh, kill, create): the background
/// carries the hover / press feedback, the glyph keeps its own colour.
fn icon_btn_style(_: &iced::Theme, status: BtnStatus) -> button::Style {
    let bg = match status {
        BtnStatus::Hovered => OryxisColors::t().bg_hover,
        BtnStatus::Pressed => OryxisColors::t().bg_selected,
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    }
}

fn hint(label: &str) -> Element<'_, Message> {
    container(text(label.to_string()).size(11).color(OryxisColors::t().text_muted))
        .padding(Padding { top: 10.0, right: 4.0, bottom: 4.0, left: 4.0 })
        .width(Length::Fill)
        .into()
}

fn placeholder(label: &str) -> Element<'_, Message> {
    container(text(label.to_string()).size(12).color(OryxisColors::t().text_muted))
        .center_x(Length::Fill)
        .padding(Padding { top: 40.0, right: 12.0, bottom: 0.0, left: 12.0 })
        .width(Length::Fill)
        .into()
}
