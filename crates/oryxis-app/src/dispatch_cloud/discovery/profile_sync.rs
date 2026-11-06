//! Background profile refresh, independent of the Discover panel:
//! the periodic tick, a single profile's sync, and the merge that
//! lands its result in the vault.
//!
//! The merge is the interesting half: cloud-owned fields follow the
//! provider, user-edited ones stick, and hosts that vanished from
//! the account are retired rather than deleted.

use super::*;

impl Oryxis {
    pub(super) fn handle_discover_profile_sync(
        &mut self,
        message: CloudMessage,
    ) -> Result<Task<Message>, CloudMessage> {
        match message {
            CloudMessage::CloudAutoRefreshTick => {
                // Fan out a sync for every configured profile. Each
                // sync is independent (own Task::perform), so a slow /
                // failing profile doesn't hold up the others. Empty
                // profile list short-circuits.
                let profile_ids: Vec<uuid::Uuid> =
                    self.vaults.cur().cloud_profiles.iter().map(|p| p.id).collect();
                let mut tasks: Vec<Task<Message>> = Vec::new();
                for pid in profile_ids {
                    tasks.push(self.handle_cloud(CloudMessage::CloudProfileSync(pid)));
                }
                return Ok(Task::batch(tasks));
            }
            CloudMessage::CloudProfileSyncManual(profile_id) => {
                // User pressed "Sync now". Report visibly (the silent
                // `CloudProfileSync` below is for the automated tick):
                // guard the plugin with a toast, mark this id manual so
                // the result toasts fire, announce the start, then run
                // the same discovery.
                let Some(label) = self
                    .vaults.cur().cloud_profiles
                    .iter()
                    .find(|p| p.id == profile_id)
                    .map(|p| p.label.clone())
                else {
                    return Ok(Task::none());
                };
                if self.cloud_sync_in_progress.contains(&profile_id) {
                    // Already running: don't stack a second toast/sync.
                    return Ok(Task::none());
                }
                // The provider's plugin must be installed, or `discover`
                // fails deep in the async task with only a log line —
                // which is exactly why "Sync now" looked dead. Guard up
                // front and say so.
                let registered = self.cloud_provider_registry.get(
                    &self
                        .vaults.cur().cloud_profiles
                        .iter()
                        .find(|p| p.id == profile_id)
                        .map(|p| p.provider.clone())
                        .unwrap_or_default(),
                ).is_some();
                let installed = self
                    .vaults.cur().cloud_profiles
                    .iter()
                    .find(|p| p.id == profile_id)
                    .map(|p| self.cloud_provider_installed(&p.provider))
                    .unwrap_or(false);
                if !registered || !installed {
                    return Ok(self.show_toast(
                        crate::i18n::t("cloud_sync_plugin_missing").to_string(),
                    ));
                }
                self.cloud_sync_manual.insert(profile_id);
                let started = self.show_toast(
                    crate::i18n::t("cloud_sync_started").replace("{name}", &label),
                );
                let sync = self.handle_cloud(CloudMessage::CloudProfileSync(profile_id));
                return Ok(Task::batch([started, sync]));
            }
            CloudMessage::CloudProfileSync(profile_id) => {
                // The actual discovery, shared by the manual path above
                // and the automated `CloudAutoRefreshTick`. Silent on its
                // own (no toast); it sets the in-progress flag (drives the
                // card spinner) and routes the result to
                // `CloudProfileSyncResult` for the sticky-fields merge.
                let Some(mut profile) = self
                    .vaults.cur().cloud_profiles
                    .iter()
                    .find(|p| p.id == profile_id)
                    .cloned()
                else {
                    return Ok(Task::none());
                };
                // Already syncing this profile: ignore so the auto-tick
                // never fans a second discovery over one already running.
                if self.cloud_sync_in_progress.contains(&profile_id) {
                    return Ok(Task::none());
                }
                let registry: Arc<CloudProviderRegistry> =
                    self.cloud_provider_registry.clone();
                let Some(provider) = registry.get(&profile.provider) else {
                    return Ok(Task::none());
                };
                if !self.cloud_provider_installed(&profile.provider) {
                    return Ok(Task::none());
                }
                if let Some(vault) = &self.vaults.cur().store {
                    profile.secret =
                        vault.get_cloud_profile_secret(&profile_id).ok().flatten();
                }
                self.cloud_sync_in_progress.insert(profile_id);
                return Ok(Task::perform(
                    async move { provider.discover(&profile).await },
                    move |result| {
                        Message::Cloud(CloudMessage::CloudProfileSyncResult(
                            profile_id,
                            result.map(Box::new).map_err(|e| e.to_string()),
                        ))
                    },
                ));
            }
            CloudMessage::CloudProfileSyncResult(profile_id, result) => {
                // The sync is done (success or not): clear the spinner
                // state for this profile whatever happens below.
                self.cloud_sync_in_progress.remove(&profile_id);
                // Only a USER-initiated sync reports via toast; the
                // automated refresh tick stays silent.
                let manual = self.cloud_sync_manual.remove(&profile_id);
                let profile_name = self
                    .vaults.cur().cloud_profiles
                    .iter()
                    .find(|p| p.id == profile_id)
                    .map(|p| p.label.clone())
                    .unwrap_or_default();
                if self.vaults.cur().store.is_none() {
                    return Ok(Task::none());
                }
                match result {
                    Ok(discovery) => {
                        let now = chrono::Utc::now();
                        // Index AWS-side EC2 results by instance id so
                        // the merge below is O(N+M) instead of O(N*M).
                        let by_id: std::collections::HashMap<
                            String,
                            &oryxis_cloud::DiscoveredEc2,
                        > = discovery
                            .ec2
                            .iter()
                            .map(|e| (e.instance_id.clone(), e))
                            .collect();
                        // Compute merge first so the vault save loop
                        // doesn't have to fight a mutable borrow of
                        // `self.vaults.cur().connections` during the diff.
                        let mut updated: Vec<Connection> = Vec::new();
                        for conn in &self.vaults.cur().connections {
                            let Some(cref) = conn.cloud_ref.as_ref() else {
                                continue;
                            };
                            if cref.profile_id != profile_id {
                                continue;
                            }
                            if cref.resource_type != CloudResourceType::Ec2 {
                                continue;
                            }
                            let mut next = conn.clone();
                            let mut changed = false;
                            if let Some(found) = by_id.get(&cref.resource_id) {
                                if cref.orphaned_at.is_some()
                                    && let Some(cr) = next.cloud_ref.as_mut()
                                {
                                    cr.orphaned_at = None;
                                    changed = true;
                                }
                                // Field-by-field merge: AWS wins unless
                                // the user flagged the field as
                                // customized post-import.
                                if !next
                                    .customized_fields
                                    .iter()
                                    .any(|s| s == "label")
                                {
                                    let new_label = found
                                        .name
                                        .clone()
                                        .unwrap_or_else(|| found.instance_id.clone());
                                    if next.label != new_label {
                                        next.label = new_label;
                                        changed = true;
                                    }
                                }
                                if !next
                                    .customized_fields
                                    .iter()
                                    .any(|s| s == "hostname")
                                {
                                    let new_hostname = found
                                        .public_dns
                                        .clone()
                                        .or_else(|| found.public_ip.clone())
                                        .or_else(|| found.private_dns.clone())
                                        .or_else(|| found.private_ip.clone())
                                        .unwrap_or_default();
                                    if !new_hostname.is_empty()
                                        && next.hostname != new_hostname
                                    {
                                        next.hostname = new_hostname;
                                        changed = true;
                                    }
                                }
                                if !next
                                    .customized_fields
                                    .iter()
                                    .any(|s| s == "username")
                                {
                                    let new_username = found
                                        .default_username
                                        .clone()
                                        .or_else(|| Some("ec2-user".to_string()));
                                    if next.username != new_username {
                                        next.username = new_username;
                                        changed = true;
                                    }
                                }
                            } else {
                                // Resource absent upstream, mark orphan
                                // on first miss (preserve the
                                // timestamp on subsequent syncs so the
                                // "orphaned for N days" math stays
                                // stable).
                                if cref.orphaned_at.is_none()
                                    && let Some(cr) = next.cloud_ref.as_mut()
                                {
                                    cr.orphaned_at = Some(now);
                                    changed = true;
                                }
                            }
                            if changed {
                                next.updated_at = now;
                                updated.push(next);
                            }
                        }
                        let cp_to_save = self
                            .vaults.cur().cloud_profiles
                            .iter()
                            .find(|p| p.id == profile_id)
                            .cloned()
                            .map(|mut cp| {
                                cp.last_discovered = Some(now);
                                cp
                            });
                        if let Some(vault) = &self.vaults.cur().store {
                            // One transaction for the whole refresh batch
                            // (a save per row used to mean a commit per
                            // row), and patch the in-memory lists instead
                            // of re-reading the entire vault.
                            let _ = vault.begin_batch();
                            for conn in &updated {
                                let _ = vault.save_connection(conn, None);
                            }
                            if let Some(cp) = &cp_to_save {
                                let _ = vault.save_cloud_profile(cp, None);
                            }
                            if vault.commit_batch().is_err() {
                                vault.rollback_batch();
                            }
                        }
                        for conn in updated {
                            if let Some(slot) =
                                self.vaults.cur_mut().connections.iter_mut().find(|c| c.id == conn.id)
                            {
                                *slot = conn;
                            } else {
                                self.vaults.cur_mut().connections.push(conn);
                            }
                        }
                        if let Some(cp) = cp_to_save
                            && let Some(slot) =
                                self.vaults.cur_mut().cloud_profiles.iter_mut().find(|p| p.id == cp.id)
                        {
                            *slot = cp;
                        }
                        // Confirm a user-initiated sync finished. Without
                        // this the only sign of success was metadata on
                        // the card, which reads as "nothing happened".
                        // The automated tick stays silent.
                        if manual {
                            return Ok(self.show_toast(
                                crate::i18n::t("cloud_sync_done").replace("{name}", &profile_name),
                            ));
                        }
                    }
                    Err(msg) => {
                        tracing::error!(
                            target = "oryxis::dispatch_cloud",
                            "cloud profile sync failed: {msg}"
                        );
                        // Surface a user-initiated failure instead of only
                        // logging it: a silent failure is indistinguishable
                        // from the action doing nothing. Auto-tick failures
                        // stay in the log (they recur on the next tick).
                        if manual {
                            return Ok(self.show_toast(
                                crate::i18n::t("cloud_sync_failed")
                                    .replace("{name}", &profile_name)
                                    .replace("{error}", &msg),
                            ));
                        }
                    }
                }
            }
            // The parent routed us here, so a message that is not
            // in this family is a grouping mistake. Hand it back
            // rather than swallow it.
            m => return Err(m),
        }
        // Reached when a non-manual (auto-tick) sync result lands: the
        // merge is done and there is no toast to show.
        Ok(Task::none())
    }
}
