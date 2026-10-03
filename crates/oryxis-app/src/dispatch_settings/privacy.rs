//! Settings dispatch helpers: Privacy Mode (issue #78). The privacy
//! arms plus the mask-term / display-redaction helpers. Split out of
//! dispatch_settings/mod.rs.

use super::*;

impl Oryxis {
    /// Usernames half the world's servers share: masking them would
    /// turn every `ls -l` into noise without protecting anything
    /// (issue #78). Seeds the editable never-mask setting on first
    /// boot; the user's stored list is authoritative after that.
    pub(crate) const PRIVACY_NEVER_MASK_DEFAULT: &'static [&'static str] = &[
        "root", "admin", "administrator", "ubuntu", "debian", "centos",
        "fedora", "alpine", "ec2-user", "azureuser", "opc", "core",
        "vagrant", "ansible", "docker", "bitnami", "guest", "user",
        "test", "postgres", "mysql", "redis", "mongodb", "oracle",
        "git", "www-data", "nginx", "apache", "nobody", "daemon",
    ];

    /// The never-mask default as the comma-joined string the setting
    /// stores and the Settings field displays.
    pub(crate) fn privacy_never_mask_default() -> String {
        Self::PRIVACY_NEVER_MASK_DEFAULT.join(", ")
    }

    /// The effective global Privacy Mode state: the volatile session
    /// override (issue #78) wins over the persisted setting. Use this,
    /// never `privacy.mode` directly, when deciding whether a
    /// surface without a per-host context masks.
    pub(crate) fn privacy_global_active(&self) -> bool {
        self.privacy.session_override
            .unwrap_or(self.privacy.mode)
    }

    /// Whether Privacy Mode is active for a connection. The session
    /// override (issue #78) wins over EVERYTHING, per-host overrides
    /// included: "I'm about to share my screen" must not leak through
    /// a host configured with privacy off. Below it, the per-host
    /// override (`Connection.privacy_mode`) wins over the global
    /// setting; `None` inherits the global default.
    pub(crate) fn privacy_active(&self, conn: &oryxis_core::models::Connection) -> bool {
        self.privacy_active_for_override(conn.privacy_mode)
    }

    /// The same resolution against a bare per-host override, for
    /// surfaces that hold one without a `Connection` behind it (the
    /// host editor's form, which is describing a row it has not
    /// written yet). Single authority: `privacy_active` delegates here
    /// so the precedence cannot be stated twice and drift.
    pub(crate) fn privacy_active_for_override(&self, over: Option<bool>) -> bool {
        self.privacy.session_override
            .unwrap_or_else(|| over.unwrap_or(self.privacy.mode))
    }

    /// Strings Privacy Mode masks literally wherever they appear (live
    /// terminal + session-log viewer): every saved connection's host
    /// address AND username (issue #78: `ls -la` / `dir /Q` owner
    /// columns), lowercased and deduped. Plain DNS names have no
    /// detectable shape (file extensions collide with ccTLDs: `main.rs`,
    /// `install.sh` are FQDN-shaped), so the known values are matched
    /// exactly instead of guessed. The user's never-mask list (seeded
    /// with `PRIVACY_NEVER_MASK_DEFAULT`) keeps shared usernames like
    /// `root` readable; the always-mask list adds arbitrary literals
    /// (company names, internal domains). Very short terms are dropped,
    /// masking every "web" in sight would be noise, not privacy.
    pub(crate) fn privacy_terms(&self) -> Vec<String> {
        // Per-class gates (issue #78 block 1): a disabled hostnames /
        // usernames class drops those derived values here, at the
        // source, so every terms consumer (terminal spans, display
        // redactor, AI context) honours it for free. The always-mask
        // list is class-less: an explicit user entry always masks.
        //
        // The username is the EFFECTIVE one (`effective_username`: the
        // host's own, else its folder's default, else its identity's):
        // that is the word the prompt, the progress lines and the card
        // carry, and a host that inherits it has nothing in the field.
        let users: Vec<String> = if self.privacy.mask_usernames {
            self.connections
                .iter()
                .filter_map(|c| self.effective_username(c))
                .collect()
        } else {
            Vec::new()
        };
        let hosts = self
            .connections
            .iter()
            .filter(|_| self.privacy.mask_hostnames)
            .map(|c| c.hostname.as_str());
        crate::widgets::assemble_privacy_terms(
            hosts.chain(users.iter().map(String::as_str)),
            &self.privacy.always_mask,
            &self.privacy.never_mask,
        )
    }

    /// The per-class gates in the terminal widget's shape (issue #78).
    /// The hostnames class has no flag there: it exists only as terms,
    /// filtered in [`Self::privacy_terms`].
    pub(crate) fn privacy_classes(&self) -> oryxis_terminal::PrivacyClasses {
        oryxis_terminal::PrivacyClasses {
            public_ips: self.privacy.mask_public_ips,
            private_ips: self.privacy.mask_private_ips,
            usernames: self.privacy.mask_usernames,
        }
    }

    /// Privacy Mode for a terminal pane, resolved from its label. Host
    /// panes match a saved connection (so the per-host override applies);
    /// local shells / WSL / PowerShell fall back to the global default.
    pub(crate) fn privacy_active_for_label(&self, label: &str) -> bool {
        let base = label.trim_end_matches(" (disconnected)");
        self.connections
            .iter()
            .find(|c| c.label == base)
            .map(|c| self.privacy_active(c))
            .unwrap_or_else(|| self.privacy_global_active())
    }

    /// A host/tab label as rendered under Privacy Mode (issue #78):
    /// labels routinely embed the address (quick-connect labels
    /// literally are `user@host`, users name hosts after their
    /// hostname), so masked surfaces run them through the same display
    /// redactor as recorded output. `lookup` is the label that keys
    /// the saved connection (a tab's automatic label, so a custom
    /// rename keeps the per-host override); `display` is what actually
    /// renders. Callers with a hover-reveal state skip the call while
    /// hovered, mirroring the card address reveal. `terms` comes from
    /// one `privacy_terms()` call per view pass, never per row.
    pub(crate) fn privacy_display_label(
        &self,
        lookup: &str,
        display: &str,
        terms: &[String],
    ) -> String {
        if self.privacy_active_for_label(lookup) {
            crate::widgets::redact_for_display(display, terms, self.privacy_classes())
        } else {
            display.to_string()
        }
    }
}

