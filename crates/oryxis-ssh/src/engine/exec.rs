use super::*;

impl SshEngine {

    /// Execute a command without PTY (non-interactive) and return the output.
    /// `timeout` bounds the whole exchange, the channel open and the exec
    /// request included: a connection that authenticated and then stops
    /// answering is exactly the one that would otherwise wait forever
    /// on the open.
    pub async fn exec_command(
        &self,
        handle: SshHandle,
        command: &str,
        timeout: std::time::Duration,
    ) -> Result<ExecResult, SshError> {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code: Option<u32> = None;

        let collect = async {
            let channel = handle.0.channel_open_session().await
                .map_err(|e| SshError::Channel(format!("open session: {}", e)))?;

            channel.exec(true, command).await
                .map_err(|e| SshError::Channel(format!("exec: {}", e)))?;

            let mut channel = channel;
            // Read until channel close (`None`), not just Eof, some
            // servers send `ExitStatus` after `Eof`, so breaking early
            // would leave us defaulting to 255.
            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Data { data }) => stdout.extend_from_slice(&data),
                    Some(ChannelMsg::ExtendedData { data, ext: 1 }) => {
                        stderr.extend_from_slice(&data);
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = Some(exit_status);
                    }
                    None => break,
                    _ => {}
                }
            }
            Ok::<(), SshError>(())
        };

        match tokio::time::timeout(timeout, collect).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(SshError::ExecTimeout(timeout.as_secs()));
            }
        }

        Ok(ExecResult {
            exit_code: exit_code.unwrap_or(255),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    /// Open another PTY session on a connection that is ALREADY
    /// authenticated (F2 reuse).
    ///
    /// This is the whole point of splitting `SshTransport` out: a
    /// second tab to a host costs one channel instead of a TCP
    /// handshake, a key exchange, an authentication and, on a jump
    /// chain, all of that again per hop. It also cannot prompt for a
    /// host key or a second factor, because the connection it rides was
    /// already verified and authenticated.
    ///
    /// The engine's own settings (terminal type, env vars, agent and
    /// X11 forwarding) apply to the NEW channel, so the caller should
    /// build the engine for the host exactly as it would for a fresh
    /// dial. Anything negotiated per CONNECTION (algorithms, keepalive,
    /// the jump chain, the proxy) belongs to the transport and is
    /// whatever the original dial agreed on.
    ///
    /// Any failure here is the caller's cue to dial fresh rather than
    /// to show an error: a server at its `MaxSessions` cap and a
    /// half-dead connection both surface as a channel-open error, and
    /// neither is something to bother the user with when a new
    /// connection would just work.
    pub async fn open_session_on(
        &self,
        transport: Arc<super::SshTransport>,
        cols: u32,
        rows: u32,
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
        if !transport.looks_healthy() {
            return Err(SshError::Channel(
                "connection is not answering; dial a fresh one".into(),
            ));
        }
        // No port-forward listeners: those are bound once per
        // CONNECTION by the dial that created it, and binding them
        // again for a second tab would fight over the same local ports.
        self.open_pty_session(transport, cols, rows, Vec::new()).await
    }

    pub(crate) async fn open_pty_session(
        &self,
        transport: Arc<super::SshTransport>,
        cols: u32,
        rows: u32,
        pf_listeners: Vec<(PortForward, tokio::net::TcpListener)>,
    ) -> Result<(SshSession, mpsc::UnboundedReceiver<Vec<u8>>), SshError> {
        // Open session channel. Nothing here holds the connection: the
        // other sessions and forwards riding it open their own channels
        // meanwhile.
        let handle = transport.handle();
        let channel = handle.channel_open_session().await
            .map_err(|e| SshError::Channel(format!("Failed to open session channel: {}", e)))?;

        // Request PTY. A custom TERM is first checked against the
        // host's own terminfo db (issue #88: e.g. CentOS 7 has no
        // `tmux-256color`, which leaves vim/nano unusable) and swapped
        // for the nearest present entry when missing; the switch is
        // recorded on the session so the UI can tell the user. The
        // default TERM is never probed, keeping the common path free.
        let requested = self
            .terminal_type
            .as_deref()
            .unwrap_or(DEFAULT_TERMINAL_TYPE);
        let mut term_fallback: Option<TermFallback> = None;
        let mut term = requested.to_string();
        if requested != DEFAULT_TERMINAL_TYPE {
            match self.probe_terminfo(handle, requested).await {
                TermProbe::Fallback(used) => {
                    tracing::warn!(
                        "host lacks terminfo for {requested}, requesting PTY with {used}"
                    );
                    term = used.clone();
                    term_fallback = Some(TermFallback {
                        requested: requested.to_string(),
                        used: Some(used),
                    });
                }
                TermProbe::MissingNoFallback => {
                    tracing::warn!(
                        "host lacks terminfo for {requested} and every fallback candidate; keeping it"
                    );
                    term_fallback = Some(TermFallback {
                        requested: requested.to_string(),
                        used: None,
                    });
                }
                TermProbe::Present | TermProbe::Inconclusive => {}
            }
        }
        channel
            .request_pty(false, &term, cols, rows, 0, 0, &[])
            .await
            .map_err(|e| SshError::Channel(format!("PTY request failed: {}", e)))?;

        // Optional ssh-agent forwarding. Must fire BEFORE `request_shell`
        //, sshd reads the channel requests in order and only sets
        // `SSH_AUTH_SOCK` on the launched process if forwarding was
        // already requested when the shell starts. Issued without
        // `want_reply`; failures (server has `AllowAgentForwarding no`)
        // are not fatal, the user still gets a normal shell, they
        // just can't hop further with their local keys.
        if self.agent_forwarding
            && let Err(e) = channel.agent_forward(false).await
        {
            tracing::warn!("agent_forward request failed (non-fatal): {}", e);
        }

        // Optional X11 forwarding, under the same ordering rule as
        // `agent_forward`: sshd only exports `DISPLAY` into the launched
        // process if the request arrived before the shell started.
        //
        // The cookie goes out as lower-case HEX, and it is the FAKE one
        // (see `x11::spoof`) so the real display cookie never lands in
        // the remote `.Xauthority`. Non-fatal, a server with
        // `X11Forwarding no` should still yield a working shell.
        if let Some(x11) = &self.x11 {
            let (proto, cookie) = x11.request_args();
            // `single_connection = false`: a desktop session opens one
            // X11 channel per client, not one per session.
            //
            // Sent WITHOUT `want_reply`, like `agent_forward`: awaiting
            // the reply here would mean reading the channel before
            // `request_shell`, and a server that never answers would
            // hang the connect. The cost is that a server-side refusal
            // (`X11Forwarding no`, missing `xauth` binary) is silent,
            // and the user only sees "cannot open display" on the
            // remote. This log is therefore the only local evidence
            // that the request went out at all.
            tracing::info!("requesting X11 forwarding for {}", x11.describe());
            if let Err(e) = channel
                .request_x11(false, false, proto, cookie, x11.screen)
                .await
            {
                tracing::warn!("x11-req failed (non-fatal): {}", e);
            }
        }

        // Per-host environment variables. Sent before `request_shell` so
        // the server can apply them to the launched process. Non-fatal:
        // most `sshd` reject anything outside `AcceptEnv` (LC_*/LANG_* by
        // default), and we'd rather give the user a shell than abort.
        for (name, value) in &self.env_vars {
            if let Err(e) = channel.set_env(false, name.clone(), value.clone()).await {
                tracing::warn!("set_env {} failed (non-fatal): {}", name, e);
            }
        }

        // Request shell
        channel.request_shell(false).await
            .map_err(|e| SshError::Channel(format!("Shell request failed: {}", e)))?;

        // I/O bridging
        let (output_tx, output_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (resize_tx, mut resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();

        // Resolve the per-host charset once. `None` (or UTF-8) means the
        // byte stream is forwarded untouched; any other charset is decoded
        // to UTF-8 inbound and encoded back outbound for the terminal.
        let enc: Option<&'static encoding_rs::Encoding> = self
            .encoding
            .as_deref()
            .and_then(|n| encoding_rs::Encoding::for_label(n.as_bytes()))
            .filter(|e| *e != encoding_rs::UTF_8);

        let mut channel_writer = channel.make_writer();

        // Published by the reader on its way out, before the output
        // sender is dropped, so the session reads as dead by the time
        // the app notices the stream ended. See `SshSession::is_alive`.
        let reader_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader_flag = std::sync::Arc::clone(&reader_done);

        // Reader task, multiplexes incoming PTY data with outgoing
        // window-change requests so we only own `channel` in one place.
        let reader_task = tokio::spawn(async move {
            let mut channel = channel;
            // Stateful decoder so a multi-byte char split across two reads
            // still decodes correctly. `None` for UTF-8 (passthrough).
            let mut decoder = enc.map(|e| e.new_decoder());
            // Cap on one forwarded message. Data messages already queued on
            // the channel are folded into a single send so the UI runs one
            // update+view+draw cycle per batch instead of one per SSH packet.
            const COALESCE_MAX: usize = 64 * 1024;
            loop {
                tokio::select! {
                    msg = channel.wait() => {
                        // Set when EOF / exit-status arrives mid-batch: the
                        // accumulated bytes are flushed first, then the loop
                        // exits, so no trailing output is dropped.
                        let mut closed = false;
                        let bytes: Option<Vec<u8>> = match msg {
                            Some(ChannelMsg::Data { data }) => Some(data.to_vec()),
                            Some(ChannelMsg::ExtendedData { data, ext: 1 }) => Some(data.to_vec()),
                            Some(ChannelMsg::ExtendedData { .. }) => continue,
                            Some(ChannelMsg::ExitStatus { exit_status }) => {
                                tracing::info!("Remote exited with status {}", exit_status);
                                break;
                            }
                            Some(ChannelMsg::Eof) | None => {
                                tracing::info!("SSH channel closed");
                                break;
                            }
                            _ => continue,
                        };
                        if let Some(mut b) = bytes {
                            // Coalesce: drain messages that are already
                            // queued (zero timeout never waits for new
                            // data, so interactive echo latency is
                            // unchanged) up to the batch cap.
                            while b.len() < COALESCE_MAX {
                                match tokio::time::timeout(
                                    std::time::Duration::ZERO,
                                    channel.wait(),
                                ).await {
                                    Ok(Some(ChannelMsg::Data { data })) => {
                                        b.extend_from_slice(&data);
                                    }
                                    Ok(Some(ChannelMsg::ExtendedData { data, ext: 1 })) => {
                                        b.extend_from_slice(&data);
                                    }
                                    Ok(Some(ChannelMsg::ExtendedData { .. })) => continue,
                                    Ok(Some(ChannelMsg::ExitStatus { exit_status })) => {
                                        tracing::info!(
                                            "Remote exited with status {}", exit_status,
                                        );
                                        closed = true;
                                        break;
                                    }
                                    Ok(Some(ChannelMsg::Eof)) | Ok(None) => {
                                        tracing::info!("SSH channel closed");
                                        closed = true;
                                        break;
                                    }
                                    Ok(Some(_)) => continue,
                                    // Nothing queued right now: flush.
                                    Err(_) => break,
                                }
                            }
                            let out = match &mut decoder {
                                Some(dec) => {
                                    let mut s = String::with_capacity(b.len() + 16);
                                    let _ = dec.decode_to_string(&b, &mut s, false);
                                    s.into_bytes()
                                }
                                None => b,
                            };
                            if output_tx.send(out).is_err() {
                                break;
                            }
                        }
                        if closed {
                            break;
                        }
                    }
                    Some((cols, rows)) = resize_rx.recv() => {
                        if let Err(e) = channel
                            .window_change(cols as u32, rows as u32, 0, 0)
                            .await
                        {
                            tracing::warn!("SSH window-change failed: {}", e);
                        }
                    }
                }
            }
            // Dead BEFORE silent, and in that order on purpose: the app
            // takes the end of this stream as the disconnect notice and
            // asks `is_alive()` before acting on it, so the answer has
            // to be settled first. Same task, no await between, which
            // is what makes it an ordering rather than a race.
            reader_flag.store(true, std::sync::atomic::Ordering::SeqCst);
            drop(output_tx);
        });

        // Writer task
        let writer_task = tokio::spawn(async move {
            while let Some(data) = writer_rx.recv().await {
                // Terminal input arrives as UTF-8; encode it to the host
                // charset when one is set. One-shot per write is fine:
                // keystrokes/pastes arrive as whole UTF-8 chars.
                let data = match enc {
                    Some(e) => {
                        let text = String::from_utf8_lossy(&data);
                        let (encoded, _, _) = e.encode(&text);
                        encoded.into_owned()
                    }
                    None => data,
                };
                if let Err(e) = channel_writer.write_all(&data).await {
                    tracing::error!("SSH write error: {}", e);
                    break;
                }
                if let Err(e) = channel_writer.flush().await {
                    tracing::error!("SSH flush error: {}", e);
                    break;
                }
            }
        });

        // The forward tasks belong to the CONNECTION (bound once per
        // dial, `open_session_on` binds none), so the transport owns
        // them: they outlive this session while any other session or
        // SFTP surface keeps the link, and die with the link itself.
        let pf_tasks = spawn_port_forward_tasks(pf_listeners, transport.handle());
        transport.adopt_port_forwards(pf_tasks);

        Ok((
            SshSession {
                transport,
                writer_tx,
                resize_tx,
                reader_task,
                writer_task,
                closed: std::sync::atomic::AtomicBool::new(false),
                reader_done,
                // Default, overridden by the engine right after this
                // returns via `sftp_open_timeout` assignment.
                sftp_open_timeout: std::time::Duration::from_secs(10),
                term_fallback,
            },
            output_rx,
        ))
    }
}
