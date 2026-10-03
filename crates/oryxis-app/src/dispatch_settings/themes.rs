//! Settings dispatch helpers: themes. Split out of dispatch_settings/mod.rs.

use super::*;

/// Read a picked theme file with a sanity cap: real scheme files are a
/// few KB, and an accidental pick of a huge file would freeze the UI
/// while `text_editor::Content` ingests it. Binary content already
/// fails cleanly via the UTF-8 error.
fn read_theme_file(path: &std::path::Path) -> Result<String, String> {
    const MAX_THEME_FILE_BYTES: u64 = 1024 * 1024;
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_THEME_FILE_BYTES) {
        return Err(crate::i18n::t("theme_import_too_large").to_string());
    }
    std::fs::read_to_string(path)
        .map_err(|e| format!("{}: {e}", crate::i18n::t("theme_import_read_failed")))
}
impl Oryxis {
    /// Move a paste to the importer it actually belongs to instead of
    /// rejecting it. The two panels live in different settings sections,
    /// so the content and the typed name travel over, the user lands on
    /// the right section, and a toast says what happened; Apply there
    /// still runs every existing validation (same contract as the
    /// `oryxis://theme` deep link, which prefills and waits too).
    fn hand_off_theme_import(
        &mut self,
        to: crate::theme_import::ThemeKind,
        content: String,
        typed_name: &str,
    ) -> Task<Message> {
        use crate::theme_import::ThemeKind;
        let name = if typed_name.trim().is_empty() {
            crate::theme_import::suggest_name(&content).unwrap_or_default()
        } else {
            typed_name.trim().to_string()
        };
        let editor = iced::widget::text_editor::Content::with_text(&content);
        let (section, toast) = match to {
            ThemeKind::Terminal => {
                self.close_modal(crate::state::Modal::UiThemeImport);
                self.ui_theme_import_error = None;
                self.panels.theme_import = true;
                self.theme_ui.import_content = editor;
                self.theme_ui.import_name = name;
                self.theme_ui.import_error = None;
                (
                    crate::state::SettingsSection::Terminal,
                    crate::i18n::t("theme_import_moved_terminal"),
                )
            }
            ThemeKind::Ui => {
                self.close_modal(crate::state::Modal::ThemeImport);
                self.theme_ui.import_error = None;
                self.panels.ui_theme_import = true;
                self.ui_theme_import_content = editor;
                self.ui_theme_import_name = name;
                self.ui_theme_import_error = None;
                (
                    crate::state::SettingsSection::Interface,
                    crate::i18n::t("theme_import_moved_ui"),
                )
            }
        };
        // A sentence to read, not a one-word confirmation, and it has to
        // outlive the section switch it explains.
        self.set_toast_secs(toast.to_string(), 6);
        // Section only when Settings is already the view, which it is
        // whenever a human clicked Import: the full `OpenSettingsSection`
        // route goes through `ChangeView`, which lands focus in the
        // sidebar search and would pull it out of the panel we just
        // opened. The long way stays for any caller that is elsewhere.
        if self.cur_view() == View::Settings {
            Task::done(Message::Settings(SettingsMessage::ChangeSettingsSection(
                section,
            )))
        } else {
            Task::done(Message::Tabs(TabsMessage::OpenSettingsSection(section)))
        }
    }
    /// Validate + persist the in-progress custom theme. Returns
    /// `Some(error_message)` on failure (shown in the editor), `None` on
    /// success (after reloading the list + repainting).
    pub(crate) fn save_theme_editor(&mut self) -> Option<String> {
        use oryxis_core::models::custom_terminal_theme::CustomTerminalTheme;
        let form = self.theme_ui.editor.clone()?;
        let name = form.name.trim().to_string();
        if name.is_empty() {
            return Some(crate::i18n::t("theme_error_name_required").to_string());
        }
        let existing = form
            .editing_id
            .and_then(|id| self.custom_terminal_themes.iter().find(|t| t.id == id).cloned());
        // A NEW builtin-colliding name is refused; a custom theme that
        // already carries one (imported before the builtin existed) stays
        // editable under its own name. Name resolution favors custom, so
        // that theme, not the builtin, is what the user sees.
        let keeps_own_name = existing.as_ref().is_some_and(|e| e.name == name);
        if !keeps_own_name
            && oryxis_terminal::TerminalTheme::ALL.iter().any(|t| t.name() == name)
        {
            return Some(crate::i18n::t("theme_error_name_builtin").to_string());
        }
        if self
            .custom_terminal_themes
            .iter()
            .any(|t| t.name == name && Some(t.id) != form.editing_id)
        {
            return Some(crate::i18n::t("theme_error_name_taken").to_string());
        }
        let valid = |h: &str| crate::widgets::parse_hex_color(h).is_some();
        if !valid(&form.foreground)
            || !valid(&form.background)
            || !valid(&form.cursor)
            || form.ansi.iter().any(|h| !valid(h))
        {
            return Some(crate::i18n::t("theme_error_color_invalid").to_string());
        }

        let old_name = existing.as_ref().map(|e| e.name.clone());
        let created_at = existing
            .as_ref()
            .map(|e| e.created_at)
            .unwrap_or_else(chrono::Utc::now);
        let theme = CustomTerminalTheme {
            id: form.editing_id.unwrap_or_else(uuid::Uuid::new_v4),
            name: name.clone(),
            foreground: form.foreground,
            background: form.background,
            cursor: form.cursor,
            ansi: form.ansi,
            created_at,
            updated_at: chrono::Utc::now(),
        };

        {
            let Some(vault) = &self.vault else {
                return Some(crate::i18n::t("theme_error_save_failed").to_string());
            };
            if vault.save_custom_terminal_theme(&theme).is_err() {
                return Some(crate::i18n::t("theme_error_save_failed").to_string());
            }
        }

        // On rename, keep the global override pointed at the same theme.
        if let Some(old) = old_name
            && old != name
            && self.terminal_theme_override.as_deref() == Some(old.as_str())
        {
            self.terminal_theme_override = Some(name.clone());
            self.persist_setting("terminal_theme_override", &name);
        }

        self.custom_terminal_themes = self
            .vault
            .as_ref()
            .and_then(|v| v.list_custom_terminal_themes().ok())
            .unwrap_or_default();
        self.terminal_palette = self.resolve_global_terminal_palette();
        self.repaint_all_terminal_palettes();
        None
    }
}

