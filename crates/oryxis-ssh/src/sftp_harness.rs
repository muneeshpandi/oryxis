//! In-process SSH + SFTP server for tests.
//!
//! Wires a real russh client to a real russh-sftp server over an
//! in-memory `tokio::io::duplex` stream: no TCP, no Docker, no network,
//! so it runs in a plain `cargo test` everywhere including CI. Unlike the
//! in-memory unit fakes, this drives the actual protocol path (handshake,
//! channel, sftp subsystem, request-id multiplexing), so it is the real
//! check that `SftpClient`'s concurrent streaming reassembles correctly
//! against a server, not just against a stub.
//!
//! The server filesystem lives entirely in a `HashMap`; only the handful
//! of operations the streaming paths touch are implemented.

#![cfg(test)]

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use russh::server::{Auth, Msg, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};

use crate::engine::{ClientHandler, SharedHandle};
use crate::sftp::SftpClient;

/// Throwaway ed25519 host key for the in-process server (generated once
/// with `ssh-keygen`, never used anywhere real). Avoids pulling an RNG
/// path into the test.
pub(crate) const HARNESS_HOST_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACBlvoDcBf/w9DbcBLuL2Rj1Lvv7QsEoUz4BIn2EjAQ7tgAAAJDSAIzt0gCM
7QAAAAtzc2gtZWQyNTUxOQAAACBlvoDcBf/w9DbcBLuL2Rj1Lvv7QsEoUz4BIn2EjAQ7tg
AAAEAfVzLcxRas90R8PzqxnURWULsvE8T9Z/naok4PjYsemmW+gNwF//D0NtwEu4vZGPUu
+/tCwShTPgEifYSMBDu2AAAAB2hhcm5lc3MBAgMEBQY=
-----END OPENSSH PRIVATE KEY-----
";

// ---------------------------------------------------------------------------
// In-memory filesystem
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Fs {
    /// Absolute path -> file contents.
    files: HashMap<String, Vec<u8>>,
    /// Open handle id -> the path it refers to.
    handles: HashMap<String, String>,
    next_handle: u64,
}

type SharedFs = Arc<Mutex<Fs>>;

fn ok_status(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".to_string(),
        language_tag: "en-US".to_string(),
    }
}

/// Collapse a POSIX path to one canonical spelling: empty and `.` become
/// root, repeated separators fold, `.` segments drop and `..` pops the
/// segment before it (never above root). What a server's `realpath`
/// answers for a symlink-free tree.
fn normalize_posix(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    if out.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", out.join("/"))
    }
}

// ---------------------------------------------------------------------------
// SFTP subsystem handler (in-memory)
// ---------------------------------------------------------------------------

struct SftpHandler {
    fs: SharedFs,
    version: Option<u32>,
}

impl russh_sftp::server::Handler for SftpHandler {
    type Error = StatusCode;

