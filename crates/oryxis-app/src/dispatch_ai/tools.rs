//! The tool-call half: propose, gate, approve, run, report.
//!
//! This is the part with teeth. A proposed command is classified, the
//! auto-exec judge and the local heuristics can block it outright, and
//! anything that is not read-only waits for the user. `ApproveAlways`
//! grants per (tool, host) for the session, never globally.

use iced::Task;
use std::sync::Arc;
use uuid::Uuid;
use crate::app::{AiMessage, Message, Oryxis};
use crate::state::{ChatMessage, ChatRole};
use crate::util::chat_scroll_to_end;

use super::*;

impl Oryxis {
    pub(super) fn handle_ai_tools(&mut self, message: AiMessage) -> Task<Message> {
        match message {
            AiMessage::ChatToolProposed { tab_id, command, risk, thought_signature } => {
                // Gate the tool call against the ORIGIN tab (routed by id, so
                // switching tabs mid-stream can't run a command on the wrong
                // host). Mode + allow-list + loop guards all read from that
                // tab. If it's gone (closed mid-stream), drop the proposal.
                let Some(idx) = self.chat_tab_index(tab_id) else {
                    return Task::none();
                };
                let first_token = command
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                let tab = &self.tabs[idx];
                let allowed = tab
                    .chat_always_run_commands
                    .iter()
                    .any(|c| c == &first_token);
                // Loop guards, read from the origin tab's per-turn auto-exec
                // state. `streak_exceeded` catches a long run of *different*
                // auto-executed commands; `already_auto_ran` catches the
                // model re-proposing the exact same command (the reported
                // `docker --version` loop). Both convert an auto-exec into a
                // confirmation prompt so the loop can't run unattended.
                let streak_exceeded = tab.chat_auto_run_streak >= CHAT_AUTO_RUN_STREAK_MAX;
                let already_auto_ran = tab.chat_auto_run_history.iter().any(|c| c == &command);
                let mode = tab.chat_mode;
                // Decide how this tool call is gated. The branching (mode,
                // allow-list bypass, destructive floor, loop guards, judge,
                // prompt) is a pure function of these flags so it can be
                // unit-tested without a live `Oryxis`, see `classify_tool_gate`.
                let gate = classify_tool_gate(ToolGateInput {
                    mode,
                    allowed,
                    has_chaining: crate::ai::has_shell_chaining(&command),
                    risk_safe: risk == "safe",
                    obviously_destructive: crate::ai::is_obviously_destructive(&command),
                    streak_exceeded,
                    already_auto_ran,
                });
                match gate {
                    // Allow-listed simple command under the streak cap: run now.
                    ToolGate::AutoExec => {
                        return Task::done(Message::Ai(AiMessage::ChatToolExec {
                            tab_id,
                            command,
                            risk,
                            thought_signature,
                        }));
                    }
                    // Loop guard tripped, or the deterministic destructive
                    // floor fired: surface it for explicit approval instead of
                    // running it unattended. This is what stops the reported
                    // runaway loop on its own.
                    ToolGate::Confirm => {
                        return Task::done(Message::Ai(AiMessage::ChatToolGuardBlocked {
                            tab_id,
                            command,
                            thought_signature,
                        }));
                    }
                    // Model-claimed `safe` and nothing above objected: hand to
                    // the independent auto-exec judge, which can only escalate
                    // to a prompt, never approve, and fails safe on error.
                    ToolGate::Judge => {
                        let api_key = self
                            .vault
                            .as_ref()
                            .and_then(|v| v.get_ai_api_key().ok().flatten())
                            .unwrap_or_default();
                        let config = crate::ai::AiConfig {
                            provider: self.ai.provider.clone(),
                            model: self.ai.model.clone(),
                            api_key,
                            api_url: if self.ai.api_url.is_empty() {
                                None
                            } else {
                                Some(self.ai.api_url.clone())
                            },
                            system_prompt: None,
                            reasoning: self.ai.reasoning,
                        };
                        self.tabs[idx].chat_loading = true;
                        let cmd_for_judge = command.clone();
                        let risk_for_exec = risk.clone();
                        let sig_for_exec = thought_signature.clone();
                        let sig_for_block = thought_signature.clone();
                        let judge = Task::perform(
                            crate::ai::judge_auto_exec(config, cmd_for_judge),
                            move |allow| {
                                if allow {
                                    Message::Ai(AiMessage::ChatToolExec {
                                        tab_id,
                                        command: command.clone(),
                                        risk: risk_for_exec.clone(),
                                        thought_signature: sig_for_exec.clone(),
                                    })
                                } else {
                                    Message::Ai(AiMessage::ChatToolGuardBlocked {
                                        tab_id,
                                        command: command.clone(),
                                        thought_signature: sig_for_block.clone(),
                                    })
                                }
                            },
                        );
                        // Track the judge call so Stop cancels a pending
                        // auto-exec decision before it can fire ChatToolExec
                        // and run a command behind the user's back.
                        return self.track_chat_task_for(tab_id, judge);
                    }
                    // Risky / unclassified: fall through to the pending bubble.
                    ToolGate::Prompt => {}
                }
                // Drop the empty assistant placeholder if the model went
                // straight to a tool call without any text.
                if let Some(last) = self.tabs[idx].chat_history.last()
                    && last.role == ChatRole::Assistant
                    && last.content.is_empty()
                {
                    self.tabs[idx].chat_history.pop();
                }
                self.tabs[idx]
                    .chat_history
                    .push(pending_tool_bubble(command, thought_signature));
                self.tabs[idx].chat_loading = false;
                if Some(idx) == self.cur_active_tab() && self.chat_ui.scroll_at_bottom {
                    return chat_scroll_to_end();
                }
            }
            AiMessage::ChatToolGuardBlocked { tab_id, command, thought_signature } => {
                // A mode / loop guard or the independent judge declined to
                // auto-run this command, so surface it on the origin tab for
                // explicit approval exactly like a risky one.
                let Some(idx) = self.chat_tab_index(tab_id) else {
                    return Task::none();
                };
                if let Some(last) = self.tabs[idx].chat_history.last()
                    && last.role == ChatRole::Assistant
                    && last.content.is_empty()
                {
                    self.tabs[idx].chat_history.pop();
                }
                self.tabs[idx]
                    .chat_history
                    .push(pending_tool_bubble(command, thought_signature));
                self.tabs[idx].chat_loading = false;
                if Some(idx) == self.cur_active_tab() && self.chat_ui.scroll_at_bottom {
                    return chat_scroll_to_end();
                }
            }
            AiMessage::ChatToolApprove(command) => {
                // RUN on a pending prompt in the active tab. Only pop the
                // trailing bubble when it's the pending prompt for THIS exact
                // command: a Play click on an older code block must not
                // silently swallow a different command still awaiting a
                // decision (#4).
                let Some(idx) = self.cur_active_tab() else {
                    return Task::none();
                };
                let tab_id = self.tabs.get(idx).map(|t| t._id);
                // Lift the proposal's signature off the bubble before it goes:
                // Gemini needs it back on the request that follows the run.
                let mut thought_signature = None;
                if let Some(tab) = self.tabs.get_mut(idx)
                    && let Some(last) = tab.chat_history.last()
                    && last.role == ChatRole::PendingTool
                    && last.content == command
                {
                    thought_signature =
                        last.tool.as_ref().and_then(|t| t.thought_signature.clone());
                    tab.chat_history.pop();
                }
                // The user just retook control, so a command they approve
                // starts a fresh auto-exec chain (clears the streak / repeat
                // history the loop guard accumulated).
                self.reset_chat_auto_run_guard();
                if let Some(tab_id) = tab_id {
                    // User-approved (was surfaced as needing confirmation), so
                    // record it as `risky` in the reconstructed tool_use.
                    return Task::done(Message::Ai(AiMessage::ChatToolExec {
                        tab_id,
                        command,
                        risk: "risky".into(),
                        thought_signature,
                    }));
                }
            }
            AiMessage::ChatToolApproveAlways(command) => {
                let Some(idx) = self.cur_active_tab() else {
                    return Task::none();
                };
                let tab_id = self.tabs.get(idx).map(|t| t._id);
                let first_token = command
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                let mut thought_signature = None;
                if let Some(tab) = self.tabs.get_mut(idx) {
                    if !first_token.is_empty()
                        && !tab
                            .chat_always_run_commands
                            .iter()
                            .any(|c| c == &first_token)
                    {
                        tab.chat_always_run_commands.push(first_token);
                    }
                    if let Some(last) = tab.chat_history.last()
                        && last.role == ChatRole::PendingTool
                        && last.content == command
                    {
                        // Same lift as the RUN path: the signature has to
                        // outlive the bubble that carried it.
                        thought_signature =
                            last.tool.as_ref().and_then(|t| t.thought_signature.clone());
                        tab.chat_history.pop();
                    }
                    // User retook control: start a fresh auto-exec chain.
                    tab.chat_auto_run_history.clear();
                    tab.chat_auto_run_streak = 0;
                }
                if let Some(tab_id) = tab_id {
                    return Task::done(Message::Ai(AiMessage::ChatToolExec {
                        tab_id,
                        command,
                        risk: "risky".into(),
                        thought_signature,
                    }));
                }
            }
            AiMessage::ChatToolDeny(command) => {
                // User said no. Drop the pending bubble and record the
                // refusal so the next user turn tells the model the command
                // was declined (otherwise it tends to re-propose the same
                // one). No stream is spawned; the user can also just type.
                if let Some(idx) = self.cur_active_tab()
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    if let Some(last) = tab.chat_history.last()
                        && last.role == ChatRole::PendingTool
                        && last.content == command
                    {
                        tab.chat_history.pop();
                    }
                    tab.chat_history.push(ChatMessage::text(
                        ChatRole::System,
                        format!("[user declined to run: {command}]"),
                    ));
                    tab.chat_loading = false;
                }
            }
            AiMessage::ChatToolExec { tab_id, command, risk, thought_signature } => {
                // Run a command in the ORIGIN tab's terminal (routed by id so
                // a tool call can never land in another session). We record a
                // *running* Tool exchange now; the poll delivers `ChatToolResult`
                // which persists the output and fires the analysis followup.
                let Some(idx) = self.chat_tab_index(tab_id) else {
                    return Task::none();
                };
                let terminal = Arc::clone(&self.tabs[idx].active().terminal);
                let pane_label = self.tabs[idx].active().label.clone();
                let privacy_terms = self
                    .privacy_active_for_label(&pane_label)
                    .then(|| self.privacy_terms());
                // Captured before the detached poll below: the spawn
                // can't borrow `self` (same reason the terms are).
                let privacy_classes = self.privacy_classes();
                // Where the shell is about to echo this command. Taken before
                // the write, so the poll reads THIS command's output instead
                // of the tail of the buffer (which is the login banner and
                // the previous commands until something is printed over it).
                // `None` on the alternate screen (tmux, a TUI): rows there are
                // repainted in place, so no anchor is meaningful and the poll
                // keeps the undelimited tail.
                let anchor = terminal
                    .lock()
                    .ok()
                    .and_then(|state| state.abs_cursor_logical_start());

                // Write the command, clearing any half-typed prompt line
                // first (Ctrl+U) so the AI's command isn't concatenated onto
                // stray user input. `write_ok` is false when the session is
                // gone, so we surface that instead of feeding back a stale
                // screen as if it were the command's output.
                let mut bytes = Vec::with_capacity(command.len() + 2);
                bytes.push(0x15); // Ctrl+U: discard a partially-typed line
                bytes.extend_from_slice(command.as_bytes());
                bytes.push(b'\n');
                let write_ok = {
                    let pane = self.tabs[idx].active();
                    if let Some(ref session) = pane.session {
                        session.write(&bytes).is_ok()
                    } else if let Ok(mut state) = pane.terminal.lock() {
                        state.write(&bytes);
                        true
                    } else {
                        false
                    }
                };
                if write_ok {
                    // AI-run commands are commands executed on the host:
                    // mirror them into the history capture like any other
                    // input (the direct write above preserves `write_ok`).
                    self.feed_input_capture(idx, &bytes);
                    // Their output belongs on screen too, so leave the
                    // scrollback the way typed input does (issue #111),
                    // gated on the tab still being the visible one (see
                    // `snap_tab_to_live_edge`).
                    self.snap_tab_to_live_edge(idx);
                }

                let tab = &mut self.tabs[idx];
                // Record this execution against the per-turn loop guard. A
                // later proposal of the same command (or one past the streak
                // cap) is then refused auto-exec by `ChatToolProposed`.
                // User-approval paths reset this first, so the count only ever
                // reflects commands run since the user last took control. Cap
                // the history length so a long agentic turn can't grow it
                // without bound; the repeated command stays within the window.
                tab.chat_auto_run_history.push(command.clone());
                if tab.chat_auto_run_history.len() > 50 {
                    tab.chat_auto_run_history.remove(0);
                }
                tab.chat_auto_run_streak += 1;

                if !write_ok {
                    // Session torn down: don't spawn a poll that would capture
                    // the dead screen and hand it to the model.
                    tab.chat_history.push(ChatMessage::text(
                        ChatRole::System,
                        "[terminal session is not connected; command not sent]",
                    ));
                    tab.chat_loading = false;
                    if Some(idx) == self.cur_active_tab() && self.chat_ui.scroll_at_bottom {
                        return chat_scroll_to_end();
                    }
                    return Task::none();
                }

                // Push a *running* Tool exchange (output `None`). Until the
                // poll fills it in, the message builder renders it as flat
                // text, so an interrupted command never dangles a `tool_use`.
                let tool_id = format!("toolu_{}", Uuid::new_v4().simple());
                tab.chat_history.push(ChatMessage {
                    role: ChatRole::Tool,
                    content: format!("$ {}", command),
                    parsed_md: Vec::new(),
                    reasoning: String::new(),
                    tool: Some(crate::state::ToolExchange {
                        id: tool_id.clone(),
                        command: command.clone(),
                        risk,
                        output: None,
                        thought_signature,
                    }),
                });
                tab.chat_loading = true;

                // Poll the terminal off-thread until the command is done, then
                // deliver the captured text as `ChatToolResult`. Redaction +
                // trim + scrollback are handled by the shared capture helpers.
                //
                // "Done" is three answers, in descending order of proof, and
                // getting this wrong is what fed the model a stale screen: a
                // quiet terminal is NOT evidence a command finished. A
                // `find / -name x` prints nothing for tens of seconds, so the
                // old "no change for 800ms" rule gave up ~1.3s in and reported
                // the login banner as the command's output.
                //
                // - The shell drew a prompt under the echo: it is over, a
                //   short stability window confirms the last write landed.
                // - No prompt we recognize, but the command HAS printed and
                //   then went quiet: most likely done (a custom prompt we
                //   can't match), so accept a longer silence.
                // - Nothing printed at all: wait it out. Only the cap ends it,
                //   and the result says so.
                let poll = Task::perform(
                    async move {
                        let poll_interval = std::time::Duration::from_millis(300);
                        let finished_threshold = std::time::Duration::from_millis(800);
                        let quiet_threshold = std::time::Duration::from_millis(2500);
                        // Anchored captures can tell "still running" from
                        // "done", so waiting longer costs nothing on fast
                        // commands. Without an anchor the cap is the only
                        // brake, so it stays where it was.
                        let max_wait = std::time::Duration::from_secs(if anchor.is_some() {
                            60
                        } else {
                            15
                        });
                        let start_time = std::time::Instant::now();
                        let mut last_snapshot = String::new();
                        let mut stable_since = std::time::Instant::now();
                        let mut timed_out = false;
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        loop {
                            let capture = anchor.and_then(|a| {
                                capture_command_output(
                                    &terminal,
                                    a,
                                    privacy_terms.as_deref(),
                                    privacy_classes,
                                )
                            });
                            // No anchor, or it stopped being readable (the
                            // command opened a TUI, or its output pushed the
                            // echo out of the scrollback ring): the tail is
                            // all there is, under the old stability rule.
                            let (snapshot, threshold) = match capture {
                                Some(c) if c.finished => (c.text, Some(finished_threshold)),
                                Some(c) if c.has_output => (c.text, Some(quiet_threshold)),
                                Some(c) => (c.text, None),
                                None => (
                                    capture_terminal_context(
                                        &terminal,
                                        40,
                                        privacy_terms.as_deref(),
                                        privacy_classes,
                                    ),
                                    Some(finished_threshold),
                                ),
                            };
                            if snapshot != last_snapshot {
                                last_snapshot = snapshot;
                                stable_since = std::time::Instant::now();
                            } else if let Some(threshold) = threshold
                                && stable_since.elapsed() >= threshold
                            {
                                break;
                            }
                            if start_time.elapsed() >= max_wait {
                                timed_out = true;
                                break;
                            }
                            tokio::time::sleep(poll_interval).await;
                        }
                        // A command still producing output at the cap (long
                        // build, `tail -f`, a TUI) would otherwise look "done"
                        // to the model; flag the partial capture.
                        if timed_out {
                            last_snapshot.push_str(&format!(
                                "\n[Note: command may still be running; output captured after {}s]",
                                max_wait.as_secs()
                            ));
                        }
                        last_snapshot
                    },
                    move |output| Message::Ai(AiMessage::ChatToolResult { tab_id, tool_id, output }),
                );
                // Track the poll so Stop / close / reset can cancel a command
                // whose output hasn't come back yet.
                return self.track_chat_task_for(tab_id, poll);
            }
            AiMessage::ChatToolResult { tab_id, tool_id, output } => {
                // Persist the captured output onto its running Tool exchange
                // (pairing it with the tool_use), then fire the analysis
                // followup from the now-complete history.
                let Some(idx) = self.chat_tab_index(tab_id) else {
                    return Task::none();
                };
                let mut attached = false;
                for m in self.tabs[idx].chat_history.iter_mut().rev() {
                    if let Some(t) = m.tool.as_mut()
                        && t.id == tool_id
                        && t.output.is_none()
                    {
                        t.output = Some(output);
                        attached = true;
                        break;
                    }
                }
                if !attached {
                    // The exchange was reset / cleared while polling; nothing
                    // to continue from.
                    self.tabs[idx].chat_loading = false;
                    return Task::none();
                }
                // The exchange is complete (command ran, output attached), so
                // it can be saved before the followup starts streaming over
                // it. Without this a conversation that ends on a tool call
                // would lose the call itself.
                self.flush_chat_history(idx);
                let followup = self.spawn_chat_stream_for(idx);
                return Task::batch(vec![chat_scroll_to_end(), followup]);
            }
            // Routed here by `handle_ai`; anything else is a
            // grouping mistake rather than a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