impl Oryxis {
    /// Apply an app-theme name (built-in or a custom UI theme) to the global
    /// `OryxisColors`, tracking it in `active_app_theme_name`. Returns false
    /// if the name matches neither. Does not persist; callers that handle a
    /// user action persist + repaint.
    /// Caller sets `active_app_theme_name` on a `true` result (kept `&self`
    /// so it can be called while `self.vault` is borrowed during boot).
    pub(crate) fn apply_app_theme_name(&self, name: &str) -> bool {
        // Custom themes resolve FIRST: a homonymous custom can only
        // predate its builtin (the editor refuses new collisions), and
        // that user's setting meant the custom theme; a new builtin must
        // not silently take over their vault on upgrade.
        if let Some(colors) = self
            .custom_ui_themes
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.colors.clone())
        {
            crate::theme::set_active_custom_ui(crate::theme::theme_colors_from_hex(&colors));
            true
        } else if let Some(theme) = AppTheme::ALL.iter().find(|t| t.name() == name).copied() {
            AppTheme::set_active(theme); // also clears any active custom UI theme
            true
        } else {
            false
        }
    }

    /// Validate + persist the in-progress custom UI theme. Returns
    /// `Some(error)` on failure. On success, if the saved theme is the
    /// active one, re-applies it live.
    pub(crate) fn save_ui_theme_editor(&mut self) -> Option<String> {
        use oryxis_core::models::custom_ui_theme::CustomUiTheme;
        let form = self.ui_theme_editor.clone()?;
        let name = form.name.trim().to_string();
        if name.is_empty() {
            return Some(crate::i18n::t("theme_error_name_required").to_string());
        }
        let existing = form
            .editing_id
            .and_then(|id| self.custom_ui_themes.iter().find(|t| t.id == id).cloned());
        // Same rule as the terminal editor: only a NEW builtin-colliding
        // name is refused; a pre-existing homonymous custom theme stays
        // editable (and wins name resolution).
        let keeps_own_name = existing.as_ref().is_some_and(|e| e.name == name);
        if !keeps_own_name && AppTheme::ALL.iter().any(|t| t.name() == name) {
            return Some(crate::i18n::t("theme_error_name_builtin").to_string());
        }
        if self
            .custom_ui_themes
            .iter()
            .any(|t| t.name == name && Some(t.id) != form.editing_id)
        {
            return Some(crate::i18n::t("theme_error_name_taken").to_string());
        }
        if form
            .colors
            .iter()
            .any(|h| crate::widgets::parse_hex_color(h).is_none())
        {
            return Some(crate::i18n::t("theme_error_color_invalid").to_string());
        }

        let old_name = existing.as_ref().map(|e| e.name.clone());
        let created_at = existing
            .as_ref()
            .map(|e| e.created_at)
            .unwrap_or_else(chrono::Utc::now);
        let theme = CustomUiTheme {
            id: form.editing_id.unwrap_or_else(uuid::Uuid::new_v4),
            name: name.clone(),
            colors: form.colors,
            created_at,
            updated_at: chrono::Utc::now(),
        };

        {
            let Some(vault) = &self.vault else {
                return Some(crate::i18n::t("theme_error_save_failed").to_string());
            };
            if vault.save_custom_ui_theme(&theme).is_err() {
                return Some(crate::i18n::t("theme_error_save_failed").to_string());
            }
        }
        self.custom_ui_themes = self
            .vault
            .as_ref()
            .and_then(|v| v.list_custom_ui_themes().ok())
            .unwrap_or_default();

        // If editing the active theme (by old or new name), re-apply live.
        let was_active = old_name.as_deref() == Some(self.active_app_theme_name.as_str())
            || self.active_app_theme_name == name;
        if was_active {
            crate::theme::set_active_custom_ui(crate::theme::theme_colors_from_hex(
                &theme.colors,
            ));
            self.active_app_theme_name = name.clone();
            self.persist_setting("app_theme", &name);
            self.terminal_palette = self.resolve_global_terminal_palette();
            self.repaint_all_terminal_palettes();
        }
        None
    }
}

