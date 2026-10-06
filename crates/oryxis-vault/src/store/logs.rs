use super::*;

/// Upper bound on the bytes of ONE recording the History content
/// search will pull into memory and scan. Applied on both ends:
/// [`VaultStore::sealed_session_output_scan`] stops collecting sealed
/// chunks past this many ciphertext bytes, and
/// [`SealedSessionOutput::open`] caps the inflated plaintext at the
/// same bound (deflated chunks can expand well past their stored
/// size). 4 MiB covers virtually every real terminal session while
/// keeping the worst case of a single scan step (full decrypt +
/// inflate + ANSI render, re-run per settled search query) bounded
/// even against a multi-hundred-MB runaway recording. A match beyond
/// the cap is deliberately missed: the scan is best-effort, same
/// contract as rows that don't unseal under a locked vault.
pub const CONTENT_SEARCH_MAX_SCAN_BYTES: usize = 4 * 1024 * 1024;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Logs CRUD
    // -----------------------------------------------------------------------

    pub fn add_log(&self, entry: &oryxis_core::models::log_entry::LogEntry) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT INTO logs (id, connection_label, hostname, event, message, timestamp)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                entry.id.to_string(), entry.connection_label, entry.hostname,
                entry.event.to_string(), entry.message, entry.timestamp.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_logs(&self, limit: usize) -> Result<Vec<oryxis_core::models::log_entry::LogEntry>, VaultError> {
        self.list_logs_page(0, limit)
    }

    /// Paginated variant: skip `offset` rows and return up to `limit` rows
    /// (still ordered by timestamp desc).
    pub fn list_logs_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<oryxis_core::models::log_entry::LogEntry>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_label, hostname, event, message, timestamp
             FROM logs ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2",
        )?;
        let logs = stmt.query_map(params![limit as i64, offset as i64], |row| {
            let event_str: String = row.get(3)?;
            let event = match event_str.as_str() {
                "Connected" => oryxis_core::models::log_entry::LogEvent::Connected,
                "Disconnected" => oryxis_core::models::log_entry::LogEvent::Disconnected,
                "Auth Failed" => oryxis_core::models::log_entry::LogEvent::AuthFailed,
                "Sync Applied" => oryxis_core::models::log_entry::LogEvent::SyncApplied,
                _ => oryxis_core::models::log_entry::LogEvent::Error,
            };
            Ok(oryxis_core::models::log_entry::LogEntry {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                connection_label: row.get(1)?,
                hostname: row.get(2)?,
                event,
                message: row.get(4)?,
                timestamp: row.get::<_, String>(5).ok()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now),
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    pub fn clear_logs(&self) -> Result<(), VaultError> {
        self.db.execute("DELETE FROM logs", [])?;
        Ok(())
    }

    /// Total number of log rows, used to drive pagination controls.
    pub fn count_logs(&self) -> Result<usize, VaultError> {
        let n: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    // -----------------------------------------------------------------------
    // Session Logs CRUD (terminal recording)
    // -----------------------------------------------------------------------

    /// Create a new session log entry with started_at = now.
    pub fn create_session_log(
        &self,
        id: &Uuid,
        connection_id: &Uuid,
        label: &str,
    ) -> Result<(), VaultError> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT INTO session_logs (id, connection_id, label, started_at, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.to_string(),
                connection_id.to_string(),
                label,
                now,
                Vec::<u8>::new(),
            ],
        )?;
        Ok(())
    }

    /// Content key for locally recorded terminal data (session-log
    /// chunks and the encrypted `command_history` text): a random
    /// 256-bit key wrapped with the master key in `vault_meta`
    /// (`session_log_key`). Payloads are sealed with this key directly
    /// (no per-write KDF), so appends stay cheap; only the first use
    /// after unlock pays the unwrap. Generated lazily on first use in a
    /// vault's lifetime. Master-password rotation re-wraps it via
    /// `convert_all_fields`, so the sealed rows never need rewriting.
    pub(super) fn session_log_key(&self) -> Result<[u8; KEY_LEN], VaultError> {
        if let Some(k) = *self.session_log_key.lock().unwrap() {
            return Ok(k);
        }
        let master = self.require_unlocked()?;
        let wrapped: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT value FROM vault_meta WHERE key = 'session_log_key'",
                [],
                |row| row.get(0),
            )
            .ok();
        let key: [u8; KEY_LEN] = match wrapped {
            Some(w) => decrypt_with_key(&w, master)?
                .try_into()
                .map_err(|_| VaultError::Crypto("malformed session log key".into()))?,
            None => {
                let mut k = [0u8; KEY_LEN];
                super::os_random(&mut k)?;
                let w = encrypt_with_key(&k, master)?;
                self.db.execute(
                    "INSERT OR REPLACE INTO vault_meta (key, value) VALUES ('session_log_key', ?1)",
                    params![w],
                )?;
                k
            }
        };
        *self.session_log_key.lock().unwrap() = Some(key);
        Ok(key)
    }

    /// Seal a payload with the content key: random nonce(12) +
    /// ciphertext(+16 tag).
    pub(super) fn seal_chunk(&self, data: &[u8]) -> Result<Vec<u8>, VaultError> {
        let key = self.session_log_key()?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|e| VaultError::Crypto(e.to_string()))?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        super::os_random(&mut nonce_bytes)?;
        let ct = cipher
            .encrypt(&Nonce::from(nonce_bytes), data)
            .map_err(|e| VaultError::Crypto(e.to_string()))?;
        let mut blob = Vec::with_capacity(NONCE_LEN + ct.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ct);
        Ok(blob)
    }

    /// Inverse of `seal_chunk`. `None` when the blob isn't a sealed
    /// chunk under `key` (i.e. a chunk recorded by an older version),
    /// in which case the caller uses the raw bytes as-is.
    pub(super) fn unseal_chunk(key: &[u8; KEY_LEN], blob: &[u8]) -> Option<Vec<u8>> {
        if blob.len() < NONCE_LEN + 16 {
            return None;
        }
        let cipher = ChaCha20Poly1305::new_from_slice(key).ok()?;
        cipher
            .decrypt(&Nonce::try_from(&blob[..NONCE_LEN]).expect("nonce length is a constant"), &blob[NONCE_LEN..])
            .ok()
    }

    /// Deflate a chunk's plaintext. `None` when compression wouldn't
    /// shrink it (already-compressed output, tiny chunks below the
    /// frame overhead), so the caller stores it raw with `comp = 0`.
    fn deflate_chunk(data: &[u8]) -> Option<Vec<u8>> {
        use std::io::Write;
        let mut enc = flate2::write::DeflateEncoder::new(
            Vec::with_capacity(data.len() / 2),
            flate2::Compression::new(6),
        );
        // Writing into a Vec cannot fail; treat an error as "don't
        // compress" rather than failing the append.
        if enc.write_all(data).is_err() {
            return None;
        }
        let out = enc.finish().ok()?;
        (out.len() < data.len()).then_some(out)
    }

    /// Inverse of `deflate_chunk` for rows flagged `comp = 1`. `None`
    /// on malformed data; the caller falls back to the stored bytes
    /// as-is (mirrors the `unseal_chunk` best-effort contract).
    fn inflate_chunk(data: &[u8]) -> Option<Vec<u8>> {
        use std::io::Write;
        let mut dec = flate2::write::DeflateDecoder::new(Vec::with_capacity(data.len() * 4));
        dec.write_all(data).ok()?;
        dec.finish().ok()
    }

    /// Apply the row's `comp` flag to an unsealed chunk.
    fn decode_chunk(plain: Vec<u8>, comp: i64) -> Vec<u8> {
        if comp == 1 {
            match Self::inflate_chunk(&plain) {
                Some(out) => out,
                None => plain,
            }
        } else {
            plain
        }
    }

    /// Append recorded terminal bytes to a session log. One INSERT of just
    /// the new bytes, no read-modify-write of the growing stream. Callers
    /// should batch (see the app's per-pane buffer) so this fires at a
    /// human cadence rather than once per SSH chunk. `offset_ms` is the
    /// chunk's capture time in milliseconds since the log's `started_at`
    /// (asciicast timing); `None` records without timing, like the
    /// pre-migration rows. `compress` deflates the plaintext before
    /// sealing (order matters: ciphertext doesn't compress) when the
    /// chunk is big enough for it to pay off; readers honor the per-row
    /// `comp` flag, so mixed logs (toggle flipped mid-history) are fine.
    pub fn append_session_data(
        &self,
        id: &Uuid,
        data: &[u8],
        offset_ms: Option<i64>,
        compress: bool,
    ) -> Result<(), VaultError> {
        if data.is_empty() {
            return Ok(());
        }
        /// Below this, the deflate framing eats the win (prompt redraws).
        const COMPRESS_MIN_BYTES: usize = 512;
        let deflated = (compress && data.len() >= COMPRESS_MIN_BYTES)
            .then(|| Self::deflate_chunk(data))
            .flatten();
        let (payload, comp): (&[u8], i64) = match &deflated {
            Some(d) => (d, 1),
            None => (data, 0),
        };
        let sealed = self.seal_chunk(payload)?;
        self.db.execute(
            "INSERT INTO session_log_chunks (log_id, data, offset_ms, kind, comp)
             VALUES (?1, ?2, ?3, 'o', ?4)",
            params![id.to_string(), sealed, offset_ms, comp],
        )?;
        Ok(())
    }

    /// Record a terminal resize in a session log (`kind = 'r'`, data
    /// `"<cols>x<rows>"`, sealed like output so nothing about the
    /// session leaks in plaintext). Replayers use it to keep the
    /// asciicast geometry in step with the live pane.
    pub fn append_session_resize(
        &self,
        id: &Uuid,
        offset_ms: i64,
        cols: u16,
        rows: u16,
    ) -> Result<(), VaultError> {
        let sealed = self.seal_chunk(format!("{cols}x{rows}").as_bytes())?;
        self.db.execute(
            "INSERT INTO session_log_chunks (log_id, data, offset_ms, kind)
             VALUES (?1, ?2, ?3, 'r')",
            params![id.to_string(), sealed, offset_ms],
        )?;
        Ok(())
    }

    /// Record one typed command in a session log (`kind = 'c'`, data =
    /// the command text, sealed like output). These rows feed the
    /// input-only (.txt) export; they are never part of the output
    /// byte stream (`get_session_data` filters on 'o') and the
    /// asciicast export skips them, so replay stays output-only.
    /// The text comes from the command-history capture, whose gates
    /// (prompt state, echo check) already keep unechoed secrets out.
    pub fn append_session_command(
        &self,
        id: &Uuid,
        offset_ms: Option<i64>,
        cmd: &str,
    ) -> Result<(), VaultError> {
        if cmd.is_empty() {
            return Ok(());
        }
        let sealed = self.seal_chunk(cmd.as_bytes())?;
        self.db.execute(
            "INSERT INTO session_log_chunks (log_id, data, offset_ms, kind)
             VALUES (?1, ?2, ?3, 'c')",
            params![id.to_string(), sealed, offset_ms],
        )?;
        Ok(())
    }

    /// Set ended_at = now on a session log.
    pub fn end_session_log(&self, id: &Uuid) -> Result<(), VaultError> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "UPDATE session_logs SET ended_at = ?1 WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }

    /// Shared row mapper for the session-log metadata projection
    /// (`id, connection_id, label, started_at, ended_at, size`), used
    /// by every query that returns `SessionLogEntry` rows.
    fn map_session_log_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionLogEntry> {
        Ok(SessionLogEntry {
            id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
            connection_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
            label: row.get(2)?,
            started_at: row
                .get::<_, String>(3)
                .ok()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(Utc::now),
            ended_at: row
                .get::<_, Option<String>>(4)?
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|d| d.with_timezone(&Utc)),
            data_size: row.get::<_, i64>(5).unwrap_or(0) as usize,
        })
    }

    /// List all session logs (metadata only, no data blob).
    pub fn list_session_logs(&self) -> Result<Vec<SessionLogEntry>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_id, label, started_at, ended_at,
                    COALESCE(LENGTH(data), 0) + bytes
             FROM session_logs ORDER BY started_at DESC",
        )?;
        let logs = stmt
            .query_map([], Self::map_session_log_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    /// Paginated variant of `list_session_logs`. Same column projection
    /// (no data blob), ordered by started_at desc, sliced by SQL LIMIT/OFFSET.
    pub fn list_session_logs_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<SessionLogEntry>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_id, label, started_at, ended_at,
                    COALESCE(LENGTH(data), 0) + bytes
             FROM session_logs ORDER BY started_at DESC LIMIT ?1 OFFSET ?2",
        )?;
        let logs = stmt
            .query_map(params![limit as i64, offset as i64], Self::map_session_log_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    /// One session log's metadata (same projection as
    /// [`Self::list_session_logs`]), or `None` when the row is gone.
    /// The History content search uses it to pull matched sessions
    /// that live beyond the UI's loaded page window into the timeline.
    pub fn get_session_log(&self, id: &Uuid) -> Result<Option<SessionLogEntry>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_id, label, started_at, ended_at,
                    COALESCE(LENGTH(data), 0) + bytes
             FROM session_logs WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id.to_string()], Self::map_session_log_row)?;
        Ok(rows.next().transpose()?)
    }

    /// Total number of session log rows.
    pub fn count_session_logs(&self) -> Result<usize, VaultError> {
        let n: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM session_logs",
            [],
            |row| row.get(0),
        )?;
        Ok(n as usize)
    }

    /// Ciphertext bytes every recording's chunks occupy together. The
    /// figure the size cap is measured against: the sum of the
    /// trigger-kept `bytes` column (see `ensure_session_log_bytes`), so
    /// one row per log is read instead of every chunk.
    pub fn session_logs_total_bytes(&self) -> Result<u64, VaultError> {
        let n: i64 = self.db.query_row(
            "SELECT COALESCE(SUM(bytes), 0) FROM session_logs",
            [],
            |row| row.get(0),
        )?;
        Ok(n.max(0) as u64)
    }

    /// Delete the OLDEST finished recordings until the total fits under
    /// `cap`, and report how many went.
    ///
    /// Retention by size, sibling of the retention by age above and the
    /// same shape as any log rotation: reaching a size limit drops the
    /// oldest history, it does not stop recording the present. That
    /// also fixes an asymmetry the age-based rule has on its own, where
    /// "1 day" deletes a 10 KB recording from yesterday and keeps a
    /// 40 GB one from today.
    ///
    /// In-progress recordings are never touched, for the same reason
    /// the age-based prune skips them: their rows are still being
    /// appended to. So a single runaway session can leave the total
    /// above `cap` with nothing left to drop, which is the case the
    /// caller has to handle by stopping the recording instead.
    pub fn prune_session_logs_to_fit(&self, cap: u64) -> Result<usize, VaultError> {
        let mut removed = 0usize;
        loop {
            if self.session_logs_total_bytes()? <= cap {
                return Ok(removed);
            }
            // Oldest FINISHED recording first.
            let oldest: Option<String> = {
                let mut stmt = self.db.prepare(
                    "SELECT id FROM session_logs
                     WHERE ended_at IS NOT NULL
                     ORDER BY started_at ASC LIMIT 1",
                )?;
                let mut rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.next().transpose()?
            };
            let Some(id) = oldest else {
                // Only in-progress recordings left: nothing here can
                // bring the total down.
                return Ok(removed);
            };
            self.db.execute(
                "DELETE FROM session_log_chunks WHERE log_id = ?1",
                params![id],
            )?;
            self.db
                .execute("DELETE FROM session_logs WHERE id = ?1", params![id])?;
            removed += 1;
        }
    }

    /// Mark a recording as cut short, so the player and the exports can
    /// say the stream is partial rather than presenting it as the whole
    /// session.
    pub fn mark_session_log_truncated(&self, log_id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "UPDATE session_logs SET truncated = 1 WHERE id = ?1",
            params![log_id.to_string()],
        )?;
        Ok(())
    }

    /// Whether a recording was cut short (see
    /// [`Self::mark_session_log_truncated`]).
    pub fn session_log_truncated(&self, log_id: &Uuid) -> Result<bool, VaultError> {
        let mut stmt = self
            .db
            .prepare("SELECT truncated FROM session_logs WHERE id = ?1")?;
        let mut rows =
            stmt.query_map(params![log_id.to_string()], |row| row.get::<_, i64>(0))?;
        Ok(rows.next().transpose()?.unwrap_or(0) != 0)
    }

    /// Delete connection events and *finished* session recordings
    /// older than `cutoff` (retention setting). In-progress sessions
    /// are never pruned: their rows are still being appended to.
    /// Returns how many rows (events + sessions) were removed.
    pub fn prune_logs_older_than(
        &self,
        cutoff: chrono::DateTime<Utc>,
    ) -> Result<usize, VaultError> {
        let cutoff = cutoff.to_rfc3339();
        let events = self.db.execute(
            "DELETE FROM logs WHERE timestamp < ?1",
            params![cutoff],
        )?;
        self.db.execute(
            "DELETE FROM session_log_chunks WHERE log_id IN
                 (SELECT id FROM session_logs
                  WHERE ended_at IS NOT NULL AND started_at < ?1)",
            params![cutoff],
        )?;
        let sessions = self.db.execute(
            "DELETE FROM session_logs WHERE ended_at IS NOT NULL AND started_at < ?1",
            params![cutoff],
        )?;
        Ok(events + sessions)
    }

    /// Drop every session log row (and its recorded chunks).
    pub fn clear_session_logs(&self) -> Result<(), VaultError> {
        self.db.execute("DELETE FROM session_log_chunks", [])?;
        self.db.execute("DELETE FROM session_logs", [])?;
        Ok(())
    }

    /// Get the raw recorded bytes for a session log: the legacy inline
    /// blob (empty for sessions recorded after the chunk migration)
    /// followed by every appended chunk in append order. The row lookup
    /// doubles as the existence check (NotFound when the log is gone).
    /// Sealed chunks are opened with the session content key; chunks
    /// recorded by older versions pass through as-is.
    pub fn get_session_data(&self, id: &Uuid) -> Result<Option<Vec<u8>>, VaultError> {
        Ok(Some(self.sealed_session_output(id)?.open()))
    }

    /// Fetch a session log's output stream still sealed, bundled with
    /// the content key: an opaque hand-off so a caller can move the
    /// expensive decrypt + inflate work onto a background thread (the
    /// History content search) while the crypto itself stays inside
    /// this crate. The SQL read here is cheap (ciphertext memcpy);
    /// [`SealedSessionOutput::open`] does the heavy part.
    pub fn sealed_session_output(
        &self,
        id: &Uuid,
    ) -> Result<SealedSessionOutput, VaultError> {
        self.sealed_session_output_inner(id, None)
    }

    /// Scan-bounded variant of [`Self::sealed_session_output`] for the
    /// History content search: collection stops once
    /// [`CONTENT_SEARCH_MAX_SCAN_BYTES`] of sealed bytes are gathered
    /// (bounding the RAM copy on the UI thread) and the bundle caps
    /// its inflated plaintext at the same bound when opened, so one
    /// oversized recording can never balloon a search step into
    /// gigabytes. Matches past the cap are missed by design; exports
    /// and the viewer keep using the unbounded reader.
    pub fn sealed_session_output_scan(
        &self,
        id: &Uuid,
    ) -> Result<SealedSessionOutput, VaultError> {
        self.sealed_session_output_inner(id, Some(CONTENT_SEARCH_MAX_SCAN_BYTES))
    }

    fn sealed_session_output_inner(
        &self,
        id: &Uuid,
        scan_cap: Option<usize>,
    ) -> Result<SealedSessionOutput, VaultError> {
        let id_str = id.to_string();
        let legacy: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT data FROM session_logs WHERE id = ?1",
                params![id_str],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Session log {}", id)))?;
        let mut legacy = legacy.unwrap_or_default();
        if let Some(cap) = scan_cap
            && legacy.len() > cap
        {
            legacy.truncate(cap);
        }
        // Output chunks only: resize events ('r') are timing metadata
        // for the asciicast export, not part of the byte stream.
        let mut stmt = self.db.prepare(
            "SELECT data, comp FROM session_log_chunks
             WHERE log_id = ?1 AND kind = 'o' ORDER BY id",
        )?;
        let rows = stmt.query_map(params![id_str], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut chunks = Vec::new();
        let mut collected = legacy.len();
        for row in rows {
            let row = row?;
            // Scan bound: stop copying ciphertext once the cap is
            // reached (the tail of an oversized recording is simply
            // not searched; noted at the constant).
            if let Some(cap) = scan_cap
                && collected >= cap
            {
                tracing::debug!(
                    "content search: session {} exceeds the {} byte scan cap, tail skipped",
                    id,
                    cap,
                );
                break;
            }
            collected += row.0.len();
            chunks.push(row);
        }
        Ok(SealedSessionOutput {
            key: self.session_log_key().ok().map(Zeroizing::new),
            legacy,
            chunks,
            scan_cap,
        })
    }

    /// Lightweight projection of EVERY session log for the History
    /// content search's output-scan queue: `(id, connection_id,
    /// label)`, newest first. Unlike the paged listing this walks the
    /// whole table but skips the per-row chunk-size aggregation, so a
    /// complete scan queue stays a single cheap column read even when
    /// the timeline UI only pages 50 rows at a time.
    pub fn list_session_log_scan_meta(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, String)>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_id, label FROM session_logs
             ORDER BY started_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(id, conn, label)| {
                (
                    Uuid::parse_str(&id).unwrap_or_default(),
                    Uuid::parse_str(&conn).unwrap_or_default(),
                    label,
                )
            })
            .collect())
    }

    /// Search the typed-command records (`kind = 'c'` chunks) of every
    /// session log for a case-insensitive substring. Returns at most
    /// one `(log_id, command)` pair per session (the first recorded
    /// match), so the History search can light up the exact sessions
    /// where a command ran without touching the output stream. The
    /// data volume is tiny (command lines only), so this runs
    /// synchronously. Requires the vault to be unlocked; locked rows
    /// simply don't open and are skipped, mirroring
    /// [`Self::get_session_commands`].
    ///
    /// Case-folding note: this tier (and `search_command_history`)
    /// matches with full Unicode `to_lowercase()`, while the app's
    /// output tier (`content_match_snippet`) folds 1:1 per char so
    /// its excerpt offsets stay aligned with the rendered text.
    /// Needles with multi-char foldings (İ, ß) can therefore match
    /// here but not there. Intentional divergence, documented at
    /// both sites.
    pub fn search_session_commands(
        &self,
        needle: &str,
    ) -> Result<Vec<(Uuid, String)>, VaultError> {
        let needle = needle.to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let key = self.session_log_key().ok();
        let mut stmt = self.db.prepare(
            "SELECT log_id, data, comp FROM session_log_chunks
             WHERE kind = 'c' ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut matches: Vec<(Uuid, String)> = Vec::new();
        let mut seen: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
        for row in rows {
            let (log_id, chunk, comp) = row?;
            let Ok(log_id) = Uuid::parse_str(&log_id) else {
                continue;
            };
            if seen.contains(&log_id) {
                continue;
            }
            let plain = match key.as_ref().and_then(|k| Self::unseal_chunk(k, &chunk)) {
                Some(plain) => plain,
                // Pre-encryption rows pass through; sealed rows that
                // don't open (locked vault) stay closed and unmatched.
                None => chunk,
            };
            let Ok(command) = String::from_utf8(Self::decode_chunk(plain, comp)) else {
                continue;
            };
            if command.to_lowercase().contains(&needle) {
                seen.insert(log_id);
                matches.push((log_id, command));
            }
        }
        Ok(matches)
    }

    /// Timed event stream for the asciicast export: the legacy inline
    /// blob first (no timing), then every chunk in append order with
    /// its capture offset and kind ('o' output / 'r' resize /
    /// 'c' typed command). Pre-migration chunks come back with
    /// `offset_ms = None`.
    pub fn get_session_events(
        &self,
        id: &Uuid,
    ) -> Result<Vec<SessionLogEvent>, VaultError> {
        let id_str = id.to_string();
        let legacy: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT data FROM session_logs WHERE id = ?1",
                params![id_str],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Session log {}", id)))?;
        let key = self.session_log_key().ok();
        let mut events: Vec<SessionLogEvent> = Vec::new();
        if let Some(data) = legacy.filter(|d| !d.is_empty()) {
            events.push(SessionLogEvent { offset_ms: None, kind: 'o', data });
        }
        let mut stmt = self.db.prepare(
            "SELECT data, offset_ms, kind, comp FROM session_log_chunks
             WHERE log_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![id_str], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        for row in rows {
            let (chunk, offset_ms, kind, comp) = row?;
            let plain = match key.as_ref().and_then(|k| Self::unseal_chunk(k, &chunk)) {
                Some(plain) => plain,
                None => chunk,
            };
            events.push(SessionLogEvent {
                offset_ms,
                // Unknown kinds (from a future version) degrade to
                // output rather than dropping recorded bytes.
                kind: match kind.as_str() {
                    "r" => 'r',
                    "c" => 'c',
                    _ => 'o',
                },
                data: Self::decode_chunk(plain, comp),
            });
        }
        Ok(events)
    }

    /// The typed commands of a session (`kind = 'c'` rows only), in
    /// capture order, for the input-only export. Empty for sessions
    /// recorded before command rows existed.
    pub fn get_session_commands(
        &self,
        id: &Uuid,
    ) -> Result<Vec<SessionLogEvent>, VaultError> {
        let key = self.session_log_key().ok();
        let mut stmt = self.db.prepare(
            "SELECT data, offset_ms, comp FROM session_log_chunks
             WHERE log_id = ?1 AND kind = 'c' ORDER BY id",
        )?;
        let rows = stmt.query_map(params![id.to_string()], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut events: Vec<SessionLogEvent> = Vec::new();
        for row in rows {
            let (chunk, offset_ms, comp) = row?;
            let plain = match key.as_ref().and_then(|k| Self::unseal_chunk(k, &chunk)) {
                Some(plain) => plain,
                None => chunk,
            };
            events.push(SessionLogEvent {
                offset_ms,
                kind: 'c',
                data: Self::decode_chunk(plain, comp),
            });
        }
        Ok(events)
    }

    /// Delete a session log and its recorded chunks.
    pub fn delete_session_log(&self, id: &Uuid) -> Result<(), VaultError> {
        let id_str = id.to_string();
        self.db.execute(
            "DELETE FROM session_log_chunks WHERE log_id = ?1",
            params![id_str],
        )?;
        self.db.execute(
            "DELETE FROM session_logs WHERE id = ?1",
            params![id_str],
        )?;
        Ok(())
    }

}

/// One session log's recorded output, still sealed, plus the content
/// key needed to open it. Produced by
/// [`VaultStore::sealed_session_output`] so the decrypt + inflate cost
/// can run off the UI thread while the crypto primitives never leave
/// this crate. The key is zeroized when the bundle drops.
pub struct SealedSessionOutput {
    /// `None` when the vault was locked at fetch time; sealed chunks
    /// then stay closed (pre-encryption chunks still pass through).
    key: Option<Zeroizing<[u8; KEY_LEN]>>,
    /// Legacy inline blob from before the chunk migration (plaintext).
    legacy: Vec<u8>,
    /// `(sealed data, comp flag)` per output chunk, append order.
    chunks: Vec<(Vec<u8>, i64)>,
    /// Plaintext bound applied while opening
    /// ([`CONTENT_SEARCH_MAX_SCAN_BYTES`] for scan bundles, `None`
    /// for the full readers). Enforced here too because a deflated
    /// chunk can inflate far past the ciphertext bound the fetch
    /// already applied.
    scan_cap: Option<usize>,
}

impl SealedSessionOutput {
    /// Open every chunk and return the concatenated output byte
    /// stream, byte-identical to [`VaultStore::get_session_data`]
    /// (scan bundles stop at their plaintext cap instead). Chunks
    /// that don't unseal (locked vault, pre-encryption rows) pass
    /// through as stored, mirroring the reader's best-effort
    /// contract.
    pub fn open(self) -> Vec<u8> {
        let cap = self.scan_cap.unwrap_or(usize::MAX);
        let mut buf = self.legacy;
        for (chunk, comp) in self.chunks {
            if buf.len() >= cap {
                break;
            }
            let plain = match self
                .key
                .as_ref()
                .and_then(|k| VaultStore::unseal_chunk(k, &chunk))
            {
                Some(plain) => plain,
                None => chunk,
            };
            buf.extend_from_slice(&VaultStore::decode_chunk(plain, comp));
        }
        // Hard bound: the last decoded chunk may have inflated past
        // the cap. No-op on full (uncapped) bundles.
        if buf.len() > cap {
            buf.truncate(cap);
        }
        buf
    }
}