    fn unimplemented(&self) -> StatusCode {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        version: u32,
        _extensions: HashMap<String, String>,
    ) -> Result<Version, StatusCode> {
        self.version = Some(version);
        Ok(Version::new())
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, StatusCode> {
        // Normalise the way a real server does: collapse repeated
        // separators and resolve `.` / `..` segments, so two different
        // spellings of one file answer with one identity. There are no
        // symlinks in this filesystem, so that is the whole of it.
        Ok(Name {
            id,
            files: vec![File::dummy(normalize_posix(&path))],
        })
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, StatusCode> {
        let mut fs = self.fs.lock().await;
        let exists = fs.files.contains_key(&filename);
        if pflags.contains(OpenFlags::READ) && !exists {
            return Err(StatusCode::NoSuchFile);
        }
        if pflags.contains(OpenFlags::TRUNCATE)
            || (pflags.contains(OpenFlags::CREATE) && !exists)
        {
            fs.files.insert(filename.clone(), Vec::new());
        }
        fs.next_handle += 1;
        let hid = format!("h{}", fs.next_handle);
        fs.handles.insert(hid.clone(), filename);
        Ok(Handle { id, handle: hid })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, StatusCode> {
        self.fs.lock().await.handles.remove(&handle);
        Ok(ok_status(id))
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, StatusCode> {
        let fs = self.fs.lock().await;
        let path = fs.handles.get(&handle).ok_or(StatusCode::Failure)?;
        let data = fs.files.get(path).ok_or(StatusCode::NoSuchFile)?;
        let off = offset as usize;
        if off >= data.len() {
            // Clean EOF: the client maps this to a 0-byte read.
            return Err(StatusCode::Eof);
        }
        let end = (off + len as usize).min(data.len());
        Ok(Data {
            id,
            data: data[off..end].to_vec(),
        })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, StatusCode> {
        let mut fs = self.fs.lock().await;
        let path = fs.handles.get(&handle).ok_or(StatusCode::Failure)?.clone();
        let buf = fs.files.entry(path).or_default();
        let off = offset as usize;
        let end = off + data.len();
        if buf.len() < end {
            buf.resize(end, 0);
        }
        buf[off..end].copy_from_slice(&data);
        Ok(ok_status(id))
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        let fs = self.fs.lock().await;
        let size = fs
            .files
            .get(&path)
            .map(|d| d.len() as u64)
            .ok_or(StatusCode::NoSuchFile)?;
        Ok(attrs_with_size(id, size))
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, StatusCode> {
        let fs = self.fs.lock().await;
        let path = fs.handles.get(&handle).ok_or(StatusCode::Failure)?;
        let size = fs.files.get(path).map(|d| d.len() as u64).unwrap_or(0);
        Ok(attrs_with_size(id, size))
    }

    /// SFTP v3 semantics, which is what the transfers have to cope with:
    /// FAIL when the destination exists. The scratch-name upload path
    /// falls back to remove + rename precisely because of this, and that
    /// fallback only gets exercised if the fake server refuses like a real
    /// one instead of quietly overwriting.
    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, StatusCode> {
        let mut fs = self.fs.lock().await;
        if fs.files.contains_key(&newpath) {
            return Err(StatusCode::Failure);
        }
        let data = fs.files.remove(&oldpath).ok_or(StatusCode::NoSuchFile)?;
        fs.files.insert(newpath, data);
        Ok(ok_status(id))
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, StatusCode> {
        let mut fs = self.fs.lock().await;
        fs.files.remove(&filename).ok_or(StatusCode::NoSuchFile)?;
        Ok(ok_status(id))
    }

    /// Truncate is SETSTAT carrying only a size, which is how an
    /// interrupted upload trims its partial back to the contiguous prefix.
    async fn setstat(
        &mut self,
        id: u32,
        path: String,
        attrs: FileAttributes,
    ) -> Result<Status, StatusCode> {
        let Some(size) = attrs.size else {
            return Ok(ok_status(id));
        };
        let mut fs = self.fs.lock().await;
        let data = fs.files.get_mut(&path).ok_or(StatusCode::NoSuchFile)?;
        data.resize(size as usize, 0);
        Ok(ok_status(id))
    }
}

fn attrs_with_size(id: u32, size: u64) -> Attrs {
    let mut attrs = FileAttributes::empty();
    attrs.size = Some(size);
    Attrs { id, attrs }
}

// ---------------------------------------------------------------------------
// SSH server handler: accept everything, hand the sftp subsystem channel
// to the in-memory SFTP handler.
// ---------------------------------------------------------------------------

struct SshHarness {
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    fs: SharedFs,
    /// Once set, a session channel open is never answered: the reply is
    /// parked here instead, which is what a link that died after the
    /// last exchange looks like to the client (no accept, no refusal).
    stall_opens: Arc<std::sync::atomic::AtomicBool>,
    parked: Vec<russh::server::ChannelOpenHandle>,
}

impl russh::server::Handler for SshHarness {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<Auth, russh::Error> {
        Ok(Auth::Accept)
    }

    async fn auth_password(&mut self, _user: &str, _password: &str) -> Result<Auth, russh::Error> {
        Ok(Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), russh::Error> {
        if self.stall_opens.load(std::sync::atomic::Ordering::SeqCst) {
            self.parked.push(reply);
            return Ok(());
        }
        reply.accept().await;
        self.channels.lock().await.insert(channel.id(), channel);
        Ok(())
    }

    /// A `direct-tcpip` open to [`UNREACHABLE_TARGET`] is parked like a
    /// stalled session open: what a server looks like while its own
    /// connect to a host that drops SYNs is still pending (minutes, on a
    /// real sshd). Any other target is accepted, and the channel kept
    /// open for as long as the server runs.
    #[allow(clippy::too_many_arguments)]
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), russh::Error> {
        if host_to_connect == UNREACHABLE_TARGET {
            self.parked.push(reply);
            return Ok(());
        }
        let refusal = match host_to_connect {
            REFUSED_TARGET => Some(russh::ChannelOpenFailure::ConnectFailed),
            FORBIDDEN_TARGET => Some(russh::ChannelOpenFailure::AdministrativelyProhibited),
            _ => None,
        };
        if let Some(reason) = refusal {
            reply.reject(reason).await;
            return Ok(());
        }
        reply.accept().await;
        match host_to_connect {
            FLOOD_TARGET => {
                tokio::spawn(async move {
                    let chunk = vec![0x5a_u8; 32 * 1024];
                    for _ in 0..(FLOOD_BYTES / chunk.len()) {
                        if channel.data(&chunk[..]).await.is_err() {
                            break;
                        }
                    }
                });
            }
            ECHO_TARGET => {
                // Reading and writing on separate tasks, with nothing
                // bounded between them: an echo that reads only after its
                // last write went out stops reading whenever the client
                // is slow to take the echo, and the two ends then wait on
                // each other.
                let (mut rx, tx) = channel.split();
                let (queue_tx, mut queue_rx) = tokio::sync::mpsc::unbounded_channel();
                tokio::spawn(async move {
                    while let Some(msg) = rx.wait().await {
                        match msg {
                            russh::ChannelMsg::Data { data } => {
                                if queue_tx.send(data).is_err() {
                                    break;
                                }
                            }
                            russh::ChannelMsg::Eof => break,
                            _ => {}
                        }
                    }
                });
                tokio::spawn(async move {
                    while let Some(data) = queue_rx.recv().await {
                        if tx.data(&data[..]).await.is_err() {
                            return;
                        }
                    }
                    let _ = tx.eof().await;
                    let _ = tx.close().await;
                });
            }
            ANSWER_AFTER_EOF_TARGET => {
                tokio::spawn(async move {
                    let mut channel = channel;
                    let mut heard = 0usize;
                    while let Some(msg) = channel.wait().await {
                        match msg {
                            russh::ChannelMsg::Data { data } => heard += data.len(),
                            russh::ChannelMsg::Eof => {
                                let answer = format!("heard {heard}");
                                let _ = channel.data(answer.as_bytes()).await;
                                let _ = channel.eof().await;
                                let _ = channel.close().await;
                                break;
                            }
                            _ => {}
                        }
                    }
                });
            }
            _ => {
                self.channels.lock().await.insert(channel.id(), channel);
            }
        }
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        if name == "sftp" {
            let channel = self
                .channels
                .lock()
                .await
                .remove(&channel_id)
                .expect("channel opened before subsystem request");
            session.channel_success(channel_id)?;
            let handler = SftpHandler {
                fs: self.fs.clone(),
                version: None,
            };
            // `run` spawns its own task and returns promptly.
            russh_sftp::server::run(channel.into_stream(), handler).await;
        } else {
            session.channel_failure(channel_id)?;
        }
        Ok(())
    }
}

/// The `direct-tcpip` target the harness server never answers for.
const UNREACHABLE_TARGET: &str = "unreachable.invalid";
/// A target the server's own connect fails for.
const REFUSED_TARGET: &str = "refused.invalid";
/// A target the server will not forward to (`PermitOpen`).
const FORBIDDEN_TARGET: &str = "forbidden.invalid";
/// A target that sends [`FLOOD_BYTES`] the moment it is reached.
const FLOOD_TARGET: &str = "flood.invalid";
const FLOOD_BYTES: usize = 64 * 1024 * 1024;
/// A target that sends back whatever it receives.
const ECHO_TARGET: &str = "echo.invalid";
/// A target that answers only once the client has finished sending: the
/// request / EOF / response shape of anything that half-closes.
const ANSWER_AFTER_EOF_TARGET: &str = "answer-after-eof.invalid";

// ---------------------------------------------------------------------------
// Harness entry point
// ---------------------------------------------------------------------------

/// Stand up the in-process server and return a connected [`SftpClient`]
/// plus a handle to the server's filesystem (for seeding / inspecting).
async fn connect_in_memory() -> (SftpClient, SharedFs) {
    let (client, fs, _) = connect_in_memory_stallable().await;
    (client, fs)
}

/// [`connect_in_memory`], plus the switch that makes the server stop
/// answering new session channel opens (the sftp channel is already
/// open by then, so the client stays usable for everything else).
async fn connect_in_memory_stallable() -> (SftpClient, SharedFs, Arc<std::sync::atomic::AtomicBool>) {
    let (shared, fs, stall_opens) = connect_handle_in_memory().await;

    // Open the sftp subsystem the same way `engine::open_sftp` does.
    let timeout = std::time::Duration::from_secs(10);
    let session = {
        let channel = shared.channel_open_session().await.expect("channel_open_session");
        channel
            .request_subsystem(true, "sftp")
            .await
            .expect("request_subsystem");
        russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .expect("SftpSession::new")
    };
    let client = SftpClient::new(session, shared, timeout);
    (client, fs, stall_opens)
}

/// The authenticated connection itself, before any channel: what a
/// forward rides.
async fn connect_handle_in_memory() -> (SharedHandle, SharedFs, Arc<std::sync::atomic::AtomicBool>) {
    use russh::keys::PrivateKey;

    let fs: SharedFs = Arc::new(Mutex::new(Fs::default()));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);

    // Server side. A fixed throwaway host key (generated once with
    // ssh-keygen) so the harness needs no RNG dependency; the client
    // trusts any key (see `ClientHandler::test_accept_all`).
    let mut server_config = russh::server::Config::default();
    let host_key = PrivateKey::from_openssh(HARNESS_HOST_KEY).expect("parse host key");
    server_config.keys.push(host_key);
    let server_config = Arc::new(server_config);
    let stall_opens = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_handler = SshHarness {
        channels: Arc::new(Mutex::new(HashMap::new())),
        fs: fs.clone(),
        stall_opens: stall_opens.clone(),
        parked: Vec::new(),
    };
    tokio::spawn(async move {
        if let Ok(running) =
            russh::server::run_stream(server_config, server_io, server_handler).await
        {
            let _ = running.await;
        }
    });

    // Client side.
    let client_config = Arc::new(russh::client::Config::default());
    let mut handle =
        russh::client::connect_stream(client_config, client_io, ClientHandler::test_accept_all())
            .await
            .expect("client connect_stream");
    let auth = handle
        .authenticate_password("tester", "tester")
        .await
        .expect("authenticate_password");
    assert!(auth.success(), "harness auth rejected");
    (Arc::new(handle), fs, stall_opens)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Round-trip a payload local -> remote -> local through the real
/// protocol and assert byte-identical. `size` chosen by the caller to hit
/// either the single-handle path or the windowed path.
async fn round_trip_through_server(size: usize) {
    let (client, _fs) = connect_in_memory().await;

    let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    let tmp = std::env::temp_dir();
    let pid = std::process::id();
    let src = tmp.join(format!("oryxis-harness-src-{pid}-{size}.bin"));
    let dst = tmp.join(format!("oryxis-harness-dst-{pid}-{size}.bin"));
    std::fs::write(&src, &payload).expect("write src");

    let remote = "/file.bin";
    client.upload_from(&src, remote).await.expect("upload_from");
    let stat = client.stat(remote).await.expect("stat");
    assert_eq!(stat.size, size as u64, "remote size mismatch");

    client
        .download_to(remote, &dst, None)
        .await
        .expect("download_to");
    let got = std::fs::read(&dst).expect("read dst");
    assert_eq!(got.len(), size, "size mismatch after round trip");
    assert_eq!(got, payload, "byte mismatch after round trip");

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
}

#[tokio::test]
async fn harness_small_round_trip() {
    // Below STREAM_THRESHOLD: single-handle sequential path.
    round_trip_through_server(64 * 1024).await;
}

#[tokio::test]
async fn harness_windowed_round_trip() {
    // Above STREAM_THRESHOLD (8 MiB): both directions use the sliding
    // window, so this is the real-server check of concurrent single-handle
    // reassembly and the request-id multiplexing the throughput win
    // depends on.
    round_trip_through_server(10 * 1024 * 1024).await;
}

#[tokio::test]
async fn harness_threshold_boundary() {
    // Exactly at the threshold: first size that takes the windowed path.
    round_trip_through_server(8 * 1024 * 1024).await;
}

#[tokio::test]
async fn harness_relay_between_servers() {
    // Two independent in-process servers. Seed a file on the source, relay
    // it to the destination through the client, verify it landed intact.
    // This is the end-to-end server-to-server path (read from A, write to
    // B, bytes through this process).
    let (src, src_fs) = connect_in_memory().await;
    let (dst, dst_fs) = connect_in_memory().await;

    let payload: Vec<u8> = (0..600 * 1024).map(|i| (i % 251) as u8).collect();
    src_fs
        .lock()
        .await
        .files
        .insert("/source.bin".to_string(), payload.clone());

    src.relay_to("/source.bin", &dst, "/dest.bin", None)
        .await
        .expect("relay_to");

    let landed = dst_fs
        .lock()
        .await
        .files
        .get("/dest.bin")
        .cloned()
        .expect("dest file present after relay");
    assert_eq!(landed, payload, "relayed bytes mismatch");
}

#[tokio::test]
async fn harness_relay_onto_itself_is_refused() {
    // The destination is opened WRITE | CREATE | TRUNCATE before a byte
    // is read, so a self-relay would empty the file and the failure
    // cleanup would then remove it. Two SFTP clients on ONE connection
    // (what `open_sibling` gives, and what two panes mounted on one host
    // through a shared session are) must be refused before the open.
    let (src, fs) = connect_in_memory().await;
    let sibling = src.open_sibling().await.expect("open sibling");
    assert!(
        src.shares_session_with(&sibling),
        "a sibling must be recognised as the same connection"
    );

    let payload: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
    fs.lock()
        .await
        .files
        .insert("/only-copy.bin".to_string(), payload.clone());

    let err = src
        .relay_to("/only-copy.bin", &sibling, "/only-copy.bin", None)
        .await
        .expect_err("relaying a file onto itself must be refused");
    assert!(
        err.to_string().contains("same file"),
        "unexpected refusal message: {err}"
    );

    let survived = fs
        .lock()
        .await
        .files
        .get("/only-copy.bin")
        .cloned()
        .expect("the file must still exist after the refusal");
    assert_eq!(survived, payload, "the refused relay damaged the file");
}

#[tokio::test]
async fn harness_relay_onto_itself_spelled_differently_is_refused() {
    // Same file, two spellings. The guard compares resolved identities,
    // not strings, so `/sub/../data.bin` must be caught as `/data.bin`.
    let (src, fs) = connect_in_memory().await;
    let sibling = src.open_sibling().await.expect("open sibling");

    let payload: Vec<u8> = (0..32 * 1024).map(|i| (i % 251) as u8).collect();
    fs.lock()
        .await
        .files
        .insert("/data.bin".to_string(), payload.clone());

    let err = src
        .relay_to("/data.bin", &sibling, "/sub/../data.bin", None)
        .await
        .expect_err("a differently spelled self-relay must be refused");
    assert!(
        err.to_string().contains("same file"),
        "unexpected refusal message: {err}"
    );
    assert_eq!(
        fs.lock().await.files.get("/data.bin"),
        Some(&payload),
        "the refused relay damaged the file"
    );
}

#[tokio::test]
async fn harness_relay_between_paths_on_one_host_is_allowed() {
    // The guard is on the resolved FILE, never on the host. Copying
    // between two locations on one server is a legitimate move (the
    // reporter's case was two users on one machine) and must still work.
    let (src, fs) = connect_in_memory().await;
    let sibling = src.open_sibling().await.expect("open sibling");

    let payload: Vec<u8> = (0..48 * 1024).map(|i| (i % 251) as u8).collect();
    fs.lock()
        .await
        .files
        .insert("/home/a/report.bin".to_string(), payload.clone());

    src.relay_to("/home/a/report.bin", &sibling, "/home/b/report.bin", None)
        .await
        .expect("same-host relay to a different path must be allowed");

    let guard = fs.lock().await;
    assert_eq!(
        guard.files.get("/home/b/report.bin"),
        Some(&payload),
        "the copy did not land"
    );
    assert_eq!(
        guard.files.get("/home/a/report.bin"),
        Some(&payload),
        "a relay must not disturb its source"
    );
}

#[tokio::test]
async fn harness_relay_same_path_across_hosts_is_allowed() {
    // Two independent servers, identical path on both. Nothing about the
    // path makes these the same file, and refusing here would break the
    // most ordinary relay there is (`/etc/app.conf` from staging to
    // prod).
    let (src, src_fs) = connect_in_memory().await;
    let (dst, dst_fs) = connect_in_memory().await;
    assert!(
        !src.shares_session_with(&dst),
        "two separate connections must not be seen as one"
    );

    let payload: Vec<u8> = (0..16 * 1024).map(|i| (i % 251) as u8).collect();
    src_fs
        .lock()
        .await
        .files
        .insert("/etc/app.conf".to_string(), payload.clone());

    src.relay_to("/etc/app.conf", &dst, "/etc/app.conf", None)
        .await
        .expect("same path on two different hosts must be allowed");

    assert_eq!(
        dst_fs.lock().await.files.get("/etc/app.conf"),
        Some(&payload),
        "the cross-host relay did not land"
    );
}

#[tokio::test]
async fn harness_relay_windowed_stale_hint() {
    // Server-to-server relay above the window threshold (10 MiB), with a
    // size hint that is too small (8 MiB) but still >= threshold. The
    // windowed branch must fstat the source for the true size and relay
    // the WHOLE file to the destination, not truncate to the hint.
    let (src, src_fs) = connect_in_memory().await;
    let (dst, dst_fs) = connect_in_memory().await;

    let actual_size = 10 * 1024 * 1024usize;
    let payload: Vec<u8> = (0..actual_size).map(|i| (i % 251) as u8).collect();
    src_fs
        .lock()
        .await
        .files
        .insert("/big-source.bin".to_string(), payload.clone());

    src.relay_to(
        "/big-source.bin",
        &dst,
        "/big-dest.bin",
        Some(8 * 1024 * 1024),
    )
    .await
    .expect("windowed relay with stale hint");

    let landed = dst_fs
        .lock()
        .await
        .files
        .get("/big-dest.bin")
        .cloned()
        .expect("dest file present after windowed relay");
    assert_eq!(landed.len(), actual_size, "stale hint truncated the relay");
    assert_eq!(landed, payload, "windowed relay corrupted the file");
}

#[tokio::test]
async fn harness_stale_hint_not_truncated() {
    // A size hint smaller than the real file (file grew since the dir
    // walk) but still >= threshold: the windowed branch must fstat the
    // open handle for the true size and download the WHOLE file, not
    // truncate to the stale hint. Without the fstat this silently
    // truncates and "succeeds".
    let (client, fs) = connect_in_memory().await;
    let actual_size = 10 * 1024 * 1024usize;
    let payload: Vec<u8> = (0..actual_size).map(|i| (i % 251) as u8).collect();
    fs.lock()
        .await
        .files
        .insert("/grew.bin".to_string(), payload.clone());

    let tmp = std::env::temp_dir();
    let dst = tmp.join(format!("oryxis-stalehint-{}.bin", std::process::id()));
    // Hint is 8 MiB (>= threshold, so windowed) but the file is 10 MiB.
    let stale_hint = Some(8 * 1024 * 1024u64);
    client
        .download_to("/grew.bin", &dst, stale_hint)
        .await
        .expect("download_to with stale hint");
    let got = std::fs::read(&dst).expect("read dst");
    assert_eq!(got.len(), actual_size, "stale hint truncated the download");
    assert_eq!(got, payload, "stale hint corrupted the download");
    let _ = std::fs::remove_file(&dst);
}

#[tokio::test]
async fn harness_ranged_reads() {
    // The zip-browse primitive: positioned reads over the real protocol
    // path, including spans that need several concurrent requests and
    // the EOF clamp.
    let (client, fs) = connect_in_memory().await;
    let size = 2 * 1024 * 1024usize;
    let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    fs.lock()
        .await
        .files
        .insert("/big.zip".to_string(), payload.clone());

    let file = client.open_ranged("/big.zip").await.expect("open_ranged");
    assert_eq!(file.len(), size as u64);
    // Small tail read (the EOCD probe pattern).
    let tail = file.read_at(size as u64 - 100, 100).await.expect("tail");
    assert_eq!(tail, &payload[size - 100..]);
    // A middle span larger than one 255 KiB request (concurrent fan-out).
    let mid = file.read_at(300_000, 600_000).await.expect("mid");
    assert_eq!(mid, &payload[300_000..900_000]);
    // Clamped at EOF; fully past EOF is empty.
    let clamp = file.read_at(size as u64 - 10, 50).await.expect("clamp");
    assert_eq!(clamp, &payload[size - 10..]);
    assert!(
        file.read_at(size as u64, 10).await.expect("past").is_empty()
    );
    file.close().await;
}

/// The scratch file is where an interrupted download's bytes live, so the
/// target name must stay untouched until the very end and the scratch must
/// be gone once it is claimed. Both halves matter: the first is what stops
/// a truncated file from looking complete, the second is what stops the
/// download folder from filling with debris.
#[tokio::test]
async fn harness_download_uses_scratch_then_claims_target() {
    let (client, fs) = connect_in_memory().await;
    let size = 9 * 1024 * 1024usize;
    let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    fs.lock()
        .await
        .files
        .insert("/scratch.bin".to_string(), payload.clone());

    let tmp = std::env::temp_dir();
    let dst = tmp.join(format!("oryxis-scratch-{}.bin", std::process::id()));
    let part = {
        let mut raw = dst.as_os_str().to_os_string();
        raw.push(".oryxis-part");
        std::path::PathBuf::from(raw)
    };
    let _ = std::fs::remove_file(&dst);
    let _ = std::fs::remove_file(&part);

    client
        .download_to("/scratch.bin", &dst, None)
        .await
        .expect("download_to");
    assert_eq!(std::fs::read(&dst).expect("read dst"), payload);
    assert!(
        !part.exists(),
        "the scratch file must not survive a successful download"
    );
    let _ = std::fs::remove_file(&dst);
}

/// Resume the real `download_to` against the real protocol.
///
/// Proving a resume ACTUALLY happened needs more than a correct result: a
/// download that silently restarted would also produce one. So the server
/// is given a file that DIFFERS from the scratch file below the verified
/// tail. Resuming keeps the scratch file's bytes there and yields the
/// expected content; restarting would overwrite them with the server's and
/// yield something else.
#[tokio::test]
async fn harness_download_resumes_a_valid_scratch_file() {
    let (client, fs) = connect_in_memory().await;
    let size = 12 * 1024 * 1024usize;
    let cut = 8 * 1024 * 1024usize;
    // What the scratch file holds, and what the finished download must be.
    let expected: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    // What the server serves: identical except in the region a resume will
    // never ask for, which is everything below the tail it verifies.
    let poison_end = crate::sftp::resume_offset(size as u64, cut as u64) as usize - 64 * 1024;
    assert!(poison_end > 0, "rewind left no room to poison");
    let mut served = expected.clone();
    served[..poison_end].fill(0x5A);
    fs.lock()
        .await
        .files
        .insert("/resume.bin".to_string(), served);

    let tmp = std::env::temp_dir();
    let dst = tmp.join(format!("oryxis-resume-{}.bin", std::process::id()));
    let part = {
        let mut raw = dst.as_os_str().to_os_string();
        raw.push(".oryxis-part");
        std::path::PathBuf::from(raw)
    };
    let _ = std::fs::remove_file(&dst);
    std::fs::write(&part, &expected[..cut]).expect("seed scratch");

    let moved = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    client
        .download_to_progress("/resume.bin", &dst, None, Some(moved.clone()))
        .await
        .expect("resumed download");

    let got = std::fs::read(&dst).expect("read dst");
    assert_eq!(
        &got[..poison_end],
        &expected[..poison_end],
        "the download restarted instead of resuming (the server's bytes won)"
    );
    assert_eq!(
        &got[poison_end..],
        &expected[poison_end..],
        "the resumed remainder does not match the source"
    );
    // The counter is seeded with what was already on disk and then only
    // advanced by real transfers, so a resume totals the file exactly once.
    assert_eq!(
        moved.load(std::sync::atomic::Ordering::Relaxed),
        size as u64,
        "progress must account for the whole file exactly once"
    );
    let _ = std::fs::remove_file(&dst);
}

/// The other half of the contract: a scratch file whose tail does NOT
/// match the server is not ours to continue, so the download starts over
/// and still produces the right bytes. Silent corruption is the failure
/// this test exists to catch.
#[tokio::test]
async fn harness_download_restarts_on_a_mismatched_scratch_file() {
    let (client, fs) = connect_in_memory().await;
    let size = 9 * 1024 * 1024usize;
    let cut = 4 * 1024 * 1024usize;
    let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    fs.lock()
        .await
        .files
        .insert("/mismatch.bin".to_string(), payload.clone());

    let tmp = std::env::temp_dir();
    let dst = tmp.join(format!("oryxis-mismatch-{}.bin", std::process::id()));
    let part = {
        let mut raw = dst.as_os_str().to_os_string();
        raw.push(".oryxis-part");
        std::path::PathBuf::from(raw)
    };
    let _ = std::fs::remove_file(&dst);
    // Same LENGTH as a valid prefix, different bytes: length alone would
    // have accepted this and spliced two unrelated files together.
    std::fs::write(&part, vec![0x5Au8; cut]).expect("seed scratch");

    client
        .download_to("/mismatch.bin", &dst, None)
        .await
        .expect("download after a mismatched scratch file");
    assert_eq!(
        std::fs::read(&dst).expect("read dst"),
        payload,
        "a mismatched scratch file must be discarded, not spliced"
    );
    let _ = std::fs::remove_file(&dst);
}

/// Upload resume over the real protocol, plus the two refusals that guard
/// it. The engine never resumes an upload on its own; these exercise what
/// happens once a caller (having asked the user) turns it on.
#[tokio::test]
async fn harness_upload_resume_and_its_refusals() {
    let (client, fs) = connect_in_memory().await;
    let size = 12 * 1024 * 1024usize;
    let cut = 8 * 1024 * 1024usize;
    let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();

    let tmp = std::env::temp_dir();
    let src = tmp.join(format!("oryxis-upresume-{}.bin", std::process::id()));
    std::fs::write(&src, &payload).expect("write src");

    // A partial upload sitting on the server, correct as far as it goes
    // except for one poisoned byte well below the tail the resume
    // verifies. A resume never rewrites that region, so the byte survives;
    // an upload that silently restarted would erase it. That is what makes
    // this a test of resuming rather than of uploading.
    let poison_at = 0usize;
    let mut partial = payload[..cut].to_vec();
    partial[poison_at] ^= 0xFF;
    assert!(
        crate::sftp::resume_offset(size as u64, cut as u64) as usize > poison_at + 64 * 1024,
        "poison must sit below the verified tail"
    );
    fs.lock()
        .await
        .files
        .insert("/up.bin".to_string(), partial.clone());
    let moved = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    client
        .upload_from_options(
            &src,
            "/up.bin",
            crate::sftp::UploadOptions {
                progress: Some(moved.clone()),
                resume: true,
                temp_name: false,
                fsync: false,
            },
        )
        .await
        .expect("resumed upload");
    let mut want = payload.clone();
    want[poison_at] ^= 0xFF;
    assert_eq!(
        fs.lock().await.files.get("/up.bin").expect("uploaded"),
        &want,
        "the upload restarted instead of resuming (the poisoned byte was rewritten)"
    );
    assert_eq!(
        moved.load(std::sync::atomic::Ordering::Relaxed),
        size as u64,
        "progress must account for the whole file exactly once"
    );

    // Same length, wrong bytes: refused rather than spliced, and the
    // destination is left exactly as it was.
    let junk = vec![0x5Au8; cut];
    fs.lock()
        .await
        .files
        .insert("/bad.bin".to_string(), junk.clone());
    let err = client
        .upload_from_options(
            &src,
            "/bad.bin",
            crate::sftp::UploadOptions {
                resume: true,
                ..Default::default()
            },
        )
        .await
        .expect_err("a mismatched partial must refuse");
    assert!(
        err.to_string().contains("do not match"),
        "unexpected error: {err}"
    );
    assert_eq!(
        fs.lock().await.files.get("/bad.bin").expect("untouched"),
        &junk,
        "a refused resume must not touch the destination"
    );

    // Already at least as long as the source: cannot be a partial copy of
    // it, so there is nothing to resume.
    fs.lock()
        .await
        .files
        .insert("/long.bin".to_string(), vec![0u8; size + 1]);
    let err = client
        .upload_from_options(
            &src,
            "/long.bin",
            crate::sftp::UploadOptions {
                resume: true,
                ..Default::default()
            },
        )
        .await
        .expect_err("a longer destination must refuse");
    assert!(
        err.to_string().contains("cannot be a partial copy"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_file(&src);
}

/// With `temp_name` the real path is claimed only by the closing rename,
/// so a watcher on the server never sees a growing file under it.
#[tokio::test]
async fn harness_upload_temp_name_claims_the_target_at_the_end() {
    let (client, fs) = connect_in_memory().await;
    let size = 9 * 1024 * 1024usize;
    let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();

    let tmp = std::env::temp_dir();
    let src = tmp.join(format!("oryxis-uptemp-{}.bin", std::process::id()));
    std::fs::write(&src, &payload).expect("write src");

    client
        .upload_from_options(
            &src,
            "/temp.bin",
            crate::sftp::UploadOptions {
                temp_name: true,
                ..Default::default()
            },
        )
        .await
        .expect("upload with a temp name");

    let guard = fs.lock().await;
    assert_eq!(
        guard.files.get("/temp.bin").expect("target claimed"),
        &payload
    );
    assert!(
        !guard.files.contains_key("/temp.bin.oryxis-part"),
        "the scratch name must not survive a successful upload"
    );
    drop(guard);
    let _ = std::fs::remove_file(&src);
}

/// `exec_timeout` bounds the whole call, the channel OPEN included: a
/// server that never answers the open (a link that died after the last
/// exchange) must not hang the caller until a keepalive notices.
#[tokio::test]
async fn harness_exec_timeout_bounds_an_unanswered_channel_open() {
    let (client, _fs, stall_opens) = connect_in_memory_stallable().await;
    stall_opens.store(true, std::sync::atomic::Ordering::SeqCst);
    let limit = std::time::Duration::from_millis(300);
    let started = std::time::Instant::now();
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.exec_timeout("true", limit),
    )
    .await
    .expect("exec_timeout must not outlive its own limit");
    assert!(
        matches!(outcome, Err(crate::SshError::ExecTimeout(_))),
        "an unanswered open reads as a timeout"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

/// One SOCKS5 CONNECT by domain name, returning the reply code.
async fn socks5_connect(port: u16, host: &str) -> u8 {
    socks5_connect_from(std::net::Ipv4Addr::LOCALHOST.into(), port, host).await
}

/// The same request, dialled at the loopback address `proxy`.
async fn socks5_connect_from(proxy: std::net::IpAddr, port: u16, host: &str) -> u8 {
    socks5_open(proxy, port, host).await.0
}

/// A SOCKS5 CONNECT that hands back the stream with the reply code, for
/// tests that go on to use the tunnel.
async fn socks5_open(
    proxy: std::net::IpAddr,
    port: u16,
    host: &str,
) -> (u8, tokio::net::TcpStream) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect((proxy, port))
        .await
        .expect("connect to socks listener");
    s.write_all(&[0x05, 0x01, 0x00]).await.expect("greeting");
    let mut method = [0u8; 2];
    s.read_exact(&mut method).await.expect("method reply");
    let mut req = vec![0x05, 0x01, 0x00, 0x03, host.len() as u8];
    req.extend_from_slice(host.as_bytes());
    req.extend_from_slice(&443u16.to_be_bytes());
    s.write_all(&req).await.expect("connect request");
    let mut reply = [0u8; 10];
    s.read_exact(&mut reply).await.expect("connect reply");
    (reply[1], s)
}

/// A `-D` forward on an OS-chosen loopback port over a fresh in-memory
/// connection.
async fn socks_forward_in_memory() -> (u16, SharedHandle, tokio::sync::watch::Sender<bool>) {
    let (shared, _fs, _) = connect_handle_in_memory().await;
    let listener = crate::engine::bind_forward_listener("127.0.0.1", 0)
        .await
        .expect("bind socks listener");
    let port = listener.local_addr().expect("listener addr").port();
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    crate::engine::spawn_dynamic_forward_task(listener, Arc::clone(&shared), port, cancel_rx);
    (port, shared, cancel_tx)
}

/// A SOCKS request whose destination the server is still trying to
/// reach must not hold up the others on the same connection (issue
/// #246): a browser behind `-D` sends dozens at once, and one host that
/// drops SYNs used to stall every page until the server gave up on it.
#[tokio::test]
async fn harness_socks_request_is_not_held_up_by_an_unanswered_one() {
    let (shared, _fs, _) = connect_handle_in_memory().await;
    let listener = crate::engine::bind_forward_listener("127.0.0.1", 0)
        .await
        .expect("bind socks listener");
    let port = listener.local_addr().expect("listener addr").port();
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let _task = crate::engine::spawn_dynamic_forward_task(
        listener,
        Arc::clone(&shared),
        port,
        cancel_rx,
    );

    let stuck = tokio::spawn(socks5_connect(port, UNREACHABLE_TARGET));
    // Let the unanswered open reach the server first.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socks5_connect(port, "reachable.invalid"),
    )
    .await
    .expect("a reachable target answers while another open is pending");
    assert_eq!(reply, 0x00, "the reachable target is granted");
    assert!(!stuck.is_finished(), "the unreachable target is still pending");
    assert!(!shared.is_closed(), "the connection reads as alive");

    stuck.abort();
    let _ = cancel_tx.send(true);
}

/// A `-D` forward on "this machine" serves a client that dials `::1`
/// (issue #246): a browser told `localhost` resolves it to `::1` first,
/// and with the proxy on IPv4 alone every new connection waited out a
/// refusal there before trying `127.0.0.1` (about 2 s each on Windows).
#[tokio::test]
async fn harness_socks_forward_answers_on_ipv6_loopback() {
    use std::net::Ipv6Addr;
    if tokio::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).await.is_err() {
        // IPv6 is off on this machine: there is no `::1` to answer on.
        return;
    }
    let (shared, _fs, _) = connect_handle_in_memory().await;
    // The OS picks the port from the IPv4 space alone, so the same number
    // can be taken on `::1` by something unrelated: ask until both bind.
    let mut bound = None;
    for _ in 0..32 {
        let listener = crate::engine::bind_forward_listener("127.0.0.1", 0)
            .await
            .expect("bind socks listener");
        let port = listener.local_addr().expect("listener addr").port();
        if tokio::net::TcpStream::connect((Ipv6Addr::LOCALHOST, port)).await.is_ok() {
            bound = Some((listener, port));
            break;
        }
    }
    let (listener, port) = bound.expect("a port free on both loopback families");
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let _task = crate::engine::spawn_dynamic_forward_task(
        listener,
        Arc::clone(&shared),
        port,
        cancel_rx,
    );

    for proxy in [Ipv6Addr::LOCALHOST.into(), std::net::Ipv4Addr::LOCALHOST.into()] {
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            socks5_connect_from(proxy, port, "reachable.invalid"),
        )
        .await
        .expect("the proxy answers on this family");
        assert_eq!(reply, 0x00, "granted over {proxy}");
    }
    let _ = cancel_tx.send(true);
}

/// A client that finishes sending and then waits for the answer gets it:
/// the end of ITS half is passed on as an EOF, and the tunnel stays up
/// for the other direction.
#[tokio::test]
async fn harness_socks_client_that_half_closes_still_gets_the_answer() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (port, _shared, cancel_tx) = socks_forward_in_memory().await;
    let v4 = std::net::Ipv4Addr::LOCALHOST.into();
    let (reply, mut stream) = socks5_open(v4, port, ANSWER_AFTER_EOF_TARGET).await;
    assert_eq!(reply, 0x00);
    stream.write_all(b"request").await.expect("send request");
    stream.shutdown().await.expect("half-close");
    let mut answer = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), stream.read_to_end(&mut answer))
        .await
        .expect("the answer arrives after the half-close")
        .expect("read answer");
    assert_eq!(answer, b"heard 7");
    let _ = cancel_tx.send(true);
}

/// The far side finishing first leaves the client's half of the tunnel
/// working: what it sends after the EOF still arrives.
#[tokio::test]
async fn harness_socks_tunnel_outlives_one_finished_half() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (port, _shared, cancel_tx) = socks_forward_in_memory().await;
    let v4 = std::net::Ipv4Addr::LOCALHOST.into();
    let (reply, mut stream) = socks5_open(v4, port, ECHO_TARGET).await;
    assert_eq!(reply, 0x00);
    // Several writes with the echo read back between them: the tunnel
    // carries both directions for as long as both are open.
    for round in 0..3u8 {
        let sent = [round; 1024];
        stream.write_all(&sent).await.expect("send");
        let mut back = [0u8; 1024];
        tokio::time::timeout(std::time::Duration::from_secs(5), stream.read_exact(&mut back))
            .await
            .expect("echo arrives")
            .expect("read echo");
        assert_eq!(back, sent);
    }
    // The client finishes; the echo target answers the EOF with its own,
    // and the client reads a clean end of stream.
    stream.shutdown().await.expect("half-close");
    let mut rest = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), stream.read_to_end(&mut rest))
        .await
        .expect("the tunnel ends once both halves are done")
        .expect("clean end of stream");
    assert!(rest.is_empty());
    let _ = cancel_tx.send(true);
}