/// Build a `TerminalPalette` from a user-defined theme's hex strings.
/// Unparseable entries fall back to white/black so a malformed color never
/// crashes the render.
pub(crate) fn custom_theme_palette(
    t: &oryxis_core::models::custom_terminal_theme::CustomTerminalTheme,
) -> oryxis_terminal::TerminalPalette {
    let c = |hex: &str, fallback: iced::Color| {
        crate::widgets::parse_hex_color(hex).unwrap_or(fallback)
    };
    oryxis_terminal::TerminalPalette {
        foreground: c(&t.foreground, iced::Color::WHITE),
        background: c(&t.background, iced::Color::BLACK),
        cursor: c(&t.cursor, iced::Color::WHITE),
        ansi: std::array::from_fn(|i| c(&t.ansi[i], iced::Color::WHITE)),
    }
}

/// Map the active app theme to its companion terminal palette. Used
/// as the bottom-of-the-stack fallback in
/// `resolve_global_terminal_theme` when neither a global override nor a
/// per-host override is set. Every app theme has a matching palette
/// of the same name.
fn app_theme_to_terminal(theme: AppTheme) -> oryxis_terminal::TerminalTheme {
    match theme {
        AppTheme::OryxisDark => oryxis_terminal::TerminalTheme::OryxisDark,
        AppTheme::OryxisLight => oryxis_terminal::TerminalTheme::OryxisLight,
        AppTheme::Termius => oryxis_terminal::TerminalTheme::Termius,
        AppTheme::Darcula => oryxis_terminal::TerminalTheme::Darcula,
        AppTheme::IslandsDark => oryxis_terminal::TerminalTheme::IslandsDark,
        AppTheme::Dracula => oryxis_terminal::TerminalTheme::Dracula,
        AppTheme::Monokai => oryxis_terminal::TerminalTheme::Monokai,
        AppTheme::HackerGreen => oryxis_terminal::TerminalTheme::HackerGreen,
        AppTheme::OneDark => oryxis_terminal::TerminalTheme::OneDark,
        AppTheme::Nord => oryxis_terminal::TerminalTheme::Nord,
        AppTheme::NordLight => oryxis_terminal::TerminalTheme::NordLight,
        AppTheme::SolarizedDark => oryxis_terminal::TerminalTheme::SolarizedDark,
        AppTheme::SolarizedLight => oryxis_terminal::TerminalTheme::SolarizedLight,
        AppTheme::PaperLight => oryxis_terminal::TerminalTheme::PaperLight,
    }
}

impl Oryxis {
    /// Effective terminal palette for callers that don't have a
    /// specific connection in mind: settings preview, local-shell tabs,
    /// new-tab spawn defaults. Order: explicit user override → app
    /// theme mapping.
    /// Resolve a theme NAME (built-in or user-defined) to its palette.
    /// `None` when the name matches neither (e.g. a custom theme the user
    /// deleted), so callers fall through to their default.
    pub(crate) fn terminal_palette_for_name(
        &self,
        name: &str,
    ) -> Option<oryxis_terminal::TerminalPalette> {
        // Custom first, same rationale as `apply_app_theme_name`: a
        // pre-builtin homonymous custom keeps meaning the user's palette
        // after an upgrade ships a builtin with the same name.
        if let Some(t) = self.custom_terminal_themes.iter().find(|t| t.name == name) {
            return Some(custom_theme_palette(t));
        }
        oryxis_terminal::TerminalTheme::ALL
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.palette())
    }

    /// Effective global terminal palette: explicit user override (built-in
    /// or custom) → app theme mapping.
    pub(crate) fn resolve_global_terminal_palette(
        &self,
    ) -> oryxis_terminal::TerminalPalette {
        if let Some(name) = &self.terminal_theme_override
            && let Some(palette) = self.terminal_palette_for_name(name)
        {
            return palette;
        }
        app_theme_to_terminal(AppTheme::active()).palette()
    }

    /// Display name of the effective global terminal theme (for the
    /// "inherit (Global)" label). Keeps a stale override name from showing
    /// once the custom theme behind it is deleted.
    pub(crate) fn resolve_global_terminal_theme_name(&self) -> String {
        if let Some(name) = &self.terminal_theme_override
            && self.terminal_palette_for_name(name).is_some()
        {
            return name.clone();
        }
        app_theme_to_terminal(AppTheme::active()).name().to_string()
    }

    /// Effective terminal palette for a known `Connection`. Per-host
    /// override wins, then the global override, then the app theme.
    pub(crate) fn resolve_terminal_palette_for_connection(
        &self,
        conn: &oryxis_core::models::Connection,
    ) -> oryxis_terminal::TerminalPalette {
        if let Some(name) = &conn.terminal_theme
            && let Some(palette) = self.terminal_palette_for_name(name)
        {
            return palette;
        }
        // The host names none: its group chain may (D4). Between the
        // host's own choice and the global setting, not instead of
        // either.
        if let Some(name) = self
            .vault
            .as_ref()
            .and_then(|v| v.resolve_effective(conn, &self.groups, &self.identities).ok())
            .and_then(|e| e.terminal_theme.map(|(name, _)| name))
            && let Some(palette) = self.terminal_palette_for_name(&name)
        {
            return palette;
        }
        self.resolve_global_terminal_palette()
    }

    /// Same resolution but starting from a tab label. Used by repaint
    /// loops where we don't already hold a `Connection` reference.
    /// Falls through to the global theme for tabs without a matching
    /// connection (local shells, WSL, PowerShell, …).
    fn resolve_terminal_palette_for_label(
        &self,
        label: &str,
    ) -> oryxis_terminal::TerminalPalette {
        let base = label.trim_end_matches(" (disconnected)");
        if let Some(conn) = self.connections.iter().find(|c| c.label == base) {
            return self.resolve_terminal_palette_for_connection(conn);
        }
        self.resolve_global_terminal_palette()
    }

    /// Re-paint every open tab's palette. Use after a global theme
    /// change. Tabs whose connection has its own override pick that
    /// override up automatically through `resolve_terminal_palette_for_label`.
    pub(crate) fn repaint_all_terminal_palettes(&self) {
        for tab in &self.tabs {
            let palette = self.resolve_terminal_palette_for_label(&tab.label);
            for pane in tab.pane_grid.panes.values() {
                if let Ok(mut state) = pane.terminal.lock() {
                    state.set_palette(palette.clone());
                }
            }
        }
    }

    /// Apply the session-only local terminal theme to every open
    /// local/ephemeral pane (panes without a saved host). Host panes keep
    /// their own resolution. `None` falls back to the global palette.
    pub(crate) fn apply_local_terminal_palette(&self) {
        let palette = match &self.local_terminal_theme {
            Some(name) => self
                .terminal_palette_for_name(name)
                .unwrap_or_else(|| self.resolve_global_terminal_palette()),
            None => self.resolve_global_terminal_palette(),
        };
        for tab in &self.tabs {
            for pane in tab.pane_grid.panes.values() {
                if matches!(pane.origin, crate::state::PaneOrigin::Host(_)) {
                    continue;
                }
                if let Ok(mut state) = pane.terminal.lock() {
                    state.set_palette(palette.clone());
                }
            }
        }
    }

    /// Re-paint only the tabs attached to a single host's label.
    /// Called when the per-host override changes.
    pub(crate) fn repaint_terminal_palettes_for_label(&self, label: &str) {
        let palette = self.resolve_terminal_palette_for_label(label);
        let base = label.trim_end_matches(" (disconnected)");
        for tab in &self.tabs {
            let tab_base = tab.label.trim_end_matches(" (disconnected)");
            if tab_base != base {
                continue;
            }
            for pane in tab.pane_grid.panes.values() {
                if let Ok(mut state) = pane.terminal.lock() {
                    state.set_palette(palette.clone());
                }
            }
        }
    }
}

