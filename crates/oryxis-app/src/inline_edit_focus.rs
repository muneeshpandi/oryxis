//! Keeps an inline rename focused while the list around it reloads.
//!
//! iced matches widget state by index, and a rename lives INSIDE the
//! file list it renames: an SFTP pane or a sidebar Files browser swaps
//! the row's label for a `text_input`. A listing that lands while the
//! user types (an upload finishing in that folder, a refresh, a
//! transfer completing) can insert or remove rows ABOVE the renamed
//! one, and the input then meets another row's state at its new index
//! and loses its focus. A keyed list would match by path instead, but
//! the fork's `keyed::Column` diffs a child without checking its widget
//! type, so a row turning into a rename input under the same key
//! panics ("Downcast widget state", measured on `sftp-delete-key.ice`).
//!
//! So the funnel answers it after the fact, the way `reconcile_tab_order`
//! repairs the strip: after every `update`, each open rename is summed
//! up as the rows that precede it, and when that sum changed under the
//! same rename the input is focused again. Doing it here rather than in
//! the arms that replace a listing is the point: the SFTP panes alone
//! have six of them, and a seventh added later inherits this for free.
//! What is lost is the caret position, which a re-focus cannot restore.

use std::hash::{Hash, Hasher};

use iced::Task;

use crate::app::{Message, Oryxis};

/// One open inline rename as the funnel last saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineEditAnchor {
    /// Which rename: the surface plus the path being renamed, so a
    /// rename closed and another opened never reads as a displaced one.
    identity: String,
    /// Hash of everything drawn above the renamed row.
    preceding: u64,
    /// The input to focus again.
    input_id: &'static str,
}

impl Oryxis {
    /// Re-focus every inline rename whose row moved since the last
    /// `update`. Costs nothing while no rename is open.
    pub(crate) fn refocus_displaced_inline_edits(&mut self) -> Option<Task<Message>> {
        let current = self.inline_edit_anchors_now();
        if current.is_empty() && self.inline_edit_anchors.is_empty() {
            return None;
        }
        let displaced: Vec<&'static str> = current
            .iter()
            .filter(|now| {
                self.inline_edit_anchors
                    .iter()
                    .any(|before| before.identity == now.identity && before.preceding != now.preceding)
            })
            .map(|a| a.input_id)
            .collect();
        self.inline_edit_anchors = current;
        // Only one input can hold the focus, and two renames open at
        // once (an SFTP pane and a sidebar browser) can only both be
        // displaced by the same message by coincidence: the last one
        // wins, which is as good as any.
        displaced
            .last()
            .map(|id| crate::widgets::focus_input(iced::widget::Id::new(id)))
    }

    fn inline_edit_anchors_now(&self) -> Vec<InlineEditAnchor> {
        let mut anchors = Vec::new();
        if let Some(rename) = &self.cur_sftp().rename {
            let pane = self.cur_sftp().pane(rename.side);
            let mut h = std::collections::hash_map::DefaultHasher::new();
            pane.show_hidden.hash(&mut h);
            let found = if pane.is_remote {
                pane.remote_path.hash(&mut h);
                let parent = pane.remote_path.trim_end_matches('/');
                preceding_names(
                    pane.remote_entries.iter().map(|e| e.name.as_str()),
                    |name| {
                        if parent.is_empty() {
                            format!("/{name}")
                        } else {
                            format!("{parent}/{name}")
                        }
                    },
                    &rename.original_path,
                    &mut h,
                )
            } else {
                pane.local_path.hash(&mut h);
                preceding_names(
                    pane.local_entries.iter().map(|e| e.name.as_str()),
                    |name| pane.local_path.join(name).to_string_lossy().into_owned(),
                    &rename.original_path,
                    &mut h,
                )
            };
            if found {
                anchors.push(InlineEditAnchor {
                    identity: format!("sftp:{:?}:{}", rename.side, rename.original_path),
                    preceding: h.finish(),
                    input_id: crate::views::sftp::RENAME_INPUT_ID,
                });
            }
        }
        for pane in self.tabs.iter().flat_map(|t| t.pane_grid.panes.values()) {
            let Some((path, _)) = &pane.files.rename else {
                continue;
            };
            let files = &pane.files;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            files.path.hash(&mut h);
            files.new_entry.is_some().hash(&mut h);
            files.show_hidden.hash(&mut h);
            let found = preceding_names(
                files.entries.iter().map(|e| e.name.as_str()),
                |name| crate::dispatch_sidebar_files::files_join(&files.path, name),
                path,
                &mut h,
            );
            if found {
                anchors.push(InlineEditAnchor {
                    identity: format!("files:{}:{path}", pane.id),
                    preceding: h.finish(),
                    input_id: "sidebar-files-rename",
                });
            }
        }
        anchors
    }
}

/// Hash the names listed before `target` into `h`, answering whether
/// `target` is in the list at all (a rename whose row is gone has
/// nothing to keep focused).
fn preceding_names<'a>(
    names: impl Iterator<Item = &'a str>,
    full: impl Fn(&str) -> String,
    target: &str,
    h: &mut impl Hasher,
) -> bool {
    for name in names {
        if full(name) == target {
            return true;
        }
        name.hash(h);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::preceding_names;
    use std::hash::Hasher;

    fn sum(names: &[&str], target: &str) -> Option<u64> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        preceding_names(names.iter().copied(), |n| format!("/d/{n}"), target, &mut h)
            .then(|| h.finish())
    }

    #[test]
    fn a_row_inserted_above_the_rename_changes_the_sum() {
        let before = sum(&["a", "c", "e"], "/d/e");
        let after = sum(&["a", "b", "c", "e"], "/d/e");
        assert!(before.is_some() && after.is_some());
        assert_ne!(before, after);
    }

    #[test]
    fn a_row_added_below_the_rename_leaves_it_alone() {
        assert_eq!(sum(&["a", "c"], "/d/a"), sum(&["a", "c", "z"], "/d/a"));
    }

    #[test]
    fn a_renamed_row_that_left_the_listing_is_not_anchored() {
        assert_eq!(sum(&["a", "b"], "/d/x"), None);
    }
}