/// A SOCKS4 CONNECT (an address) and a SOCKS4A one (a name after the
/// user id) are served like a SOCKS5 one, each answered in its own
/// dialect.
#[tokio::test]
async fn harness_socks4_and_4a_clients_are_served() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (port, _shared, cancel_tx) = socks_forward_in_memory().await;

    // 4A: DSTIP 0.0.0.1 announces the name after the user id.
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.expect("connect");
    let mut req = vec![0x04, 0x01];
    req.extend_from_slice(&443u16.to_be_bytes());
    req.extend_from_slice(&[0, 0, 0, 1]);
    req.extend_from_slice(b"someone\0");
    req.extend_from_slice(ECHO_TARGET.as_bytes());
    req.push(0);
    s.write_all(&req).await.expect("socks4a request");
    let mut reply = [0u8; 8];
    s.read_exact(&mut reply).await.expect("socks4a reply");
    assert_eq!(reply[..2], [0x00, 0x5A], "granted, in the SOCKS4 reply format");
    s.write_all(b"ping").await.expect("send");
    let mut back = [0u8; 4];
    s.read_exact(&mut back).await.expect("echo through the 4A tunnel");
    assert_eq!(&back, b"ping");

    // Plain 4: a literal address and an empty user id.
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.expect("connect");
    let mut req = vec![0x04, 0x01];
    req.extend_from_slice(&80u16.to_be_bytes());
    req.extend_from_slice(&[192, 0, 2, 7]);
    req.push(0);
    s.write_all(&req).await.expect("socks4 request");
    let mut reply = [0u8; 8];
    s.read_exact(&mut reply).await.expect("socks4 reply");
    assert_eq!(reply[..2], [0x00, 0x5A]);

    // BIND is not served, and says so in the same dialect.
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.expect("connect");
    let mut req = vec![0x04, 0x02];
    req.extend_from_slice(&80u16.to_be_bytes());
    req.extend_from_slice(&[192, 0, 2, 7]);
    req.push(0);
    s.write_all(&req).await.expect("socks4 bind");
    let mut reply = [0u8; 8];
    s.read_exact(&mut reply).await.expect("socks4 rejection");
    assert_eq!(reply[..2], [0x00, 0x5B]);
    let _ = cancel_tx.send(true);
}

