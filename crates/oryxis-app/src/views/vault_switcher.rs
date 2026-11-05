//! The vault doors on screen: the switcher chip (toolbar) and badge (nav
//! rail) naming the vault the window works on, the menu they open, and
//! the vault dialog (create, rename, delete).

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, svg, pick_list, text, text_input, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, MultiVaultMessage, Oryxis};
use crate::dispatch_multi_vault::{VaultChipPlace, VAULT_DIALOG_NAME_INPUT};
use crate::i18n::t;
use crate::keynav::RowAction;
use crate::state::{CopySettingsChoice, VaultDialog, VaultDialogMode, VaultState};
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row, styled_button, styled_button_opt};

impl Oryxis {
    /// The name of the vault the window being drawn works on.
    fn shown_vault_name(&self) -> String {
        self.vault_name(self.vaults.cur().id)
    }

    /// The toolbar chip's width, for the sub-nav's overflow budget (the
    /// pills that fit beside it): its padding, glyphs and gaps, plus the
    /// vault's name at the chip's 12 px.
    pub(crate) fn vault_chip_width(&self) -> f32 {
        let name = self.shown_vault_name().chars().count() as f32;
        (9.0 + 13.0 + 6.0 + 6.0 + 12.0 + 9.0 + name * 7.0).min(260.0)
    }

