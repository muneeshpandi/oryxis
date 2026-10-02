//! What the host editor shows for a field the host leaves unset but a
//! group answers (D4).
//!
//! Resolution runs against the group the FORM currently names, not the
//! one the saved host is in, so moving a host between groups in the
//! combo updates the hints in the same frame. Nothing here changes what
//! is saved: an inherited value stays absent from the host's own row,
//! which is what keeps it following the group afterwards.

use crate::app::Oryxis;
use oryxis_core::models::group::GroupDefaults;

/// The defaults in effect for the group the editor form points at,
/// nearest ancestor first, paired with the label to name in the hint.
pub(crate) struct InheritedContext {
    /// `(field value, group label)` per field, already resolved to the
    /// nearest ancestor that sets it.
    pub username: Option<(String, String)>,
    pub identity: Option<(String, String)>,
    pub proxy: Option<(String, String)>,
    pub terminal_theme: Option<(String, String)>,
    pub startup_snippet: Option<(String, String)>,
}

impl Oryxis {
    /// Build the hints for the current editor form.
    ///
    /// Cheap enough for `view()`: it walks a handful of groups and
    /// resolves at most five labels, with no database access (the
    /// vault's own resolver is not used here because that one hydrates
    /// the proxy PASSWORD, which the editor has no business reading to
    /// draw a label).
    pub(crate) fn editor_inherited(&self) -> InheritedContext {
        let mut ctx = InheritedContext {
            username: None,
            identity: None,
            proxy: None,
            terminal_theme: None,
            startup_snippet: None,
        };
        // The form names its group by breadcrumb path; an empty or
        // unmatched value is the vault root, which inherits nothing.
        let Some(gid) = oryxis_core::models::Group::resolve_path_or_label(
            &self.groups,
            &self.editor_form.group_name,
            &Default::default(),
        ) else {
            return ctx;
        };

        // The one ancestry walk (`Group::ancestry`, cycle-safe), nearest
        // first, so the hint and the dial read the same chain.
        for group in oryxis_core::models::Group::ancestry(&self.groups, gid).chain {
            if let Some(defaults) = group.defaults.as_ref() {
                self.absorb(&mut ctx, defaults, &group.label);
            }
        }
        ctx
    }

    /// Whether the editor form answers the credential parameter itself:
    /// a password that WILL be stored (typed this session, or already
    /// stored and not being cleared) or a key picked. Mirrors the gate
    /// `resolve_effective` applies at connect time (own password / own
    /// `key_id` block a group identity default), read off form state so
    /// the hint updates in the same frame as the field.
    fn editor_form_answers_credentials(&self) -> bool {
        let form = &self.editor_form;
        let password_will_exist = match form.password.resolve() {
            // Edited: stores when non-empty, clears when empty.
            Some(typed) => !typed.is_empty(),
            // Untouched: whatever the vault already holds stands.
            None => form.has_existing_password,
        };
        password_will_exist || form.selected_key.is_some()
    }

    /// Take from `defaults` only what no nearer ancestor already
    /// answered, so the nearest scope wins per field.
    fn absorb(&self, ctx: &mut InheritedContext, defaults: &GroupDefaults, label: &str) {
        if ctx.username.is_none()
            && let Some(u) = defaults.username.clone().filter(|u| !u.is_empty())
        {
            ctx.username = Some((u, label.to_string()));
        }
        if ctx.identity.is_none()
            && let Some(id) = defaults.identity_id
            // A reference to something deleted names nothing, so it is
            // not shown as inherited: the host will fall through too
            // (`resolve_effective` skips it the same way).
            && let Some(ident) = self.identities.iter().find(|i| i.id == id)
            // Credentials are ONE parameter family (the resolver's
            // rule): a form carrying its own password or key answers
            // it, so the hint must not promise an identity the connect
            // will not inherit.
            && !self.editor_form_answers_credentials()
        {
            ctx.identity = Some((ident.label.clone(), label.to_string()));
        }
        if ctx.proxy.is_none()
            && let Some(id) = defaults.proxy_identity_id
            && let Some(proxy) = self.proxy_identities.iter().find(|p| p.id == id)
        {
            ctx.proxy = Some((proxy.label.clone(), label.to_string()));
        }
        if ctx.terminal_theme.is_none()
            && let Some(theme) = defaults.terminal_theme.clone().filter(|t| !t.is_empty())
        {
            ctx.terminal_theme = Some((theme, label.to_string()));
        }
        if ctx.startup_snippet.is_none()
            && let Some(id) = defaults.startup_snippet_id
            && let Some(snippet) = self.snippets.iter().find(|s| s.id == id)
        {
            ctx.startup_snippet = Some((snippet.label.clone(), label.to_string()));
        }
    }
}