/// The server's answer to the open reaches the client as the SOCKS
/// error that says why, in the dialect it asked in.
#[tokio::test]
async fn harness_socks_reply_says_why_the_tunnel_was_not_opened() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (port, _shared, cancel_tx) = socks_forward_in_memory().await;
    let v4 = std::net::Ipv4Addr::LOCALHOST.into();
    assert_eq!(socks5_connect_from(v4, port, REFUSED_TARGET).await, 0x05);
    assert_eq!(socks5_connect_from(v4, port, FORBIDDEN_TARGET).await, 0x02);

    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.expect("connect");
    let mut req = vec![0x04, 0x01];
    req.extend_from_slice(&443u16.to_be_bytes());
    req.extend_from_slice(&[0, 0, 0, 1, 0]);
    req.extend_from_slice(REFUSED_TARGET.as_bytes());
    req.push(0);
    s.write_all(&req).await.expect("socks4a request");
    let mut reply = [0u8; 8];
    s.read_exact(&mut reply).await.expect("socks4a reply");
    assert_eq!(reply[..2], [0x00, 0x5B]);
    let _ = cancel_tx.send(true);
}

/// A tunnel whose client stopped reading (a paused download, a client
/// that hangs) holds up nothing but itself: its own channel runs out of
/// window and the server stops sending on it, while every other tunnel on
/// the connection goes on. Needs the per-channel flow control of the
/// russh fork (see the patch entry in the root `Cargo.toml`).
#[tokio::test]
async fn harness_socks_stream_nobody_reads_does_not_hold_up_the_others() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (port, shared, cancel_tx) = socks_forward_in_memory().await;
    let v4 = std::net::Ipv4Addr::LOCALHOST.into();
    let (reply, mut unread) = socks5_open(v4, port, FLOOD_TARGET).await;
    assert_eq!(reply, 0x00);
    // Let the flood fill every buffer between the server and the client
    // that is not reading.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    for round in 0..3u8 {
        let echoed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let (reply, mut stream) = socks5_open(v4, port, ECHO_TARGET).await;
            assert_eq!(reply, 0x00);
            stream.write_all(&[round; 4]).await.expect("send");
            let mut back = [0u8; 4];
            stream.read_exact(&mut back).await.expect("echo");
            back
        })
        .await
        .expect("another tunnel answers while one is not being read");
        assert_eq!(echoed, [round; 4]);
    }
    assert!(!shared.is_closed(), "the connection is still up");

    // The stalled tunnel was only waiting: once its client reads, the
    // whole stream arrives.
    let mut total = 0usize;
    let mut buf = vec![0u8; 64 * 1024];
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while total < FLOOD_BYTES {
            let n = unread.read(&mut buf).await.expect("read the flood");
            assert!(n > 0, "the flood ended early at {total}");
            assert!(buf[..n].iter().all(|b| *b == 0x5a));
            total += n;
        }
    })
    .await
    .expect("the stalled tunnel resumes");
    assert_eq!(total, FLOOD_BYTES);
    let _ = cancel_tx.send(true);
}