    /// The toolbar chip: the window's vault, pressing it opens the
    /// switcher menu under it.
    pub(crate) fn vault_switcher_chip(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let chip = button(
            dir_row(vec![
                iced_fonts::lucide::lock().size(13).color(c.accent).boxed(),
                Space::new().width(6).boxed(),
                text(self.shown_vault_name()).size(12).color(c.text_primary).boxed(),
                Space::new().width(6).boxed(),
                iced_fonts::lucide::chevron_down().size(12).color(c.text_muted).boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::MultiVault(MultiVaultMessage::ShowVaultSwitcher(VaultChipPlace::Toolbar)))
        .height(Length::Fixed(28.0))
        .padding(Padding { top: 0.0, right: 9.0, bottom: 0.0, left: 9.0 })
        .style(chip_style(6.0));
        // No tooltip while its own menu is open: it would sit over the
        // menu's first row.
        let menu_open = self
            .cur_overlay()
            .is_some_and(|o| matches!(o.content, crate::state::OverlayContent::VaultSwitcher));
        // The ring half only: the chip is BUILT whether or not the
        // toolbar places it, so the toolbar records the item where it
        // puts the chip (recording here rang an invisible control first).
        let chip = self.keynav_toolbar_ring(crate::keynav::ToolbarItem::VaultSwitcher, chip.boxed());
        let chip: Element<'_, Message> = if menu_open {
            chip
        } else {
            crate::views::terminal::icon_tooltip(chip, t("vault_switcher_tooltip"))
        };
        crate::widgets::bounds_reporter(chip, self.vault_chip_bounds.clone())
    }

    /// The nav rail's badge: the same door, glyph only while the rail is
    /// collapsed.
    pub(crate) fn vault_switcher_badge(&self, expanded: bool) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let inner: Element<'_, Message> = if expanded {
            dir_row(vec![
                iced_fonts::lucide::lock().size(14).color(c.accent).boxed(),
                Space::new().width(8).boxed(),
                text(self.shown_vault_name()).size(13).color(c.text_primary).boxed(),
                Space::new().width(Length::Fill).boxed(),
                iced_fonts::lucide::chevron_down().size(12).color(c.text_muted).boxed(),
            ])
            .align_y(iced::Alignment::Center)
            .boxed()
        } else {
            container(iced_fonts::lucide::lock().size(16).color(c.accent))
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .boxed()
        };
        let pad = if expanded {
            Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 12.0 }
        } else {
            Padding::ZERO
        };
        let badge = button(container(inner).center_y(Length::Fixed(40.0)).width(Length::Fill))
            .on_press(Message::MultiVault(MultiVaultMessage::ShowVaultSwitcher(VaultChipPlace::Rail)))
            .width(Length::Fill)
            .padding(pad)
            .style(chip_style(8.0));
        let badge: Element<'_, Message> = if expanded {
            badge.boxed()
        } else {
            // Collapsed, the name is only in the tooltip.
            crate::views::terminal::icon_tooltip_owned(badge.boxed(), self.shown_vault_name())
        };
        container(crate::widgets::bounds_reporter(badge, self.vault_badge_bounds.clone()))
            .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
            .boxed()
    }

    /// The start picker (Settings > Vaults "Ask which vault to open at
    /// start"): every vault this device knows, the last one used first
    /// in the keyboard's eye (its row is the default, Enter opens it).
    /// In the default theme: no vault is chosen yet.
    pub(crate) fn view_start_picker(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        self.modal_nav_reset();
        let last = self.vaults.window_id();
        let mut rows = iced::widget::Column::<Element<'_, Message>>::new().spacing(8).width(Length::Fill);
        for entry in self.registered_vaults() {
            let id = entry.id;
            let available = self.vault_available(id);
            let mut title: Vec<Element<'_, Message>> = vec![
                iced_fonts::lucide::database()
                    .size(15)
                    .color(if available { c.accent } else { c.text_muted })
                    .boxed(),
                Space::new().width(10).boxed(),
                text(Self::vault_display_name(&entry))
                    .size(14)
                    .color(if available { c.text_primary } else { c.text_muted })
                    .boxed(),
            ];
            if id == last {
                title.push(Space::new().width(8).boxed());
                title.push(
                    container(text(t("vault_picker_last_used")).size(10).color(c.text_secondary))
                        .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
                        .style(|_| container::Style {
                            background: Some(Background::Color(OryxisColors::t().bg_selected)),
                            border: Border { radius: Radius::from(4.0), ..Default::default() },
                            ..Default::default()
                        })
                        .boxed(),
                );
            }
            let detail: Element<'_, Message> = if available {
                text(
                    oryxis_vault::device::installed()
                        .map(|d| d.resolve(&entry).display().to_string())
                        .unwrap_or_default(),
                )
                .size(11)
                .font(iced::Font::MONOSPACE)
                .color(c.text_muted)
                .boxed()
            } else {
                text(t("vault_picker_missing")).size(11).color(c.error).boxed()
            };
            let pick = Message::MultiVault(MultiVaultMessage::PickStartVault(id));
            let card = button(
                column![
                    dir_row(title).align_y(iced::Alignment::Center),
                    Space::new().height(4),
                    detail,
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .on_press_maybe(available.then(|| pick.clone()))
            .width(Length::Fill)
            .padding(14)
            .style(chip_style(10.0));
            let card = card.boxed();
            rows = rows.push(if !available {
                card
            } else if id == last {
                self.modal_nav_slot_default(RowAction::activate(pick), 10.0, false, card)
            } else {
                self.modal_nav_slot(RowAction::activate(pick), 10.0, false, card)
            });
        }
        let body = column![
            svg(self.logo_handle.clone()).width(56).height(56),
            Space::new().height(14),
            text(t("vault_picker_title")).size(24).color(c.text_primary),
            Space::new().height(6),
            text(t("vault_picker_body")).size(13).color(c.text_secondary),
            Space::new().height(22),
            rows,
        ]
        .width(Length::Fixed(440.0))
        .align_x(iced::Alignment::Center);
        let page = container(iced::widget::scrollable(container(body).padding(24).center_x(Length::Fill)))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                ..Default::default()
            });
        crate::views::vault::with_chrome(page.boxed(), self.cur_maximized())
    }

    /// The lock / open-failed screen's way out: the other vaults this
    /// device knows, each a link that moves the window to it.
    pub(crate) fn other_vault_links(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let here = self.vaults.cur().id;
        let mut links: Vec<Element<'_, Message>> = vec![
            text(t("vault_other_vaults")).size(12).color(c.text_muted).boxed(),
            Space::new().width(4).boxed(),
        ];
        for entry in self.registered_vaults().into_iter().filter(|e| e.id != here) {
            links.push(
                button(text(Self::vault_display_name(&entry)).size(12).color(c.accent))
                    .on_press(Message::MultiVault(MultiVaultMessage::SwitchToVault(entry.id)))
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(menu_row_style)
                    .boxed(),
            );
        }
        dir_row(links).align_y(iced::Alignment::Center).wrap().boxed()
    }

    /// The switcher menu: every vault this device knows (pick one to
    /// move this window to it, the trailing button opens it in a new
    /// window), then "New vault", "Manage vaults" (Settings > Vaults)
    /// and "Lock all vaults".
    pub(crate) fn build_menu_vault_switcher(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let here = self.vaults.cur().id;
        let mut items = iced::widget::Column::<Element<'_, Message>>::new();
        for entry in self.registered_vaults() {
            let id = entry.id;
            let locked = self
                .vaults
                .all()
                .find(|v| v.id == id)
                .is_some_and(|v| v.vault_ui.state == VaultState::Locked);
            // A vault whose file is gone is listed, inert, until it is back.
            let available = self.vault_available(id);
            let icon: Element<'_, Message> = if id == here {
                iced_fonts::lucide::check().size(14).color(c.accent).boxed()
            } else if !available {
                iced_fonts::lucide::circle_alert().size(14).color(c.text_muted).boxed()
            } else if locked {
                iced_fonts::lucide::lock().size(14).color(c.text_muted).boxed()
            } else {
                iced_fonts::lucide::database().size(14).color(c.text_secondary).boxed()
            };
            let switch = Message::MultiVault(MultiVaultMessage::SwitchToVault(id));
            let name_color = if available { c.text_primary } else { c.text_muted };
            let row = button(
                dir_row(vec![
                    icon,
                    Space::new().width(8).boxed(),
                    text(Self::vault_display_name(&entry)).size(12).color(name_color).boxed(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .on_press_maybe(available.then(|| switch.clone()))
            .width(Length::Fill)
            .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 })
            .style(menu_row_style);
            if !available {
                items = items.push(crate::views::terminal::icon_tooltip(row.boxed(), t("vault_status_missing")));
                continue;
            }
            let new_window = Message::MultiVault(MultiVaultMessage::OpenVaultInNewWindow(id));
            let window_button = button(iced_fonts::lucide::app_window().size(13).color(c.text_muted))
                .on_press(new_window.clone())
                .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 })
                .style(menu_row_style);
            items = items.push(
                dir_row(vec![
                    self.modal_nav_slot(RowAction::activate(switch), 4.0, false, row.boxed()),
                    self.modal_nav_slot(
                        RowAction::activate(new_window),
                        4.0,
                        false,
                        crate::views::terminal::icon_tooltip(window_button.boxed(), t("vault_open_new_window")),
                    ),
                ])
                .align_y(iced::Alignment::Center)
                .boxed(),
            );
        }
        items = items.push(crate::views::layout::menu_divider());
        items = items.push(self.menu_item(
            iced_fonts::lucide::plus(),
            t("vault_new"),
            Message::MultiVault(MultiVaultMessage::ShowNewVault),
            c.text_secondary,
        ));
        items = items.push(self.menu_item(
            iced_fonts::lucide::settings(),
            t("vault_manage"),
            Message::MultiVault(MultiVaultMessage::ShowVaultSettings),
            c.text_secondary,
        ));
        if self.any_vault_lockable() {
            items = items.push(self.menu_item(
                iced_fonts::lucide::lock(),
                t("vault_lock_all"),
                Message::MultiVault(MultiVaultMessage::LockAllVaults),
                c.text_secondary,
            ));
        }
        items.boxed()
    }

    /// Rows of the switcher menu, next to its builder (the divider counts
    /// as half a row).
    pub(crate) fn vault_switcher_menu_rows(&self) -> f32 {
        let vaults = self.registered_vaults().len() as f32;
        let lock = if self.any_vault_lockable() { 1.0 } else { 0.0 };
        vaults + 0.5 + 2.0 + lock
    }

    /// Whether "Lock all vaults" has anything to lock.
    pub(crate) fn any_vault_lockable(&self) -> bool {
        self.vaults
            .all()
            .any(|v| v.vault_ui.has_user_password && v.vault_ui.state == VaultState::Unlocked)
    }

    /// The vault dialog: create (a name and the vault whose settings it
    /// starts with), rename (the name), or delete (the vault's name typed
    /// back; the button stays inert until it matches).
    pub(crate) fn build_vault_dialog<'a>(&'a self, form: &'a VaultDialog) -> Element<'a, Message> {
        let c = OryxisColors::t();
        self.modal_nav_reset();
        let input_id = iced::widget::Id::new(VAULT_DIALOG_NAME_INPUT);
        let submit = Message::MultiVault(MultiVaultMessage::VaultDialogSubmit);
        let cancel = Message::MultiVault(MultiVaultMessage::CancelVaultDialog);
        let (title, intro, field, placeholder, confirm, confirm_color) = match form.mode {
            VaultDialogMode::Create => (
                t("vault_new_title").to_string(),
                t("vault_new_body").to_string(),
                t("vault_new_name"),
                t("vault_new_name_placeholder"),
                t("vault_create"),
                c.accent,
            ),
            VaultDialogMode::Rename(id) => (
                t("vault_rename_title").replacen("{name}", &self.vault_name(id), 1),
                String::new(),
                t("vault_new_name"),
                t("vault_new_name_placeholder"),
                t("vault_rename"),
                c.accent,
            ),
            VaultDialogMode::Delete(id) => (
                t("vault_delete_title").replacen("{name}", &self.vault_name(id), 1),
                t("vault_delete_body").to_string(),
                t("vault_delete_type_name"),
                "",
                t("vault_delete"),
                c.error,
            ),
        };
        // Delete arms only on the exact name: the typed name IS the
        // confirmation, so Enter in the field may submit too.
        let armed = match form.mode {
            VaultDialogMode::Delete(id) => form.name.trim() == self.vault_name(id),
            _ => true,
        };
        let mut body = iced::widget::Column::<Element<'a, Message>>::new()
            .width(Length::Fill)
            .align_x(dir_align_x())
            .push(text(title).size(16).color(c.text_primary).width(Length::Fill).align_x(dir_align_x()).boxed());
        if !intro.is_empty() {
            body = body.push(Space::new().height(4).boxed()).push(
                text(intro).size(12).color(c.text_secondary).width(Length::Fill).align_x(dir_align_x()).boxed(),
            );
        }
        body = body
            .push(Space::new().height(14).boxed())
            .push(text(field).size(12).color(c.text_secondary).boxed())
            .push(Space::new().height(4).boxed())
            .push(self.modal_nav_slot(
                RowAction::input(input_id.clone()),
                crate::widgets::INPUT_RADIUS,
                false,
                text_input(placeholder, &form.name)
                    .id(input_id)
                    .on_input(|v| Message::MultiVault(MultiVaultMessage::VaultDialogNameChanged(v)))
                    .on_submit(submit.clone())
                    .padding(10)
                    // 13px to match the app's inline form controls
                    // (password fields, option rows); without it the
                    // field fell back to iced's ~16px default and read
                    // larger than every other input.
                    .size(13)
                    .width(Length::Fill)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .boxed(),
            ));
        if form.mode == VaultDialogMode::Create {
            let mut choices = vec![CopySettingsChoice { vault: None, label: t("vault_copy_none").to_string() }];
            choices.extend(self.registered_vaults().into_iter().map(|e| CopySettingsChoice {
                vault: Some(e.id),
                label: Self::vault_display_name(&e),
            }));
            let selected = choices.iter().find(|c| c.vault == form.copy_from).cloned();
            let (prev, next) = (
                copy_from_step(&choices, form.copy_from, -1),
                copy_from_step(&choices, form.copy_from, 1),
            );
            body = body
                .push(Space::new().height(12).boxed())
                .push(text(t("vault_copy_from")).size(12).color(c.text_secondary).boxed())
                .push(Space::new().height(4).boxed())
                .push(self.modal_nav_slot(
                    RowAction::picker(Some(prev), Some(next)),
                    6.0,
                    false,
                    pick_list(selected, choices, |choice: &CopySettingsChoice| choice.to_string())
                        .on_select(|choice: CopySettingsChoice| {
                            Message::MultiVault(MultiVaultMessage::NewVaultCopyFromChanged(choice.vault))
                        })
                        // Match the name input's 13px and the app's
                        // shared surface/border style so the dropdown
                        // lines up with the rest of the form.
                        .text_size(13)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .width(Length::Fill)
                        .boxed(),
                ))
                .push(Space::new().height(6).boxed())
                .push(
                    text(t("vault_copy_from_hint"))
                        .size(11)
                        .color(c.text_muted)
                        .width(Length::Fill)
                        .align_x(dir_align_x())
                        .boxed(),
                );
        }
        if let Some(error) = &form.error {
            body = body.push(Space::new().height(10).boxed());
            body = body.push(
                text(error.as_str()).size(12).color(c.error).width(Length::Fill).align_x(dir_align_x()).boxed(),
            );
        }
        body = body.push(Space::new().height(16).boxed());
        body = body.push(dir_row(vec![
            self.modal_nav_slot_default(
                RowAction::activate(submit.clone()),
                6.0,
                true,
                styled_button_opt(confirm, armed.then_some(submit), confirm_color),
            ),
            Space::new().width(8).boxed(),
            self.modal_nav_slot(
                RowAction::activate(cancel.clone()),
                6.0,
                false,
                styled_button(t("cancel"), cancel, c.error),
            ),
        ]).boxed());
        container(body.padding(24))
            .width(Length::Fixed(400.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            })
            .boxed()
    }
}

