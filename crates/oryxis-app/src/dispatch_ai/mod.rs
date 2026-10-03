//! `Oryxis::handle_ai`, match arms for the AI side of the app:
//! provider/model/api-key Settings panel knobs, and the chat sidebar
//! conversation flow (send, receive, retry, tool exec).

#![allow(clippy::result_large_err)]

use iced::Task;

use std::sync::Arc;

use uuid::Uuid;

use crate::app::{AiMessage, Message, Oryxis};
use crate::state::{ChatMessage, ChatRole};

/// Cap on consecutive AI-auto-executed commands per user turn. Past this,
/// further auto-exec is refused and the command is surfaced for explicit
/// approval. A backstop for runaway loops of *different* commands;
/// exact-repeat loops are caught earlier by `chat_auto_run_history`.
const CHAT_AUTO_RUN_STREAK_MAX: usize = 12;

/// Snapshot the last `n_lines` of a terminal pane's visible grid as text
/// for the AI context. Trailing whitespace is trimmed per line (alacritty
/// pads empty cells with spaces, which would otherwise inflate the token
/// count for no signal). When `privacy_terms` is `Some`, every line is run
/// through the same redaction the live terminal and session-log viewer use,
/// so IPs / hostnames / usernames the user chose to hide never leave the
/// machine in an AI request. Free function (not a method) so it works both
/// synchronously and inside the detached tool-followup `tokio::spawn`, which
/// can't borrow `self`. Returns an empty string if the lock is poisoned.
fn capture_terminal_context(
    terminal: &std::sync::Mutex<crate::state::TerminalState>,
    n_lines: usize,
    privacy_terms: Option<&[String]>,
    privacy_classes: oryxis_terminal::PrivacyClasses,
) -> String {
    let Ok(state) = terminal.lock() else {
        return String::new();
    };
    // `tail_text` reads the last N rows including scrollback history (already
    // trimmed, wide-char-aware), so a command whose output scrolled off the
    // visible screen still reaches the model. Redact per line under Privacy.
    state
        .tail_text(n_lines)
        .into_iter()
        .map(|l| match privacy_terms {
            Some(terms) => crate::widgets::redact_for_display(&l, terms, privacy_classes),
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Cap on the lines of a single command's output handed to the model. Past
/// this the middle is elided (see [`oryxis_terminal::RegionText`]).
const MAX_TOOL_OUTPUT_LINES: usize = 200;

/// One anchored read of what a command printed: the region of the grid from
/// the row the command was echoed on down to the last row with content.
struct CommandCapture {
    /// The text as the model will see it, redacted under Privacy Mode.
    text: String,
    /// A shell prompt has been drawn under the echo, so the command is over
    /// and a short stability window is enough to call the capture complete.
    finished: bool,
    /// The command has printed something. A quiet screen with nothing printed
    /// is not evidence of anything: `find / -name x` prints its first line
    /// tens of seconds in, so the poll must keep waiting rather than hand the
    /// model an empty answer.
    has_output: bool,
}

/// Read the output of the command echoed at absolute row `anchor`.
///
/// This is what keeps a tool result about the command that ran: an
/// undelimited tail of the grid returns whatever was already on screen (the
/// login banner, the previous commands) whenever the command itself printed
/// little or nothing yet, and the model reports that back as the answer.
///
/// `None` when the anchor is no longer readable (the pane entered the
/// alternate screen, or the row rotated out of the scrollback ring), which
/// leaves the caller to fall back to [`capture_terminal_context`].
fn capture_command_output(
    terminal: &std::sync::Mutex<crate::state::TerminalState>,
    anchor: i64,
    privacy_terms: Option<&[String]>,
    privacy_classes: oryxis_terminal::PrivacyClasses,
) -> Option<CommandCapture> {
    let region = {
        let state = terminal.lock().ok()?;
        state.text_from_abs(anchor, MAX_TOOL_OUTPUT_LINES)?
    };
    // The first line is the echo (prompt + command); everything under it is
    // this command's own output, and the shell's next prompt closes it.
    //
    // Two ways to know the prompt is back, and neither subsumes the other.
    //
    // The strong one is the CURSOR parked on that line: a shell waiting for
    // input sits there, and a command that printed a line and kept working
    // left the cursor on the row below (which the region trims away). That
    // gate is what stopped `# end of file`, the last line of a `cat`, from
    // reading as "the command is over"; behind it the marker test can afford
    // to be loose enough for prompts no marker list covers, fish's `~> ` and
    // oh-my-zsh's `\u{279c}  ~ `, which ends on the path rather than the marker.
    // Loose, but not absent: a progress bar redrawn in place is under the
    // cursor too, and `Working...` must not read as a prompt.
    //
    // The weak one is the line ENDING at a marker, which stands on its own
    // for the moments the cursor is elsewhere.
    let last_content = region.lines.iter().rposition(|l| !l.trim().is_empty());
    let finished = last_content.is_some_and(|i| {
        let line = &region.lines[i];
        i > 0
            && ((region.cursor_line == Some(i)
                && crate::command_capture::line_could_be_prompt(line))
                || crate::command_capture::line_is_bare_prompt(line))
    });
    let body_end = if finished { last_content.unwrap_or(0) } else { region.lines.len() };
    let has_output = region
        .lines
        .iter()
        .take(body_end)
        .skip(1)
        .any(|l| !l.trim().is_empty());

    let mut out: Vec<String> = Vec::with_capacity(region.lines.len() + 2);
    for (i, line) in region.lines.iter().enumerate() {
        if region.dropped > 0 && i == region.head_len {
            out.push(format!(
                "[... {} lines omitted from the middle of the output ...]",
                region.dropped
            ));
        }
        out.push(match privacy_terms {
            Some(terms) => crate::widgets::redact_for_display(line, terms, privacy_classes),
            None => line.clone(),
        });
    }
    if finished && !has_output {
        out.push("[command completed with no output]".to_string());
    }
    Some(CommandCapture { text: out.join("\n"), finished, has_output })
}

/// Reconstruct the provider-agnostic message list from chat history,
/// rebuilding native `tool_use` / `tool_result` pairs from completed `Tool`
/// exchanges. Rules that keep the request valid on every provider:
///
/// - A completed `Tool` exchange (output `Some`) merges its `tool_use` into
///   the immediately-preceding assistant text turn (the canonical single
///   `[text, tool_use]` turn), or emits a standalone assistant `tool_use`
///   turn when there is no preamble; then a `tool` turn carries the result.
/// - A `Tool` exchange still running (output `None`) is emitted as flat text,
///   so an in-flight command never leaves a dangling `tool_use` that would
///   400 the next request.
/// - `Error` / `PendingTool` bubbles and empty assistant placeholders are
///   dropped. `System` notes ride along as plain user text.
///
/// Pure (no `self`), so the per-provider shaping is unit-tested end to end.
fn build_provider_messages(history: &[ChatMessage]) -> Vec<crate::ai::ChatMsg> {
    use crate::ai::{ChatMsg, ToolResultMsg, ToolUseMsg};
    let mut out: Vec<ChatMsg> = Vec::new();
    for m in history {
        match m.role {
            ChatRole::Error | ChatRole::PendingTool => {}
            ChatRole::Assistant => {
                if !m.content.is_empty() {
                    let mut a = ChatMsg::text("assistant", &m.content);
                    // Carry the turn's chain-of-thought back to the
                    // provider; DeepSeek's thinking mode 400s the request
                    // when a prior assistant turn arrives without it (#105).
                    if !m.reasoning.is_empty() {
                        a.reasoning = Some(m.reasoning.clone());
                    }
                    out.push(a);
                }
            }
            ChatRole::User | ChatRole::System => {
                out.push(ChatMsg::text("user", &m.content));
            }
            ChatRole::Tool => {
                let Some(tx) = &m.tool else { continue };
                match &tx.output {
                    // Still running: flat text, no structured tool_use.
                    None => out.push(ChatMsg::text(
                        "user",
                        format!("[Running command: {}]", tx.command),
                    )),
                    Some(output) => {
                        let tu = ToolUseMsg {
                            id: tx.id.clone(),
                            command: tx.command.clone(),
                            risk: tx.risk.clone(),
                            // Gemini signs each function call and demands the
                            // signature back on the next request; we rebuild
                            // the call from name+args, so it has to ride here.
                            thought_signature: tx.thought_signature.clone(),
                        };
                        // Merge into the preceding assistant text turn if any,
                        // else emit a standalone assistant tool_use turn.
                        if let Some(last) = out.last_mut()
                            && last.role == "assistant"
                            && last.tool_use.is_none()
                            && last.tool_result.is_none()
                        {
                            last.tool_use = Some(tu);
                        } else {
                            let mut a = ChatMsg::text("assistant", "");
                            a.tool_use = Some(tu);
                            out.push(a);
                        }
                        out.push(ChatMsg::tool_result(ToolResultMsg {
                            id: tx.id.clone(),
                            output: output.clone(),
                        }));
                    }
                }
            }
        }
    }
    out
}

/// A bubble asking the user to approve a proposed command.
///
/// It carries the proposed call, not just its text, so Gemini's per-call
/// `thoughtSignature` survives the wait for an answer: the approve paths
/// rebuild the exchange from this bubble, and a signature lost here would
/// 400 the request AFTER the command already ran. `build_provider_messages`
/// skips `PendingTool` entirely, so nothing here reaches a request until the
/// command actually executes.
fn pending_tool_bubble(command: String, thought_signature: Option<String>) -> ChatMessage {
    let mut bubble = ChatMessage::text(ChatRole::PendingTool, command.clone());
    bubble.tool = Some(crate::state::ToolExchange {
        // The real id is minted when the command runs; this bubble is only
        // ever read for its command + signature.
        id: String::new(),
        command,
        risk: "risky".into(),
        output: None,
        thought_signature,
    });
    bubble
}

/// Flags that decide how a proposed AI tool call is gated. Pulled out of
/// `Oryxis` state so the decision is a pure function (see
/// [`classify_tool_gate`]) and can be unit-tested.
struct ToolGateInput {
    /// The tab's chat mode (`Plan` / `Ask` / `Auto`), the top-level branch.
    mode: crate::state::ChatMode,
    /// First token is on the tab's "always run" allow-list.
    allowed: bool,
    /// Command chains / pipes / redirects / substitutes (e.g. `ls; rm`).
    has_chaining: bool,
    /// The model self-classified the command as `safe`.
    risk_safe: bool,
    /// Command matches the deterministic catastrophic-command floor.
    obviously_destructive: bool,
    /// The per-turn auto-run streak has reached `CHAT_AUTO_RUN_STREAK_MAX`.
    streak_exceeded: bool,
    /// This exact command was already auto-run earlier in the turn.
    already_auto_ran: bool,
}

/// What to do with a proposed tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolGate {
    /// Run it immediately, no prompt.
    AutoExec,
    /// Surface it for explicit user approval (loop guard or destructive floor).
    Confirm,
    /// Hand to the independent auto-exec judge.
    Judge,
    /// Queue a pending-tool bubble (risky / unclassified).
    Prompt,
}

/// Decide how a proposed tool call is gated, given the tab's chat mode.
///
/// The allow-list bypass runs first in every mode: a command whose exact
/// name the user marked "always run" is honored (still subject to the loop
/// backstop) whether the tab is in Plan, Ask, or Auto, because that decision
/// was an explicit per-command grant. After that the mode decides:
///
/// - **Ask**: nothing runs unattended; every non-allow-listed command is a
///   `Prompt`.
/// - **Plan**: read-only investigation may auto-run (same floor + judge path
///   as Auto), but any write / destructive / unclassified command is
///   surfaced (`Prompt`) instead of executed, so the session stays read-only
///   by default. The user can still click RUN on a surfaced write.
/// - **Auto**: the full pipeline (destructive floor + loop guards collapse
///   to `Confirm`, model-claimed safe goes to the `Judge`, everything else
///   `Prompt`).
fn classify_tool_gate(i: ToolGateInput) -> ToolGate {
    use crate::state::ChatMode;
    // Allow-listed simple command: runs unattended in any mode, but even a
    // trusted command can't loop forever, so the streak backstop still
    // applies. Exact-repeat is deliberately NOT applied to allow-listed
    // commands (the user may legitimately want `ls` run more than once).
    if i.allowed && !i.has_chaining {
        return if i.streak_exceeded {
            ToolGate::Confirm
        } else {
            ToolGate::AutoExec
        };
    }
    match i.mode {
        // Every command needs an explicit click.
        ChatMode::Ask => ToolGate::Prompt,
        // Read-only auto-runs (through the same guards as Auto); anything
        // that isn't cleanly read-only is surfaced rather than executed.
        ChatMode::Plan => {
            if i.risk_safe
                && !i.obviously_destructive
                && !i.streak_exceeded
                && !i.already_auto_ran
            {
                ToolGate::Judge
            } else {
                ToolGate::Prompt
            }
        }
        ChatMode::Auto => {
            // Deterministic catastrophic-command floor: always prompt.
            if i.risk_safe && i.obviously_destructive {
                return ToolGate::Confirm;
            }
            // Loop guards on the judge path: a repeated command or an
            // over-long streak is refused auto-exec and surfaced instead.
            // This is what breaks the runaway loop with no user action.
            if i.risk_safe && (i.streak_exceeded || i.already_auto_ran) {
                return ToolGate::Confirm;
            }
            // Model-claimed safe and nothing objected: let the judge decide.
            if i.risk_safe {
                return ToolGate::Judge;
            }
            // Risky or unclassified: explicit prompt.
            ToolGate::Prompt
        }
    }
}

impl Oryxis {
    /// Index of the tab with this id, if it still exists. The chat pipeline
    /// routes by id (not `active_tab`) so switching or closing tabs
    /// mid-stream can never land a delta or a command on the wrong session.
    fn chat_tab_index(&self, id: Uuid) -> Option<usize> {
        self.tabs.iter().position(|t| t._id == id)
    }

    /// Abort the in-flight chat stream on the tab with `id` (if any) and
    /// forget its handle. Aborting the iced stream drops the receiver
    /// feeding it, which makes the detached tool-followup task's `tx.send`
    /// fail so it stops too. No-op if the tab is gone or idle.
    pub(crate) fn abort_chat_task_for(&mut self, id: Uuid) {
        if let Some(idx) = self.chat_tab_index(id)
            && let Some(handle) = self.tabs[idx].chat_task.take()
        {
            handle.abort();
        }
    }

    /// Abort the active tab's chat stream and clear its loading flag. The
    /// user-gesture stop path (Stop button, closing the sidebar, reset):
    /// those always act on the tab the user is looking at.
    pub(crate) fn abort_active_chat_task(&mut self) {
        if let Some(idx) = self.cur_active_tab()
            && let Some(tab) = self.tabs.get_mut(idx)
        {
            if let Some(handle) = tab.chat_task.take() {
                handle.abort();
            }
            tab.chat_loading = false;
        }
    }

    /// Abort every tab's chat stream and clear the loading flags. The
    /// no-UI-left paths: hiding the Chat tab's placement or disabling AI
    /// takes the Stop / Reset affordances away from EVERY terminal tab at
    /// once, so a loop left running on a background tab would have no
    /// reachable stop control at all.
    pub(crate) fn abort_all_chat_tasks(&mut self) {
        for tab in &mut self.tabs {
            if let Some(handle) = tab.chat_task.take() {
                handle.abort();
            }
            tab.chat_loading = false;
        }
    }

    /// Replace a tab's tracked chat task: abort whatever was running on it,
    /// make the new task abortable, store its handle on the tab, and return
    /// the wrapped task to hand back to iced. Funnel every chat-stream /
    /// judge task through this (keyed by origin tab id) so a single Stop /
    /// close / reset cancels the right conversation's live work, and a chat
    /// on one tab never cancels another's. Passthrough if the tab is gone.
    fn track_chat_task_for(&mut self, id: Uuid, task: Task<Message>) -> Task<Message> {
        self.abort_chat_task_for(id);
        let (task, handle) = task.abortable();
        if let Some(idx) = self.chat_tab_index(id) {
            self.tabs[idx].chat_task = Some(handle);
        }
        task
    }

    /// Clear the per-turn auto-exec guard state on the active tab. Called
    /// whenever the user retakes control (sends a message, resets, or
    /// explicitly approves a command) so a fresh turn starts with a clean
    /// streak and repeat history.
    fn reset_chat_auto_run_guard(&mut self) {
        if let Some(idx) = self.cur_active_tab()
            && let Some(tab) = self.tabs.get_mut(idx)
        {
            tab.chat_auto_run_history.clear();
            tab.chat_auto_run_streak = 0;
        }
    }

    /// Spawn a fresh assistant stream for tab `idx` over its CURRENT chat
    /// history, marking the tab loading, pushing the empty assistant
    /// placeholder the deltas land in, and returning the tracked stream task.
    /// Shared by `SendChat` (after it pushes the user message) and `ChatRetry`
    /// (which resumes over the existing history, including tool-output System
    /// bubbles, without needing a trailing user message). Injects the current
    /// terminal output (redacted under Privacy Mode) as leading context.
    fn spawn_chat_stream_for(&mut self, idx: usize) -> Task<Message> {
        // Resolve everything that needs an immutable borrow of `self` up
        // front (tab id for routing, terminal handle, Privacy-Mode inputs,
        // provider config) so the mutable history push doesn't collide.
        let tab_id = self.tabs[idx]._id;
        let terminal = Arc::clone(&self.tabs[idx].active().terminal);
        let pane_label = self.tabs[idx].active().label.clone();
        let privacy_terms = self
            .privacy_active_for_label(&pane_label)
            .then(|| self.privacy_terms());
        let api_key = self
            .vault
            .as_ref()
            .and_then(|v| v.get_ai_api_key().ok().flatten())
            .unwrap_or_default();
        let extra_prompt = self
            .vault
            .as_ref()
            .and_then(|v| v.get_setting("ai_system_prompt").ok().flatten());
        let config = crate::ai::AiConfig {
            provider: self.ai.provider.clone(),
            model: self.ai.model.clone(),
            api_key,
            api_url: if self.ai.api_url.is_empty() {
                None
            } else {
                Some(self.ai.api_url.clone())
            },
            system_prompt: extra_prompt,
            reasoning: self.ai.reasoning,
        };

        // Snapshot the last ~50 lines of terminal output for context,
        // redacted when Privacy Mode is active for this pane.
        let terminal_context = capture_terminal_context(
            &terminal,
            50,
            privacy_terms.as_deref(),
            self.privacy_classes(),
        );

        let tab = &mut self.tabs[idx];
        tab.chat_loading = true;

        // Build messages: inject current terminal output as a leading user
        // turn, then the reconstructed history (with native tool blocks).
        let mut messages: Vec<crate::ai::ChatMsg> = Vec::new();
        if !terminal_context.is_empty() {
            messages.push(crate::ai::ChatMsg::text(
                "user",
                format!(
                    "[Current terminal output (last ~50 lines)]\n```\n{}\n```",
                    terminal_context
                ),
            ));
            messages.push(crate::ai::ChatMsg::text(
                "assistant",
                "I can see the terminal output. How can I help?",
            ));
        }
        messages.extend(build_provider_messages(&tab.chat_history));

        // Insert an empty assistant placeholder so streamed text deltas have
        // a bubble to land in.
        tab.chat_history
            .push(ChatMessage::text(ChatRole::Assistant, String::new()));

        let stream_task = Task::stream(crate::ai::send_chat_stream(config, messages)).map(
            move |chunk| match chunk {
                crate::ai::StreamChunk::Text(delta) => Message::Ai(AiMessage::ChatStreamChunk { tab_id, delta }),
                crate::ai::StreamChunk::Reasoning(delta) => {
                    Message::Ai(AiMessage::ChatStreamReasoning { tab_id, delta })
                }
                crate::ai::StreamChunk::ToolUse { command, risk, thought_signature } => {
                    Message::Ai(AiMessage::ChatToolProposed {
                        tab_id,
                        command,
                        risk,
                        thought_signature,
                    })
                }
                crate::ai::StreamChunk::Done => Message::Ai(AiMessage::ChatStreamDone { tab_id }),
                crate::ai::StreamChunk::Error(error) => Message::Ai(AiMessage::ChatError { tab_id, error }),
            },
        );
        // Track the stream so Stop / close / reset can abort it.
        self.track_chat_task_for(tab_id, stream_task)
    }

    /// Route one AI message to the phase that owns it.
    ///
    /// Was a 939-line `match`. The groups are the life of a
    /// conversation, not a naming accident: configure the provider, open
    /// the panel, talk, and (the half with teeth) run a tool.
    pub(crate) fn handle_ai(&mut self, message: AiMessage) -> Task<Message> {
        match message {
            m @ (
                AiMessage::ToggleAiEnabled
                | AiMessage::ToggleAiSaveHistory
                | AiMessage::ToggleAiReasoning
                | AiMessage::AiProviderChanged(..)
                | AiMessage::AiModelChanged(..)
                | AiMessage::AiApiKeyChanged(..)
                | AiMessage::AiApiUrlChanged(..)
                | AiMessage::AiSystemPromptAction(..)
                | AiMessage::SaveAiApiKey
            ) => self.handle_ai_config(m),
            m @ (
                AiMessage::ToggleSidebarRegion(..)
                | AiMessage::SelectTerminalSidebarTab(..)
                | AiMessage::SidebarSnippetSearchChanged(..)
                | AiMessage::HostsTreeToggleGroup(..)
                | AiMessage::HostsTreeSearchChanged(..)
                | AiMessage::ToggleSidebarSort
                | AiMessage::ToggleSidebarSearch
                | AiMessage::ChatSidebarResizeStart(..)
                | AiMessage::ChatSidebarResizeStop
            ) => self.handle_ai_sidebar(m),
            m @ (
                AiMessage::ChatInputAction(..)
                | AiMessage::ChatScrolled(..)
                | AiMessage::ChatResetConversation
                | AiMessage::ChatModeChanged(..)
                | AiMessage::SendChat
                | AiMessage::ChatStreamChunk { .. }
                | AiMessage::ChatStreamReasoning { .. }
                | AiMessage::ChatStreamDone { .. }
                | AiMessage::ChatStop
                | AiMessage::ChatError { .. }
                | AiMessage::ChatRetry
            ) => self.handle_ai_conversation(m),
            m @ (
                AiMessage::ChatToolProposed { .. }
                | AiMessage::ChatToolGuardBlocked { .. }
                | AiMessage::ChatToolApprove(..)
                | AiMessage::ChatToolApproveAlways(..)
                | AiMessage::ChatToolDeny(..)
                | AiMessage::ChatToolExec { .. }
                | AiMessage::ChatToolResult { .. }
            ) => self.handle_ai_tools(m),
        }
    }
}

mod config;
mod conversation;
mod sidebar;
mod tools;

#[cfg(test)]
mod tests {
    use super::{capture_command_output, classify_tool_gate, ToolGate, ToolGateInput};
    use crate::state::ChatMode;

    /// What a freshly-connected pane holds before the assistant runs
    /// anything: a login banner, then earlier commands and their output. None
    /// of it is a tool result, and all of it is what the undelimited tail used
    /// to hand the model.
    const BANNER: &[u8] = b"Welcome to Ubuntu 24.04.2 LTS (GNU/Linux 6.8.0-124-generic x86_64)\r\n\
        \r\n System load: 0.14   Processes: 326\r\n\
        admin@db:~$ ls\r\nwget-log\r\nadmin@db:~$ ";

    /// Drive a terminal to the state a pane is in when the assistant writes a
    /// command: banner + prompt on screen, anchor taken, then `echoed` (the
    /// shell's echo of the command and whatever followed it) processed.
    fn capture_after(
        echoed: &[u8],
    ) -> super::CommandCapture {
        let mut term = oryxis_terminal::TerminalState::new_no_pty(80, 24).unwrap();
        term.process(BANNER);
        let anchor = term
            .abs_cursor_logical_start()
            .expect("the prompt row is on the primary screen");
        term.process(echoed);
        let term = std::sync::Mutex::new(term);
        capture_command_output(&term, anchor, None, Default::default())
            .expect("the anchor is still readable")
    }

    /// The bug behind the "it opens a new connection every time" report: a
    /// command that has printed nothing yet must capture nothing, never the
    /// screen it was typed on.
    #[test]
    fn a_silent_command_captures_no_prior_screen() {
        let cap = capture_after(b"find / -name \"wp-config.php\" 2>/dev/null\r\n");
        assert!(
            !cap.text.contains("Welcome to Ubuntu"),
            "the login banner is not this command's output: {:?}",
            cap.text
        );
        assert!(!cap.text.contains("wget-log"), "nor is the previous command's: {:?}", cap.text);
        assert!(cap.text.contains("find / -name"), "the echo delimits the region: {:?}", cap.text);
        // And nothing here says the command is over, so the poll keeps
        // waiting instead of reporting an empty answer as the result.
        assert!(!cap.finished);
        assert!(!cap.has_output);
    }

    /// The shell's next prompt is what proves the command ended.
    #[test]
    fn a_prompt_under_the_output_ends_the_capture() {
        let cap = capture_after(
            b"find / -name \"wp-config.php\" 2>/dev/null\r\n\
              /dados/ecp.org.br/wp-config.php\r\nadmin@db:~$ ",
        );
        assert!(cap.finished);
        assert!(cap.has_output);
        assert!(cap.text.contains("/dados/ecp.org.br/wp-config.php"));
        assert!(!cap.text.contains("Welcome to Ubuntu"));
    }

    /// A command that ran and printed nothing has to SAY so: an empty tool
    /// result reads as a failure, or gets dropped by the provider.
    #[test]
    fn a_finished_command_with_no_output_says_so() {
        let cap = capture_after(b"touch /tmp/x\r\nadmin@db:~$ ");
        assert!(cap.finished);
        assert!(!cap.has_output);
        assert!(cap.text.contains("[command completed with no output]"), "{:?}", cap.text);
    }

    /// Same as [`capture_after`], for a shell whose prompt carries no marker
    /// this side recognizes (fish, or any custom `PS1`).
    fn capture_after_under_prompt(prompt: &[u8], echoed: &[u8]) -> super::CommandCapture {
        let mut term = oryxis_terminal::TerminalState::new_no_pty(80, 24).unwrap();
        term.process(prompt);
        let anchor = term
            .abs_cursor_logical_start()
            .expect("the prompt row is on the primary screen");
        term.process(echoed);
        let term = std::sync::Mutex::new(term);
        capture_command_output(&term, anchor, None, Default::default())
            .expect("the anchor is still readable")
    }

    /// A line of OUTPUT that looks like a prompt is not one. `cat`ing a
    /// script that ends in a comment used to end the capture 800 ms later,
    /// so anything the command printed after a pause was reported as
    /// though it had never been printed.
    #[test]
    fn a_marker_inside_the_output_does_not_end_the_capture() {
        let cap = capture_after(b"cat setup.sh\r\nset -e\r\n# end of file\r\n");
        assert!(!cap.finished, "no shell prompt has been drawn yet: {:?}", cap.text);
        assert!(cap.has_output);
        assert!(cap.text.contains("# end of file"), "and the line is still captured");
    }

    /// The `% ` in a `df` row, and the `$ ` in a grepped shell script, are
    /// the same trap one column over.
    #[test]
    fn a_marker_mid_line_does_not_end_the_capture_either() {
        let cap = capture_after(b"df -h\r\n/dev/sda1  47G  19G  26G  41% /run/user\r\n");
        assert!(!cap.finished, "{:?}", cap.text);
    }

    /// The cursor is what actually says the shell is waiting: fish's
    /// `wilson@box ~> ` matches no marker, and before this the poll waited
    /// out a full silence window on every command and never added the
    /// no-output note.
    #[test]
    fn an_unrecognized_prompt_still_ends_the_capture() {
        let cap = capture_after_under_prompt(
            b"wilson@box ~> ",
            b"touch /tmp/x\r\nwilson@box ~> ",
        );
        assert!(cap.finished, "the cursor is parked on the prompt: {:?}", cap.text);
        assert!(!cap.has_output);
        assert!(cap.text.contains("[command completed with no output]"), "{:?}", cap.text);
    }

    /// The cursor is necessary, not sufficient: a command that writes a
    /// partial line and keeps working leaves the cursor sitting on it, and
    /// declaring that finished would report a progress bar as the answer.
    #[test]
    fn a_partial_line_under_the_cursor_is_not_a_prompt() {
        let cap = capture_after(b"./deploy.sh\r\nWorking...");
        assert!(!cap.finished, "{:?}", cap.text);
        assert!(cap.has_output);
    }

    /// A command left syntactically incomplete parks the cursor on bash's
    /// `> ` continuation prompt. The shell is waiting for the rest of the
    /// command, so reporting "completed with no output" would be a lie about
    /// the one state where the model most needs the truth.
    #[test]
    fn a_continuation_prompt_is_not_a_finished_command() {
        let cap = capture_after(b"awk '{print $1}\r\n> ");
        assert!(!cap.finished, "{:?}", cap.text);
        assert!(
            !cap.text.contains("[command completed with no output]"),
            "{:?}",
            cap.text
        );
    }

    /// oh-my-zsh's default prompt ends on the PATH, not on its `➜` marker,
    /// so it is the same case as fish from this side.
    #[test]
    fn a_prompt_whose_marker_is_not_last_still_ends_the_capture() {
        let cap = capture_after_under_prompt(
            "\u{279c}  ~ ".as_bytes(),
            "whoami\r\nadmin\r\n\u{279c}  ~ ".as_bytes(),
        );
        assert!(cap.finished, "{:?}", cap.text);
        assert!(cap.has_output);
    }

    /// Convenience builder: Auto mode, everything else off (a plain
    /// risky/unclassified call). Individual tests override the fields they
    /// exercise via struct-update syntax.
    fn input() -> ToolGateInput {
        ToolGateInput {
            mode: ChatMode::Auto,
            allowed: false,
            has_chaining: false,
            risk_safe: false,
            obviously_destructive: false,
            streak_exceeded: false,
            already_auto_ran: false,
        }
    }

    #[test]
    fn unclassified_command_prompts() {
        assert_eq!(classify_tool_gate(input()), ToolGate::Prompt);
    }

    #[test]
    fn safe_command_goes_to_judge() {
        let gate = classify_tool_gate(ToolGateInput {
            risk_safe: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Judge);
    }

    #[test]
    fn allow_listed_simple_command_auto_execs() {
        let gate = classify_tool_gate(ToolGateInput {
            allowed: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::AutoExec);
    }

    #[test]
    fn allow_listed_chained_command_falls_through_to_prompt() {
        // A trusted first token can't smuggle a chained command past the gate.
        let gate = classify_tool_gate(ToolGateInput {
            allowed: true,
            has_chaining: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Prompt);
    }

    #[test]
    fn repeated_safe_command_is_refused_auto_exec() {
        // The reported bug: the model re-proposing `docker --version`. A
        // safe command already auto-run this turn must be surfaced, not
        // auto-run again, so the loop can't continue unattended.
        let gate = classify_tool_gate(ToolGateInput {
            risk_safe: true,
            already_auto_ran: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Confirm);
    }

    #[test]
    fn over_long_streak_is_refused_even_when_safe() {
        // Backstop for a run of *different* safe commands.
        let gate = classify_tool_gate(ToolGateInput {
            risk_safe: true,
            streak_exceeded: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Confirm);
    }

    #[test]
    fn streak_cap_also_blocks_allow_listed_commands() {
        // A loop of an always-run command is still a loop.
        let gate = classify_tool_gate(ToolGateInput {
            allowed: true,
            streak_exceeded: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Confirm);
    }

    #[test]
    fn allow_listed_repeat_without_streak_still_auto_execs() {
        // Exact-repeat is NOT applied to allow-listed commands: the user
        // may legitimately want `ls` run more than once in a turn.
        let gate = classify_tool_gate(ToolGateInput {
            allowed: true,
            already_auto_ran: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::AutoExec);
    }

    #[test]
    fn destructive_safe_command_is_refused_before_judge() {
        // The deterministic floor fires regardless of the judge.
        let gate = classify_tool_gate(ToolGateInput {
            risk_safe: true,
            obviously_destructive: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Confirm);
    }

    // ── Mode branches ──

    #[test]
    fn ask_mode_prompts_even_for_safe_commands() {
        // Ask never auto-runs; a model-claimed safe command still needs a
        // click instead of going to the judge.
        let gate = classify_tool_gate(ToolGateInput {
            mode: ChatMode::Ask,
            risk_safe: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Prompt);
    }

    #[test]
    fn ask_mode_still_honors_allow_list() {
        // An explicit "always run X" grant applies in every mode.
        let gate = classify_tool_gate(ToolGateInput {
            mode: ChatMode::Ask,
            allowed: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::AutoExec);
    }

    #[test]
    fn plan_mode_lets_read_only_reach_the_judge() {
        // Plan allows investigation: a safe, non-destructive command goes to
        // the judge just like Auto.
        let gate = classify_tool_gate(ToolGateInput {
            mode: ChatMode::Plan,
            risk_safe: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Judge);
    }

    #[test]
    fn plan_mode_surfaces_writes_instead_of_running_them() {
        // A risky / unclassified (write) command is surfaced, never
        // auto-run, so a Plan session stays read-only by default.
        let gate = classify_tool_gate(ToolGateInput {
            mode: ChatMode::Plan,
            risk_safe: false,
            ..input()
        });
        assert_eq!(gate, ToolGate::Prompt);
    }

    #[test]
    fn plan_mode_surfaces_destructive_safe_commands() {
        // Even a model-claimed-safe command that trips the destructive floor
        // is surfaced under Plan, not sent to the judge.
        let gate = classify_tool_gate(ToolGateInput {
            mode: ChatMode::Plan,
            risk_safe: true,
            obviously_destructive: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Prompt);
    }

    // ── Message reconstruction (build_provider_messages) ──

    use super::build_provider_messages;
    use crate::state::{ChatMessage, ChatRole, ToolExchange};

    fn tool_msg(id: &str, command: &str, risk: &str, output: Option<&str>) -> ChatMessage {
        ChatMessage {
            role: ChatRole::Tool,
            content: format!("$ {command}"),
            parsed_md: Vec::new(),
            reasoning: String::new(),
            tool: Some(ToolExchange {
                id: id.into(),
                command: command.into(),
                risk: risk.into(),
                output: output.map(str::to_string),
                thought_signature: None,
            }),
        }
    }

    #[test]
    fn completed_tool_merges_preamble_then_emits_result() {
        // Assistant preamble + completed tool → ONE assistant turn carrying
        // [text, tool_use], then a `tool` turn with the matching result.
        let history = vec![
            ChatMessage::text(ChatRole::Assistant, "Let me check."),
            tool_msg("t1", "df -h", "safe", Some("Filesystem ... 40% /")),
        ];
        let out = build_provider_messages(&history);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "assistant");
        assert_eq!(out[0].content.as_str(), Some("Let me check."));
        let tu = out[0].tool_use.as_ref().expect("tool_use on assistant turn");
        assert_eq!(tu.id, "t1");
        assert_eq!(tu.command, "df -h");
        assert_eq!(tu.risk, "safe");
        assert_eq!(out[1].role, "tool");
        let tr = out[1].tool_result.as_ref().expect("tool_result turn");
        assert_eq!(tr.id, "t1"); // pairs with the tool_use id
        assert_eq!(tr.output, "Filesystem ... 40% /");
    }

    #[test]
    fn completed_tool_without_preamble_is_standalone() {
        // No assistant text before the tool → a standalone assistant turn
        // carrying just the tool_use (empty text), then the result.
        let history = vec![
            ChatMessage::text(ChatRole::User, "how much disk is free?"),
            tool_msg("t2", "df -h", "safe", Some("ok")),
        ];
        let out = build_provider_messages(&history);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].role, "user");
        assert_eq!(out[1].role, "assistant");
        assert_eq!(out[1].content.as_str(), Some(""));
        assert!(out[1].tool_use.is_some());
        assert_eq!(out[2].role, "tool");
    }

    #[test]
    fn running_tool_is_flat_never_a_dangling_tool_use() {
        // A still-running exchange (output None) must NOT produce a tool_use
        // block, or the next request would 400 on a dangling tool_use.
        let history = vec![
            ChatMessage::text(ChatRole::Assistant, "Checking."),
            tool_msg("t3", "sleep 30", "safe", None),
        ];
        let out = build_provider_messages(&history);
        assert!(out.iter().all(|m| m.tool_use.is_none() && m.tool_result.is_none()));
        assert_eq!(out.last().unwrap().role, "user");
        assert_eq!(
            out.last().unwrap().content.as_str(),
            Some("[Running command: sleep 30]")
        );
    }

    #[test]
    fn deny_and_error_and_pending_leave_no_tool_use() {
        // The not-run paths stay flat: a declined command is a System note, and
        // Error / PendingTool bubbles are dropped entirely. No tool_use leaks.
        let history = vec![
            ChatMessage::text(ChatRole::Assistant, "I could run this."),
            ChatMessage::text(ChatRole::PendingTool, "rm -rf /"),
            ChatMessage::text(ChatRole::System, "[user declined to run: rm -rf /]"),
            ChatMessage::text(ChatRole::Error, "network blip"),
            ChatMessage::text(ChatRole::User, "never mind"),
        ];
        let out = build_provider_messages(&history);
        assert!(out.iter().all(|m| m.tool_use.is_none() && m.tool_result.is_none()));
        // assistant text + declined-note(as user) + user follow-up; the
        // PendingTool and Error bubbles are gone.
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].role, "assistant");
        assert_eq!(out[1].role, "user");
        assert_eq!(out[2].role, "user");
    }

    #[test]
    fn two_tools_back_to_back_stay_paired() {
        // A second tool right after a result (no text between) gets its own
        // standalone assistant turn: every tool_use has exactly one following
        // tool_result, so the sequence is valid.
        let history = vec![
            ChatMessage::text(ChatRole::Assistant, "First."),
            tool_msg("a", "ls", "safe", Some("file1")),
            tool_msg("b", "pwd", "safe", Some("/home")),
        ];
        let out = build_provider_messages(&history);
        // assistant[First,tool a] / tool a-result / assistant[tool b] / tool b-result
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].tool_use.as_ref().unwrap().id, "a");
        assert_eq!(out[1].tool_result.as_ref().unwrap().id, "a");
        assert!(out[2].tool_use.is_some() && out[2].content.as_str() == Some(""));
        assert_eq!(out[2].tool_use.as_ref().unwrap().id, "b");
        assert_eq!(out[3].tool_result.as_ref().unwrap().id, "b");
    }
}