impl Oryxis {
    /// Theme-family arms: terminal / app theme pickers, the custom
    /// terminal + UI theme editors, theme import and the theme cards.
    pub(super) fn handle_settings_themes(
        &mut self,
        message: SettingsMessage,
    ) -> Result<Task<Message>, SettingsMessage> {
        match message {
            SettingsMessage::TerminalThemeChanged(name) => {
                // Empty string == "follow app theme". Anything else is
                // matched against the known theme names; an unknown
                // string is ignored so a typo'd setting can't lock
                // the user out of the picker.
                if name.is_empty() {
                    self.terminal_theme_override = None;
                    self.persist_setting("terminal_theme_override", "");
                } else if self.terminal_palette_for_name(&name).is_some() {
                    // Built-in or custom theme name.
                    self.terminal_theme_override = Some(name.clone());
                    self.persist_setting("terminal_theme_override", &name);
                } else {
                    return Ok(Task::none());
                }
                self.terminal_palette = self.resolve_global_terminal_palette();
                self.repaint_all_terminal_palettes();
            }
            SettingsMessage::LocalConfigThemeChanged(name) => {
                // Session-only override for local/ephemeral panes. Empty =
                // follow the global terminal theme. Unknown names ignored.
                if name.is_empty() {
                    self.local_terminal_theme = None;
                } else if self.terminal_palette_for_name(&name).is_some() {
                    self.local_terminal_theme = Some(name);
                } else {
                    return Ok(Task::none());
                }
                self.apply_local_terminal_palette();
            }
            SettingsMessage::LocalConfigSaveGlobal => {
                // Promote the session override to the persisted global
                // default, then drop it (the panes now follow global).
                if let Some(name) = self.local_terminal_theme.take() {
                    self.terminal_theme_override = Some(name.clone());
                    self.persist_setting("terminal_theme_override", &name);
                    self.terminal_palette = self.resolve_global_terminal_palette();
                    self.repaint_all_terminal_palettes();
                }
            }
            SettingsMessage::ThemeEditorOpenPicker(slot) => {
                self.theme_ui.color_popover = Some((slot, self.cur_mouse()));
            }
            SettingsMessage::ThemeEditorClosePicker => {
                self.theme_ui.color_popover = None;
            }
            SettingsMessage::ThemeCardHovered(idx) => {
                self.hover.theme_card = Some(idx);
            }
            SettingsMessage::ThemeCardUnhovered(idx) => {
                self.hover.leave_theme_card(idx);
            }
            SettingsMessage::ThemeEditorNew => {
                // Seed from the active terminal palette so the user starts
                // from the currently-selected theme.
                let p = self.terminal_palette.clone();
                let hex = |c: iced::Color| {
                    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
                    format!("#{:02x}{:02x}{:02x}", q(c.r), q(c.g), q(c.b))
                };
                self.theme_ui.editor = Some(crate::state::ThemeEditorForm {
                    editing_id: None,
                    name: String::new(),
                    foreground: hex(p.foreground),
                    background: hex(p.background),
                    cursor: hex(p.cursor),
                    ansi: std::array::from_fn(|i| hex(p.ansi[i])),
                    error: None,
                });
            }
            SettingsMessage::ThemeImportOpen => {
                self.panels.theme_import = true;
                self.theme_ui.import_content = iced::widget::text_editor::Content::new();
                self.theme_ui.import_name.clear();
                self.theme_ui.import_error = None;
            }
            SettingsMessage::ThemeImportClose => {
                self.panels.theme_import = false;
            }
            SettingsMessage::ThemeImportContentAction(action) => {
                self.theme_ui.import_content.perform(action);
                self.theme_ui.import_error = None;
            }
            SettingsMessage::ThemeImportNameChanged(v) => {
                self.theme_ui.import_name = v;
            }
            SettingsMessage::ThemeImportApply => {
                let content = self.theme_ui.import_content.text();
                let name = if self.theme_ui.import_name.trim().is_empty() {
                    crate::i18n::t("theme_imported_default").to_string()
                } else {
                    self.theme_ui.import_name.trim().to_string()
                };
                match crate::theme_import::parse_theme(&content, &name) {
                    Ok(theme) => {
                        // Open the parsed colors in the editor (as a new
                        // theme) so the user can review / rename before save.
                        let mut form = crate::state::ThemeEditorForm::from_theme(&theme);
                        form.editing_id = None;
                        self.theme_ui.editor = Some(form);
                        self.panels.theme_import = false;
                    }
                    // A UI theme pasted here is not a mistake to report,
                    // it is a paste in the wrong panel: carry it over.
                    Err(crate::theme_import::ImportError::WrongImporter(kind)) => {
                        let typed = self.theme_ui.import_name.clone();
                        return Ok(self.hand_off_theme_import(kind, content, &typed));
                    }
                    Err(e) => self.theme_ui.import_error = Some(e.localized()),
                }
            }
            // -- Custom UI (chrome) themes --
            SettingsMessage::UiThemeEditorNew => {
                // Seed from the currently active chrome colors so the user
                // starts from a working theme.
                let seed = crate::theme::theme_colors_to_hex(crate::theme::OryxisColors::t());
                self.ui_theme_editor =
                    Some(crate::state::UiThemeEditorForm::new_from_colors(seed));
            }
            SettingsMessage::UiThemeEditorEdit(idx) => {
                if let Some(theme) = self.custom_ui_themes.get(idx) {
                    self.ui_theme_editor =
                        Some(crate::state::UiThemeEditorForm::from_theme(theme));
                }
            }
            SettingsMessage::UiThemeEditorClose => {
                self.ui_theme_editor = None;
                self.ui_color_popover = None;
            }
            SettingsMessage::UiThemeEditorNameChanged(name) => {
                if let Some(form) = &mut self.ui_theme_editor {
                    form.name = name;
                    form.error = None;
                }
            }
            SettingsMessage::UiThemeColorChanged(idx, value) => {
                if let Some(form) = &mut self.ui_theme_editor
                    && idx < 21
                {
                    let cleaned: String = value
                        .chars()
                        .filter(|c| *c == '#' || c.is_ascii_hexdigit())
                        .take(7)
                        .collect();
                    form.colors[idx] = cleaned;
                }
            }
            SettingsMessage::UiThemeEditorOpenPicker(idx) => {
                self.ui_color_popover = Some((idx, self.cur_mouse()));
            }
            SettingsMessage::UiThemeEditorClosePicker => {
                self.ui_color_popover = None;
            }
            SettingsMessage::UiThemeEditorSave => {
                if let Some(err) = self.save_ui_theme_editor() {
                    if let Some(form) = &mut self.ui_theme_editor {
                        form.error = Some(err);
                    }
                } else {
                    self.ui_theme_editor = None;
                    self.ui_color_popover = None;
                }
            }
            SettingsMessage::UiThemeDelete(idx) => {
                if let Some(theme) = self.custom_ui_themes.get(idx)
                    && let Some(vault) = &self.vault
                {
                    let was_active = self.active_app_theme_name == theme.name;
                    let _ = vault.delete_custom_ui_theme(&theme.id);
                    self.custom_ui_themes =
                        vault.list_custom_ui_themes().unwrap_or_default();
                    if was_active {
                        // The active theme is gone; fall back to the default.
                        crate::theme::AppTheme::set_active(
                            crate::theme::AppTheme::OryxisDark,
                        );
                        self.active_app_theme_name = "Oryxis Dark".to_string();
                        self.persist_setting("app_theme", "Oryxis Dark");
                        self.terminal_palette = self.resolve_global_terminal_palette();
                        self.repaint_all_terminal_palettes();
                    }
                }
            }
            SettingsMessage::UiThemeCardHovered(idx) => {
                self.hover.ui_theme_card = Some(idx);
            }
            SettingsMessage::UiThemeCardUnhovered(idx) => {
                self.hover.leave_ui_theme_card(idx);
            }
            SettingsMessage::ThemeEditorEdit(idx) => {
                if let Some(theme) = self.custom_terminal_themes.get(idx) {
                    self.theme_ui.editor =
                        Some(crate::state::ThemeEditorForm::from_theme(theme));
                }
            }
            SettingsMessage::ThemeEditorClose => {
                self.close_modal(crate::state::Modal::ThemeEditor);
            }
            SettingsMessage::ThemeEditorNameChanged(name) => {
                if let Some(form) = &mut self.theme_ui.editor {
                    form.name = name;
                    form.error = None;
                }
            }
            SettingsMessage::ThemeEditorColorChanged(slot, value) => {
                if let Some(form) = &mut self.theme_ui.editor {
                    // Keep only hex-ish characters so the live preview stays
                    // sane while typing; full validation happens on save.
                    let cleaned: String = value
                        .chars()
                        .filter(|c| *c == '#' || c.is_ascii_hexdigit())
                        .take(7)
                        .collect();
                    form.set_slot(slot, cleaned);
                }
            }
            SettingsMessage::ThemeEditorSave => {
                if let Some(err) = self.save_theme_editor() {
                    if let Some(form) = &mut self.theme_ui.editor {
                        form.error = Some(err);
                    }
                } else {
                    self.close_modal(crate::state::Modal::ThemeEditor);
                }
            }
            SettingsMessage::ThemeDeleteRequested(idx) => {
                if let Some(theme) = self.custom_terminal_themes.get(idx) {
                    let name = theme.name.clone();
                    self.confirm_remove(
                        name,
                        Message::Settings(SettingsMessage::ThemeDelete(idx)),
                    );
                }
            }
            SettingsMessage::UiThemeDeleteRequested(idx) => {
                if let Some(theme) = self.custom_ui_themes.get(idx) {
                    let name = theme.name.clone();
                    self.confirm_remove(
                        name,
                        Message::Settings(SettingsMessage::UiThemeDelete(idx)),
                    );
                }
            }
            SettingsMessage::ThemeDelete(idx) => {
                if let Some(theme) = self.custom_terminal_themes.get(idx)
                    && let Some(vault) = &self.vault
                {
                    let _ = vault.delete_custom_terminal_theme(&theme.id);
                    self.custom_terminal_themes =
                        vault.list_custom_terminal_themes().unwrap_or_default();
                    // A host / global override pointing at the deleted theme
                    // now resolves to its fallback; repaint reflects that.
                    self.terminal_palette = self.resolve_global_terminal_palette();
                    self.repaint_all_terminal_palettes();
                }
            }
            SettingsMessage::AppThemeChanged(name) => {
                if self.apply_app_theme_name(&name) {
                    self.active_app_theme_name = name.clone();
                    self.persist_setting("app_theme", &name);
                    // Refresh the global derived palette and re-paint
                    // every tab. Tabs whose connection has its own
                    // terminal_theme override pick that up via
                    // `resolve_terminal_theme_for_label`, so the user's
                    // per-host pick survives an app theme switch.
                    self.terminal_palette = self.resolve_global_terminal_palette();
                    self.repaint_all_terminal_palettes();
                }
            }
            SettingsMessage::ThemeBuiltinCardHovered(idx) => {
                self.hover.builtin_theme_card = Some(idx);
            }
            SettingsMessage::ThemeBuiltinCardUnhovered(idx) => {
                self.hover.leave_builtin_theme_card(idx);
            }
            SettingsMessage::ThemeClone(idx) => {
                if let Some(theme) = self.custom_terminal_themes.get(idx) {
                    let mut form = crate::state::ThemeEditorForm::from_theme(theme);
                    form.editing_id = None;
                    form.name = self.unique_terminal_theme_name(&theme.name);
                    self.theme_ui.editor = Some(form);
                }
            }
            SettingsMessage::ThemeCloneBuiltin(idx) => {
                // Editable copy of a built-in preset (the issue-#82 "start
                // from Dracula" flow), seeded from its palette.
                if let Some(theme) = oryxis_terminal::TerminalTheme::ALL.get(idx) {
                    let p = theme.palette();
                    let hex = crate::theme::color_to_hex;
                    self.theme_ui.editor = Some(crate::state::ThemeEditorForm {
                        editing_id: None,
                        name: self.unique_terminal_theme_name(theme.name()),
                        foreground: hex(p.foreground),
                        background: hex(p.background),
                        cursor: hex(p.cursor),
                        ansi: std::array::from_fn(|i| hex(p.ansi[i])),
                        error: None,
                    });
                }
            }
            SettingsMessage::ThemeExport(idx) => {
                if let Some(theme) = self.custom_terminal_themes.get(idx) {
                    let json = crate::theme_export::terminal_theme_to_json(theme);
                    let file_name = format!(
                        "{}.json",
                        crate::theme_export::sanitize_theme_filename(&theme.name)
                    );
                    return Ok(save_theme_file_task(json, file_name));
                }
            }
            SettingsMessage::ThemeExportBuiltin(idx) => {
                // Presets export too: the file doubles as a format template
                // for anyone hand-building or sharing a scheme.
                if let Some(theme) = oryxis_terminal::TerminalTheme::ALL.get(idx) {
                    let p = theme.palette();
                    let hex = crate::theme::color_to_hex;
                    let mut t = oryxis_core::models::custom_terminal_theme::CustomTerminalTheme::new_default(
                        theme.name().to_string(),
                    );
                    t.foreground = hex(p.foreground);
                    t.background = hex(p.background);
                    t.cursor = hex(p.cursor);
                    t.ansi = std::array::from_fn(|i| hex(p.ansi[i]));
                    let json = crate::theme_export::terminal_theme_to_json(&t);
                    let file_name = format!(
                        "{}.json",
                        crate::theme_export::sanitize_theme_filename(theme.name())
                    );
                    return Ok(save_theme_file_task(json, file_name));
                }
            }
            SettingsMessage::ThemeExportFinished(result) => match result {
                Ok(()) => {
                    return Ok(self.show_toast_secs(
                        crate::i18n::t("theme_exported").to_string(),
                        4,
                    ));
                }
                Err(e) if e == "cancelled" => {}
                Err(e) => {
                    return Ok(self.show_toast_secs(
                        format!("{}: {e}", crate::i18n::t("theme_export_failed")),
                        6,
                    ));
                }
            },
            SettingsMessage::ThemeGalleryFilterChanged(v) => {
                self.theme_ui.gallery_filter = v;
            }
            SettingsMessage::ThemeGalleryToneChanged(tone) => {
                self.theme_ui.gallery_tone = tone;
            }
            SettingsMessage::UiThemeGalleryFilterChanged(v) => {
                self.theme_ui.ui_gallery_filter = v;
            }
            SettingsMessage::UiThemeGalleryToneChanged(tone) => {
                self.theme_ui.ui_gallery_tone = tone;
            }
            SettingsMessage::ThemeImportBrowse => {
                // Feed a scheme file into the paste modal; the existing
                // Apply path parses it (h3 roadmap: file-picker import).
                return Ok(Task::perform(
                    tokio::task::spawn_blocking(move || {
                        let file = rfd::FileDialog::new()
                            .set_title(crate::i18n::t("theme_import_title"))
                            .add_filter(
                                "Theme files",
                                &["json", "itermcolors", "yaml", "yml", "txt"],
                            )
                            .add_filter("All files", &["*"])
                            .pick_file();
                        match file {
                            Some(path) => read_theme_file(&path),
                            None => Err("cancelled".to_string()),
                        }
                    }),
                    |result| {
                        let r = match result {
                            Ok(r) => r,
                            Err(e) => Err(format!(
                                "{}: {e}",
                                crate::i18n::t("theme_import_read_failed")
                            )),
                        };
                        Message::Settings(SettingsMessage::ThemeImportFileLoaded(r))
                    },
                ));
            }
            SettingsMessage::ThemeImportFileLoaded(result) => match result {
                Ok(content) => {
                    if self.theme_ui.import_name.trim().is_empty()
                        && let Some(name) = crate::theme_import::suggest_name(&content)
                    {
                        self.theme_ui.import_name = name;
                    }
                    self.theme_ui.import_content =
                        iced::widget::text_editor::Content::with_text(&content);
                    self.theme_ui.import_error = None;
                }
                Err(e) if e == "cancelled" => {}
                Err(e) => self.theme_ui.import_error = Some(e),
            },
            SettingsMessage::UiThemeBuiltinCardHovered(idx) => {
                self.hover.builtin_ui_theme_card = Some(idx);
            }
            SettingsMessage::UiThemeBuiltinCardUnhovered(idx) => {
                self.hover.leave_builtin_ui_theme_card(idx);
            }
            SettingsMessage::UiThemeClone(idx) => {
                if let Some(theme) = self.custom_ui_themes.get(idx) {
                    self.ui_theme_editor = Some(crate::state::UiThemeEditorForm {
                        editing_id: None,
                        name: self.unique_ui_theme_name(&theme.name),
                        colors: theme.colors.clone(),
                        error: None,
                    });
                }
            }
            SettingsMessage::UiThemeCloneBuiltin(idx) => {
                if let Some(theme) = AppTheme::ALL.get(idx) {
                    self.ui_theme_editor = Some(crate::state::UiThemeEditorForm {
                        editing_id: None,
                        name: self.unique_ui_theme_name(theme.name()),
                        colors: crate::theme::theme_colors_to_hex(theme.colors_ref()),
                        error: None,
                    });
                }
            }
            SettingsMessage::UiThemeExport(idx) => {
                if let Some(theme) = self.custom_ui_themes.get(idx) {
                    let json = crate::theme_export::ui_theme_to_json(theme);
                    let file_name = format!(
                        "{}.json",
                        crate::theme_export::sanitize_theme_filename(&theme.name)
                    );
                    return Ok(save_theme_file_task(json, file_name));
                }
            }
            SettingsMessage::UiThemeExportBuiltin(idx) => {
                if let Some(theme) = AppTheme::ALL.get(idx) {
                    let t = oryxis_core::models::custom_ui_theme::CustomUiTheme::new(
                        theme.name().to_string(),
                        crate::theme::theme_colors_to_hex(theme.colors_ref()),
                    );
                    let json = crate::theme_export::ui_theme_to_json(&t);
                    let file_name = format!(
                        "{}.json",
                        crate::theme_export::sanitize_theme_filename(theme.name())
                    );
                    return Ok(save_theme_file_task(json, file_name));
                }
            }
            SettingsMessage::UiThemeImportOpen => {
                self.panels.ui_theme_import = true;
                self.ui_theme_import_content =
                    iced::widget::text_editor::Content::new();
                self.ui_theme_import_name.clear();
                self.ui_theme_import_error = None;
            }
            SettingsMessage::UiThemeImportClose => {
                self.close_modal(crate::state::Modal::UiThemeImport);
            }
            SettingsMessage::UiThemeImportContentAction(action) => {
                self.ui_theme_import_content.perform(action);
                self.ui_theme_import_error = None;
            }
            SettingsMessage::UiThemeImportNameChanged(v) => {
                self.ui_theme_import_name = v;
            }
            SettingsMessage::UiThemeImportApply => {
                let content = self.ui_theme_import_content.text();
                match crate::theme_import::parse_ui_theme(
                    &content,
                    crate::i18n::t("theme_imported_default"),
                ) {
                    Ok(theme) => {
                        // A typed name overrides the file's own; dedupe up
                        // front so Save doesn't trip on a collision.
                        let typed = self.ui_theme_import_name.trim();
                        let base = if typed.is_empty() { theme.name.as_str() } else { typed };
                        let name = if self.ui_theme_name_taken(base) {
                            self.unique_ui_theme_name(base)
                        } else {
                            base.to_string()
                        };
                        self.ui_theme_editor = Some(crate::state::UiThemeEditorForm {
                            editing_id: None,
                            name,
                            colors: theme.colors,
                            error: None,
                        });
                        self.panels.ui_theme_import = false;
                    }
                    // Same courtesy in the other direction: a terminal
                    // scheme travels to Settings > Terminal instead of
                    // being turned away here.
                    Err(crate::theme_import::ImportError::WrongImporter(kind)) => {
                        let typed = self.ui_theme_import_name.clone();
                        return Ok(self.hand_off_theme_import(kind, content, &typed));
                    }
                    Err(e) => self.ui_theme_import_error = Some(e.localized()),
                }
            }
            SettingsMessage::UiThemeImportBrowse => {
                return Ok(Task::perform(
                    tokio::task::spawn_blocking(move || {
                        let file = rfd::FileDialog::new()
                            .set_title(crate::i18n::t("theme_import_title"))
                            .add_filter("Oryxis UI theme", &["json"])
                            .add_filter("All files", &["*"])
                            .pick_file();
                        match file {
                            Some(path) => read_theme_file(&path),
                            None => Err("cancelled".to_string()),
                        }
                    }),
                    |result| {
                        let r = match result {
                            Ok(r) => r,
                            Err(e) => Err(format!(
                                "{}: {e}",
                                crate::i18n::t("theme_import_read_failed")
                            )),
                        };
                        Message::Settings(SettingsMessage::UiThemeImportFileLoaded(r))
                    },
                ));
            }
            SettingsMessage::UiThemeImportFileLoaded(result) => match result {
                Ok(content) => {
                    // Fill the paste modal; Apply parses it like any pasted
                    // content (mirrors the terminal import flow).
                    if self.ui_theme_import_name.trim().is_empty()
                        && let Some(name) = crate::theme_import::suggest_name(&content)
                    {
                        self.ui_theme_import_name = name;
                    }
                    self.ui_theme_import_content =
                        iced::widget::text_editor::Content::with_text(&content);
                    self.ui_theme_import_error = None;
                }
                Err(e) if e == "cancelled" => {}
                Err(e) => self.ui_theme_import_error = Some(e),
            },
            m => return Err(m),
        }
        Ok(Task::none())
    }