/// The picker message one step from `current` in `choices` (wrapping),
/// for the keyboard's Left / Right on the "Start with settings from" row.
fn copy_from_step(choices: &[CopySettingsChoice], current: Option<uuid::Uuid>, step: isize) -> Message {
    let len = choices.len() as isize;
    let at = choices.iter().position(|c| c.vault == current).unwrap_or(0) as isize;
    let next = (at + step).rem_euclid(len.max(1)) as usize;
    Message::MultiVault(MultiVaultMessage::NewVaultCopyFromChanged(
        choices.get(next).and_then(|c| c.vault),
    ))
}

/// A chip / badge that reads as pressable: the surface, a hover tint, a
/// press tint.
fn chip_style(radius: f32) -> impl Fn(&iced::Theme, BtnStatus) -> button::Style {
    move |_, status| {
        let c = OryxisColors::t();
        let bg = match status {
            BtnStatus::Hovered => c.bg_hover,
            BtnStatus::Pressed => c.bg_selected,
            _ => c.bg_surface,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: c.text_primary,
            border: Border { radius: Radius::from(radius), color: c.border, width: 1.0 },
            ..Default::default()
        }
    }
}

/// A row of the switcher menu.
fn menu_row_style(_: &iced::Theme, status: BtnStatus) -> button::Style {
    let c = OryxisColors::t();
    let bg = match status {
        BtnStatus::Hovered => c.bg_hover,
        BtnStatus::Pressed => c.bg_selected,
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: c.text_primary,
        border: Border { radius: Radius::from(4.0), ..Default::default() },
        ..Default::default()
    }
}