impl Oryxis {
    /// Privacy Mode arms: the global toggle, the session override, the
    /// always / never mask lists and the per-class gates.
    pub(super) fn handle_settings_privacy(
        &mut self,
        message: SettingsMessage,
    ) -> Result<Task<Message>, SettingsMessage> {
        match message {
            SettingsMessage::TogglePrivacyMode => {
                self.privacy.mode = !self.privacy.mode;
                self.persist_setting(
                    "privacy_mode",
                    if self.privacy.mode { "true" } else { "false" },
                );
            }
            SettingsMessage::TogglePrivacySessionOverride => {
                // One press forces the opposite of the configured
                // global state (per-host overrides included); the next
                // press falls back to the settings. The toast spells
                // the resulting state out because a silent flip is how
                // the original #53 confusion happened.
                self.privacy.session_override = match self.privacy.session_override {
                    None => Some(!self.privacy.mode),
                    Some(_) => None,
                };
                let key = match self.privacy.session_override {
                    Some(true) => "privacy_toast_session_on",
                    Some(false) => "privacy_toast_session_off",
                    None => "privacy_toast_follow",
                };
                return Ok(self.show_toast(crate::i18n::t(key).to_string()));
            }
            SettingsMessage::SettingPrivacyAlwaysMaskAction(action) => {
                // Textarea buffer first; the `String` mirror (what
                // `privacy_terms()` reads per frame) and the vault row
                // only refresh on actual edits, not cursor motion.
                let was_edit = action.is_edit();
                self.privacy.always_mask_editor.perform(action);
                if was_edit {
                    self.privacy.always_mask =
                        self.privacy.always_mask_editor.text();
                    let v = self.privacy.always_mask.clone();
                    self.persist_setting("privacy_always_mask", &v);
                }
            }
            SettingsMessage::SettingPrivacyNeverMaskAction(action) => {
                let was_edit = action.is_edit();
                self.privacy.never_mask_editor.perform(action);
                if was_edit {
                    self.privacy.never_mask =
                        self.privacy.never_mask_editor.text();
                    let v = self.privacy.never_mask.clone();
                    self.persist_setting("privacy_never_mask", &v);
                }
            }
            SettingsMessage::TogglePrivacyMaskClass(class) => {
                use crate::messages::PrivacyMaskClass;
                let (key, field): (&str, &mut bool) = match class {
                    PrivacyMaskClass::PublicIps => (
                        "privacy_mask_public_ips",
                        &mut self.privacy.mask_public_ips,
                    ),
                    PrivacyMaskClass::PrivateIps => (
                        "privacy_mask_private_ips",
                        &mut self.privacy.mask_private_ips,
                    ),
                    PrivacyMaskClass::Usernames => (
                        "privacy_mask_usernames",
                        &mut self.privacy.mask_usernames,
                    ),
                    PrivacyMaskClass::Hostnames => (
                        "privacy_mask_hostnames",
                        &mut self.privacy.mask_hostnames,
                    ),
                };
                *field = !*field;
                let value = if *field { "true" } else { "false" };
                self.persist_setting(key, value);
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