    /// Seed name for a cloned / imported terminal theme, unique across
    /// built-ins and existing custom themes.
    fn unique_terminal_theme_name(&self, base: &str) -> String {
        crate::theme_export::unique_copy_name(
            base,
            crate::i18n::t("theme_copy_suffix"),
            |n| {
                oryxis_terminal::TerminalTheme::ALL.iter().any(|t| t.name() == n)
                    || self.custom_terminal_themes.iter().any(|t| t.name == n)
            },
        )
    }

    /// True when a UI theme name collides with a built-in or custom theme.
    fn ui_theme_name_taken(&self, name: &str) -> bool {
        AppTheme::ALL.iter().any(|t| t.name() == name)
            || self.custom_ui_themes.iter().any(|t| t.name == name)
    }

    /// Seed name for a cloned / imported UI theme, unique across built-ins
    /// and existing custom themes.
    fn unique_ui_theme_name(&self, base: &str) -> String {
        crate::theme_export::unique_copy_name(
            base,
            crate::i18n::t("theme_copy_suffix"),
            |n| self.ui_theme_name_taken(n),
        )
    }
}

/// Save-dialog task shared by the terminal and UI theme exports: pick a
/// destination, write the JSON, report back via `ThemeExportFinished`
/// ("cancelled" stays silent there).
fn save_theme_file_task(contents: String, file_name: String) -> Task<Message> {
    Task::perform(
        tokio::task::spawn_blocking(move || {
            let file = rfd::FileDialog::new()
                .set_title(crate::i18n::t("theme_export_title"))
                .set_file_name(file_name)
                .add_filter("JSON", &["json"])
                .save_file();
            match file {
                Some(path) => {
                    std::fs::write(&path, contents.as_bytes()).map_err(|e| e.to_string())
                }
                None => Err("cancelled".to_string()),
            }
        }),
        |result| {
            let r = match result {
                Ok(r) => r,
                Err(e) => Err(e.to_string()),
            };
            Message::Settings(SettingsMessage::ThemeExportFinished(r))
        },
    )
}
