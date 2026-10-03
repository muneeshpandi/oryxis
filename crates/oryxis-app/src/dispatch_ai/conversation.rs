//! One conversation: composing, sending, and the stream coming back.
//!
//! The stream is tracked per TAB (`track_chat_task_for`), so Stop, a
//! reset, or closing the tab abort the right one. Every terminal arm
//! (`ChatStreamDone`, `ChatError`, `ChatStop`) settles the history,
//! which is where the vault flush happens.

use iced::widget::text_editor;
use iced::Task;
use crate::app::{AiMessage, Message, Oryxis};
use crate::state::{ChatMessage, ChatRole};
use crate::util::chat_scroll_to_end;


impl Oryxis {
    pub(super) fn handle_ai_conversation(&mut self, message: AiMessage) -> Task<Message> {
        match message {
            AiMessage::ChatInputAction(action) => {
                self.chat_ui.input.perform(action);
            }
            AiMessage::ChatScrolled(relative_y) => {
                // Strict end check (not "near end"), relative_offset.y
                // becomes 1.0 when the user is exactly at the bottom.
                // Tiny epsilon covers f32 rounding from the layout pass.
                self.chat_ui.scroll_at_bottom = relative_y >= 0.999;
            }
            AiMessage::ChatResetConversation => {
                // Cancel any in-flight stream first, otherwise the detached
                // tool-followup task would keep running and re-populate the
                // history we're about to clear. Acts on the active tab (the
                // conversation the user is looking at).
                self.abort_active_chat_task();
                self.reset_chat_auto_run_guard();
                if let Some(idx) = self.cur_active_tab() {
                    if let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_history.clear();
                    }
                    // Whatever was already saved stays saved and readable on
                    // the History screen; the next exchange just starts its
                    // own conversation instead of appending to the one the
                    // user cleared.
                    self.detach_saved_chat(idx);
                }
                self.chat_ui.scroll_at_bottom = true;
            }
            AiMessage::ChatModeChanged(mode) => {
                // Apply to the active tab's conversation and remember it as
                // the default for new tabs (process-wide default + setting).
                if let Some(idx) = self.cur_active_tab()
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    tab.chat_mode = mode;
                }
                crate::state::set_default_chat_mode(mode);
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_default_mode", mode.as_setting());
                }
            }
            AiMessage::SendChat => {
                let input = self.chat_ui.input.text().trim().to_string();
                if input.is_empty() || !self.ai.enabled {
                    return Task::none();
                }
                let Some(idx) = self.cur_active_tab() else {
                    return Task::none();
                };
                if idx >= self.tabs.len() {
                    return Task::none();
                }

                let tab = &mut self.tabs[idx];
                tab.chat_history
                    .push(ChatMessage::text(ChatRole::User, input));
                // A fresh user turn clears the auto-exec guard so the
                // streak / repeat history from the previous turn
                // doesn't bleed into this one.
                tab.chat_auto_run_history.clear();
                tab.chat_auto_run_streak = 0;
                self.chat_ui.input = text_editor::Content::new();
                // Sending a message snaps focus back to the latest
                // exchange, so the next assistant response should
                // also follow (until the user scrolls up again).
                self.chat_ui.scroll_at_bottom = true;

                let stream_task = self.spawn_chat_stream_for(idx);
                return Task::batch(vec![chat_scroll_to_end(), stream_task]);
            }
            AiMessage::ChatStreamChunk { tab_id, delta } => {
                // Route to the origin tab by id (not `active_tab`): the user
                // may have switched tabs while this stream keeps running.
                let is_active = self.chat_tab_index(tab_id) == self.cur_active_tab();
                if let Some(idx) = self.chat_tab_index(tab_id) {
                    // Markdown parse is O(content), so re-parsing on every
                    // token makes a long streamed reply O(n^2). Throttle to
                    // ~10 parses/s off the tab's own last-parse clock (a
                    // shared static would never throttle when two tabs stream
                    // at once); `ChatStreamDone` does the final parse.
                    let now = std::time::Instant::now();
                    let due = self.tabs[idx]
                        .chat_last_md_parse
                        .map(|t| now.duration_since(t).as_millis() >= 100)
                        .unwrap_or(true);
                    if let Some(last) = self.tabs[idx].chat_history.last_mut()
                        && last.role == ChatRole::Assistant
                    {
                        last.content.push_str(&delta);
                        if due {
                            last.parsed_md =
                                iced::widget::markdown::parse(&last.content).collect();
                        }
                    }
                    if due {
                        self.tabs[idx].chat_last_md_parse = Some(now);
                    }
                }
                // Only follow the scroll when the streaming tab is the one
                // on screen; a background stream must not yank the view.
                if is_active && self.chat_ui.scroll_at_bottom {
                    return chat_scroll_to_end();
                }
            }
            AiMessage::ChatStreamReasoning { tab_id, delta } => {
                // Accumulate onto the same assistant bubble the text deltas
                // feed, but in its own field: this is not part of the answer
                // and is never rendered. No markdown re-parse and no scroll
                // follow, so a long chain-of-thought costs nothing on screen.
                if let Some(idx) = self.chat_tab_index(tab_id)
                    && let Some(last) = self.tabs[idx].chat_history.last_mut()
                    && last.role == ChatRole::Assistant
                {
                    last.reasoning.push_str(&delta);
                }
            }
            AiMessage::ChatStreamDone { tab_id } => {
                let is_active = self.chat_tab_index(tab_id) == self.cur_active_tab();
                if let Some(idx) = self.chat_tab_index(tab_id) {
                    // Final parse so the rendered markdown can't lag behind
                    // the throttled streaming parses above.
                    if let Some(last) = self.tabs[idx].chat_history.last_mut()
                        && last.role == ChatRole::Assistant
                    {
                        last.parsed_md =
                            iced::widget::markdown::parse(&last.content).collect();
                    }
                    // Empty assistant placeholders are filtered out at the
                    // view layer and excluded from the message-builder when
                    // we send to the model, so we don't try to pop them
                    // here. (Popping was racy when a tool followup pushed
                    // its own placeholder before the original stream's Done
                    // arrived.)
                    // The stream finished on its own; drop the now-spent
                    // abort handle so a later Stop doesn't cancel nothing.
                    self.tabs[idx].chat_task = None;
                    self.tabs[idx].chat_loading = false;
                    // The reply is complete: a settle point where saving the
                    // conversation cannot capture a half-streamed answer.
                    self.flush_chat_history(idx);
                }
                if is_active && self.chat_ui.scroll_at_bottom {
                    return chat_scroll_to_end();
                }
            }
            AiMessage::ChatStop => {
                // User asked to stop. Abort the active tab's live stream (and
                // the detached tool-followup pipeline it feeds) and freeze
                // the auto-exec guard where it is, so nothing else runs until
                // the user sends the next message.
                self.abort_active_chat_task();
            }
            AiMessage::ChatError { tab_id, error } => {
                // Provider/network failures get their own role so the
                // bubble can render with an error treatment + Retry,
                // instead of being indistinguishable from a real reply.
                let is_active = self.chat_tab_index(tab_id) == self.cur_active_tab();
                if let Some(idx) = self.chat_tab_index(tab_id) {
                    // If the stream errored before the model wrote any
                    // text, drop the empty assistant placeholder so we
                    // don't render a blank bubble above the error.
                    if let Some(last) = self.tabs[idx].chat_history.last()
                        && last.role == ChatRole::Assistant
                        && last.content.is_empty()
                    {
                        self.tabs[idx].chat_history.pop();
                    }
                    self.tabs[idx]
                        .chat_history
                        .push(ChatMessage::text(ChatRole::Error, error));
                    self.tabs[idx].chat_task = None;
                    self.tabs[idx].chat_loading = false;
                }
                if is_active && self.chat_ui.scroll_at_bottom {
                    return chat_scroll_to_end();
                }
            }
            AiMessage::ChatRetry => {
                // Strip the trailing error bubble plus any partial-stream
                // assistant remnants (empty placeholders, or text that
                // arrived before the error), then resume a fresh assistant
                // stream over whatever remains. Unlike the old flow this does
                // NOT require a trailing user message: when the error followed
                // a tool execution the history ends on a System bubble (the
                // command + its output), and the stream continues from there
                // (#3). It also does not pop the user message, so nothing is
                // lost if the remaining history isn't what we expected.
                let Some(idx) = self.cur_active_tab() else {
                    return Task::none();
                };
                if idx >= self.tabs.len() {
                    return Task::none();
                }
                {
                    let tab = &mut self.tabs[idx];
                    while matches!(
                        tab.chat_history.last().map(|m| &m.role),
                        Some(ChatRole::Error) | Some(ChatRole::Assistant)
                    ) {
                        tab.chat_history.pop();
                    }
                    // Nothing left to respond to (whole conversation was error
                    // remnants): don't spawn an empty stream.
                    if tab.chat_history.is_empty() {
                        tab.chat_loading = false;
                        return Task::none();
                    }
                }
                // Reconcile the saved copy with the shorter history BEFORE
                // the new answer arrives. `flush_chat_history` tracks what
                // it stored as a COUNT, so it only notices a rewrite is due
                // while the live history is shorter than that count: let the
                // stream fill the gap first and the counts match again, the
                // flush reports "nothing new", and the conversation stays
                // saved with the error bubble the user just retried away,
                // permanently missing the answer that replaced it. (Reaching
                // that needs the error to have been saved, which opening the
                // History screen mid-conversation does.) Cheap and
                // idempotent, and a no-op when saving is off.
                self.flush_chat_history(idx);
                self.chat_ui.scroll_at_bottom = true;
                let stream_task = self.spawn_chat_stream_for(idx);
                return Task::batch(vec![chat_scroll_to_end(), stream_task]);
            }
            // Routed here by `handle_ai`; anything else is a
            // grouping mistake rather than a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
