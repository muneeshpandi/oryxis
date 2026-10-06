use super::*;

#[test]
fn session_log_roundtrips_appended_chunks() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"first chunk\n", None, false).unwrap();
    vault.append_session_data(&log_id, b"second chunk\n", None, false).unwrap();
    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(data, b"first chunk\nsecond chunk\n");
}


#[test]
fn session_log_chunks_are_not_stored_in_the_clear() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    let marker = b"TOP-SECRET-OUTPUT-MARKER";
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, marker, None, false).unwrap();
    // Structural check straight against the column: the stored blob
    // must not contain the recorded bytes.
    let raw: Vec<u8> = vault
        .db
        .query_row(
            "SELECT data FROM session_log_chunks WHERE log_id = ?1",
            params![log_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !raw.windows(marker.len()).any(|w| w == marker),
        "recorded output stored in the clear"
    );
    // And it still reads back through the API.
    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(data, marker);
}


#[test]
fn session_commands_roundtrip_and_stay_out_of_the_output_stream() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"prompt$ ls -la\r\ntotal 0\r\n", Some(0), false).unwrap();
    vault.append_session_command(&log_id, Some(120), "ls -la").unwrap();
    vault.append_session_command(&log_id, None, "sudo apt update").unwrap();
    // Empty commands are dropped, not stored.
    vault.append_session_command(&log_id, None, "").unwrap();

    let cmds = vault.get_session_commands(&log_id).unwrap();
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].data, b"ls -la");
    assert_eq!(cmds[0].offset_ms, Some(120));
    assert_eq!(cmds[0].kind, 'c');
    assert_eq!(cmds[1].data, b"sudo apt update");
    assert_eq!(cmds[1].offset_ms, None);

    // The output byte stream must not grow command rows: replay and
    // the transcript stay exactly what the terminal printed.
    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(data, b"prompt$ ls -la\r\ntotal 0\r\n");

    // The full event stream preserves the kind so the asciicast
    // export can skip 'c' rows instead of misreading them as output.
    let events = vault.get_session_events(&log_id).unwrap();
    assert_eq!(
        events.iter().map(|e| e.kind).collect::<Vec<_>>(),
        vec!['o', 'c', 'c']
    );
}


#[test]
fn session_commands_are_not_stored_in_the_clear() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    let marker = "TOP-SECRET-COMMAND-MARKER";
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_command(&log_id, None, marker).unwrap();
    let raw: Vec<u8> = vault
        .db
        .query_row(
            "SELECT data FROM session_log_chunks WHERE log_id = ?1 AND kind = 'c'",
            params![log_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !raw.windows(marker.len()).any(|w| w == marker.as_bytes()),
        "recorded command stored in the clear"
    );
    let cmds = vault.get_session_commands(&log_id).unwrap();
    assert_eq!(cmds[0].data, marker.as_bytes());
}


#[test]
fn session_log_chunks_survive_master_password_change() {
    let mut vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"before change\n", None, false).unwrap();
    vault.set_user_password("brand-new-password").unwrap();
    // Drop the cached content key to force a re-unwrap with the new
    // master key, as a fresh process would.
    *vault.session_log_key.lock().unwrap() = None;
    vault.append_session_data(&log_id, b"after change\n", None, false).unwrap();
    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(data, b"before change\nafter change\n");
}


#[test]
fn session_log_chunks_concatenate_in_append_order() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault
        .create_session_log(&log_id, &conn_id, "web-01")
        .unwrap();

    // Append in three writes; the recorded stream must read back as
    // the exact byte-for-byte concatenation, in order.
    vault.append_session_data(&log_id, b"$ apt update\n", None, false).unwrap();
    vault.append_session_data(&log_id, b"Hit:1 http://deb\n", None, false).unwrap();
    vault.append_session_data(&log_id, b"Reading package lists\n", None, false).unwrap();
    // Empty appends are no-ops, never a stray zero-length chunk.
    vault.append_session_data(&log_id, b"", None, false).unwrap();

    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(
        data,
        b"$ apt update\nHit:1 http://deb\nReading package lists\n"
    );

    // Metadata size reflects the stored chunk bytes; each sealed
    // chunk carries a nonce + AEAD tag on top of the recording.
    let entry = vault
        .list_session_logs()
        .unwrap()
        .into_iter()
        .find(|e| e.id == log_id)
        .expect("log listed");
    assert_eq!(entry.data_size, data.len() + 3 * (NONCE_LEN + 16));
}


#[test]
fn session_log_reads_legacy_inline_blob_then_chunks() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault
        .create_session_log(&log_id, &conn_id, "legacy")
        .unwrap();
    // Simulate a row recorded before the chunk migration: bytes live
    // in the inline `data` column. New appends go to chunks; the read
    // path must stitch legacy-prefix + chunks.
    vault
        .db
        .execute(
            "UPDATE session_logs SET data = ?1 WHERE id = ?2",
            params![b"OLD".to_vec(), log_id.to_string()],
        )
        .unwrap();
    vault.append_session_data(&log_id, b"NEW", None, false).unwrap();

    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(data, b"OLDNEW");

    let entry = vault
        .list_session_logs()
        .unwrap()
        .into_iter()
        .find(|e| e.id == log_id)
        .unwrap();
    // Inline legacy bytes are raw; the appended chunk is sealed.
    assert_eq!(entry.data_size, 3 + 3 + NONCE_LEN + 16);
}


#[test]
fn deleting_session_log_drops_its_chunks() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    vault
        .create_session_log(&log_id, &Uuid::new_v4(), "doomed")
        .unwrap();
    vault.append_session_data(&log_id, b"transient", None, false).unwrap();

    vault.delete_session_log(&log_id).unwrap();

    // No orphan chunks left behind.
    let orphans: i64 = vault
        .db
        .query_row(
            "SELECT COUNT(*) FROM session_log_chunks WHERE log_id = ?1",
            params![log_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(orphans, 0);
    assert!(vault.get_session_data(&log_id).is_err());
}


#[test]
fn add_and_list_logs() {
    let vault = unlocked_vault();
    let entry = LogEntry::new("prod-web", "192.168.1.10", LogEvent::Connected, "OK");
    vault.add_log(&entry).unwrap();

    let logs = vault.list_logs(10).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].connection_label, "prod-web");
}


#[test]
fn logs_ordered_by_timestamp_desc() {
    let vault = unlocked_vault();
    vault.add_log(&LogEntry::new("first", "h1", LogEvent::Connected, "")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    vault.add_log(&LogEntry::new("second", "h2", LogEvent::Disconnected, "")).unwrap();

    let logs = vault.list_logs(10).unwrap();
    assert_eq!(logs[0].connection_label, "second"); // most recent first
}


#[test]
fn clear_logs() {
    let vault = unlocked_vault();
    vault.add_log(&LogEntry::new("x", "y", LogEvent::Error, "fail")).unwrap();
    vault.add_log(&LogEntry::new("a", "b", LogEvent::Connected, "ok")).unwrap();
    vault.clear_logs().unwrap();
    assert_eq!(vault.list_logs(100).unwrap().len(), 0);
}


#[test]
fn logs_limit_works() {
    let vault = unlocked_vault();
    for i in 0..20 {
        vault.add_log(&LogEntry::new(&format!("conn-{}", i), "h", LogEvent::Connected, "")).unwrap();
    }
    let logs = vault.list_logs(5).unwrap();
    assert_eq!(logs.len(), 5);
}

// ── MCP enabled field ──


#[test]
fn session_events_carry_offsets_and_kinds() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_resize(&log_id, 0, 120, 30).unwrap();
    vault.append_session_data(&log_id, b"$ ls\n", Some(150), false).unwrap();
    vault.append_session_data(&log_id, b"README\n", Some(400), false).unwrap();
    vault.append_session_resize(&log_id, 900, 100, 40).unwrap();

    let events = vault.get_session_events(&log_id).unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[0].kind, 'r');
    assert_eq!(events[0].offset_ms, Some(0));
    assert_eq!(events[0].data, b"120x30");
    assert_eq!(events[1].kind, 'o');
    assert_eq!(events[1].offset_ms, Some(150));
    assert_eq!(events[1].data, b"$ ls\n");
    assert_eq!(events[3].kind, 'r');
    assert_eq!(events[3].data, b"100x40");
}

#[test]
fn resize_events_stay_out_of_the_byte_stream() {
    // `get_session_data` reconstructs what the terminal received;
    // resize rows are replay metadata and must never leak into it.
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"before ", Some(10), false).unwrap();
    vault.append_session_resize(&log_id, 20, 90, 25).unwrap();
    vault.append_session_data(&log_id, b"after\n", Some(30), false).unwrap();
    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(data, b"before after\n");
}

#[test]
fn untimed_chunks_read_back_with_no_offset() {
    // Pre-migration rows have NULL offsets; the export layer gives
    // them a fixed replay delta, the store must not invent timing.
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"legacy\n", None, false).unwrap();
    let events = vault.get_session_events(&log_id).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].offset_ms, None);
    assert_eq!(events[0].kind, 'o');
}

#[test]
fn compressed_chunks_roundtrip_and_actually_shrink() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    // Repetitive terminal-ish output well above the size threshold.
    let big: Vec<u8> = b"drwxr-xr-x 2 root root 4096 Jul  4 12:00 dir\n"
        .repeat(200)
        .to_vec();
    vault.append_session_data(&log_id, &big, Some(100), true).unwrap();
    let (stored_len, comp): (i64, i64) = vault
        .db
        .query_row(
            "SELECT LENGTH(data), comp FROM session_log_chunks WHERE log_id = ?1",
            params![log_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(comp, 1);
    assert!(
        (stored_len as usize) < big.len() / 4,
        "expected real shrink, stored {stored_len} of {}",
        big.len()
    );
    // Both read paths inflate transparently.
    assert_eq!(vault.get_session_data(&log_id).unwrap().unwrap(), big);
    let events = vault.get_session_events(&log_id).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, big);
    assert_eq!(events[0].offset_ms, Some(100));
}


#[test]
fn small_chunks_stay_raw_even_with_compression_on() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"$ ls\n", None, true).unwrap();
    let comp: i64 = vault
        .db
        .query_row(
            "SELECT comp FROM session_log_chunks WHERE log_id = ?1",
            params![log_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(comp, 0, "below-threshold chunk must not pay deflate framing");
    assert_eq!(vault.get_session_data(&log_id).unwrap().unwrap(), b"$ ls\n");
}


#[test]
fn mixed_raw_and_compressed_chunks_concatenate_in_order() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    let big: Vec<u8> = b"the same line over and over\n".repeat(100).to_vec();
    // Raw (toggle off), compressed (on), tiny (on but under threshold):
    // the reader honors each row's own flag.
    vault.append_session_data(&log_id, b"before ", Some(0), false).unwrap();
    vault.append_session_data(&log_id, &big, Some(300), true).unwrap();
    vault.append_session_data(&log_id, b"after\n", Some(600), true).unwrap();
    let mut expected = b"before ".to_vec();
    expected.extend_from_slice(&big);
    expected.extend_from_slice(b"after\n");
    assert_eq!(vault.get_session_data(&log_id).unwrap().unwrap(), expected);
    let events = vault.get_session_events(&log_id).unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[1].data, big);
}


#[test]
fn search_session_commands_finds_the_right_sessions() {
    let vault = unlocked_vault();
    let conn_id = Uuid::new_v4();
    let log_a = Uuid::new_v4();
    let log_b = Uuid::new_v4();
    let log_c = Uuid::new_v4();
    vault.create_session_log(&log_a, &conn_id, "host-a").unwrap();
    vault.create_session_log(&log_b, &conn_id, "host-a").unwrap();
    vault.create_session_log(&log_c, &conn_id, "host-a").unwrap();
    vault.append_session_command(&log_a, Some(0), "kubectl get pods").unwrap();
    vault.append_session_command(&log_a, Some(50), "kubectl top nodes").unwrap();
    vault.append_session_command(&log_b, Some(0), "ls -la").unwrap();
    vault.append_session_command(&log_c, Some(0), "KUBECTL version").unwrap();

    // Case-insensitive, one hit per session (the first match).
    let mut hits = vault.search_session_commands("kubectl").unwrap();
    hits.sort_by_key(|(id, _)| *id);
    let mut expected = vec![
        (log_a, "kubectl get pods".to_string()),
        (log_c, "KUBECTL version".to_string()),
    ];
    expected.sort_by_key(|(id, _)| *id);
    assert_eq!(hits, expected);

    assert!(vault.search_session_commands("terraform").unwrap().is_empty());
    assert!(vault.search_session_commands("").unwrap().is_empty());
}


#[test]
fn scan_meta_covers_every_session_newest_first() {
    // The content search builds its output-scan queue from this
    // projection; it must walk the WHOLE table (the UI list is a
    // 50-row page window) and mirror the timeline's ordering.
    let vault = unlocked_vault();
    let conn_a = Uuid::new_v4();
    let conn_b = Uuid::new_v4();
    let mut ids = Vec::new();
    for i in 0..60 {
        let log_id = Uuid::new_v4();
        let conn = if i % 2 == 0 { conn_a } else { conn_b };
        vault
            .create_session_log(&log_id, &conn, &format!("host-{i}"))
            .unwrap();
        ids.push(log_id);
    }
    let meta = vault.list_session_log_scan_meta().unwrap();
    assert_eq!(meta.len(), 60, "every recording, not one page");
    let listed: std::collections::HashSet<Uuid> =
        meta.iter().map(|(id, _, _)| *id).collect();
    assert!(ids.iter().all(|id| listed.contains(id)));
    // Same ordering contract as the paged listing (started_at desc):
    // rows created in one burst share a second, so assert against the
    // full listing instead of insertion order.
    let full: Vec<Uuid> = vault
        .list_session_logs()
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(
        meta.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
        full
    );
    // The projection carries what the queue filter needs.
    let (_, conn, label) = meta
        .iter()
        .find(|(id, _, _)| *id == ids[0])
        .unwrap()
        .clone();
    assert_eq!(conn, conn_a);
    assert_eq!(label, "host-0");
}


#[test]
fn get_session_log_fetches_one_row_or_none() {
    // Single-row metadata fetch used to pull a matched session from
    // beyond the UI's page window into the timeline. Same projection
    // as the listing, including the aggregated data size.
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"some output\n", Some(0), false).unwrap();
    vault.end_session_log(&log_id).unwrap();

    let entry = vault.get_session_log(&log_id).unwrap().expect("row exists");
    assert_eq!(entry.id, log_id);
    assert_eq!(entry.connection_id, conn_id);
    assert_eq!(entry.label, "host-a");
    assert!(entry.ended_at.is_some());
    let listed = vault
        .list_session_logs()
        .unwrap()
        .into_iter()
        .find(|e| e.id == log_id)
        .unwrap();
    assert_eq!(entry.data_size, listed.data_size);

    assert!(vault.get_session_log(&Uuid::new_v4()).unwrap().is_none());
}


#[test]
fn sealed_session_output_opens_off_handle_byte_identical() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    let big: Vec<u8> = b"the same line over and over\n".repeat(100).to_vec();
    vault.append_session_data(&log_id, b"before ", Some(0), false).unwrap();
    vault.append_session_data(&log_id, &big, Some(300), true).unwrap();
    // Command rows must stay out of the sealed output stream too.
    vault.append_session_command(&log_id, Some(400), "secret-cmd").unwrap();

    let sealed = vault.sealed_session_output(&log_id).unwrap();
    // The whole point: the bundle is self-contained, so opening it on
    // another thread with the handle gone must reproduce the stream.
    drop(vault);
    let opened = std::thread::spawn(move || sealed.open()).join().unwrap();
    let mut expected = b"before ".to_vec();
    expected.extend_from_slice(&big);
    assert_eq!(opened, expected);
}


#[test]
fn scan_bounded_output_stops_collecting_at_the_cap() {
    // The content search's reader must never copy a runaway recording
    // wholesale: sealed-chunk collection stops at the cap while the
    // full reader (exports, viewer) stays byte-complete.
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "big-host").unwrap();
    let chunk = vec![b'x'; 1024 * 1024];
    for _ in 0..6 {
        vault.append_session_data(&log_id, &chunk, None, false).unwrap();
    }

    let full = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(full.len(), 6 * 1024 * 1024, "full reader stays unbounded");

    let scanned = vault.sealed_session_output_scan(&log_id).unwrap().open();
    assert_eq!(
        scanned.len(),
        CONTENT_SEARCH_MAX_SCAN_BYTES,
        "scan reader stops at the cap"
    );
    // What survives the cap is the stream's head, decrypted intact.
    assert!(scanned.iter().all(|b| *b == b'x'));
}


#[test]
fn scan_bounded_output_caps_inflation_of_compressed_chunks() {
    // A deflated chunk is tiny at rest but can inflate far past the
    // ciphertext bound the fetch applied, so the plaintext cap in
    // open() must hold on its own.
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "compressed-host").unwrap();
    let huge = vec![b'y'; 8 * 1024 * 1024];
    vault.append_session_data(&log_id, &huge, None, true).unwrap();
    // Sanity: the chunk really was stored deflated (well under the
    // scan cap at rest), otherwise this test exercises nothing.
    let stored: i64 = vault
        .db
        .query_row(
            "SELECT LENGTH(data) FROM session_log_chunks WHERE log_id = ?1",
            params![log_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!((stored as usize) < CONTENT_SEARCH_MAX_SCAN_BYTES);

    let scanned = vault.sealed_session_output_scan(&log_id).unwrap().open();
    assert_eq!(
        scanned.len(),
        CONTENT_SEARCH_MAX_SCAN_BYTES,
        "inflated plaintext is truncated to the cap"
    );
    assert!(scanned.iter().all(|b| *b == b'y'));
    // The unbounded readers still see the whole stream.
    let full = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(full.len(), huge.len());
}

/// What every chunk of `log_id` occupies, measured the slow way: the
/// figure the trigger-kept `bytes` column must always agree with.
fn chunk_bytes(vault: &VaultStore, log_id: &Uuid) -> i64 {
    vault
        .db
        .query_row(
            "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM session_log_chunks WHERE log_id = ?1",
            params![log_id.to_string()],
            |row| row.get(0),
        )
        .unwrap()
}

fn bytes_column(vault: &VaultStore, log_id: &Uuid) -> i64 {
    vault
        .db
        .query_row(
            "SELECT bytes FROM session_logs WHERE id = ?1",
            params![log_id.to_string()],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn session_log_size_follows_every_chunk_write() {
    let vault = unlocked_vault();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    vault.create_session_log(&a, &Uuid::new_v4(), "host-a").unwrap();
    vault.create_session_log(&b, &Uuid::new_v4(), "host-b").unwrap();
    assert_eq!(bytes_column(&vault, &a), 0);

    vault.append_session_data(&a, b"plain output\n", Some(0), false).unwrap();
    vault.append_session_data(&a, &[b'z'; 4096], Some(5), true).unwrap();
    vault.append_session_resize(&a, 9, 120, 40).unwrap();
    vault.append_session_command(&a, Some(10), "ls -la").unwrap();
    vault.append_session_data(&b, b"other host\n", None, false).unwrap();
    assert!(chunk_bytes(&vault, &a) > 0);
    assert_eq!(bytes_column(&vault, &a), chunk_bytes(&vault, &a));
    assert_eq!(bytes_column(&vault, &b), chunk_bytes(&vault, &b));

    // Every reader reports the column, and the total is their sum.
    let listed = vault.list_session_logs_page(0, 50).unwrap();
    let size_of = |id: &Uuid| listed.iter().find(|e| e.id == *id).unwrap().data_size as i64;
    assert_eq!(size_of(&a), chunk_bytes(&vault, &a));
    assert_eq!(vault.get_session_log(&b).unwrap().unwrap().data_size as i64, chunk_bytes(&vault, &b));
    assert_eq!(
        vault.session_logs_total_bytes().unwrap() as i64,
        chunk_bytes(&vault, &a) + chunk_bytes(&vault, &b)
    );

    // A rewritten chunk (key rotation's shape) moves the size with it.
    vault
        .db
        .execute(
            "UPDATE session_log_chunks SET data = X'0102' WHERE log_id = ?1 AND kind = 'r'",
            params![a.to_string()],
        )
        .unwrap();
    assert_eq!(bytes_column(&vault, &a), chunk_bytes(&vault, &a));

    vault.delete_session_log(&a).unwrap();
    assert_eq!(vault.session_logs_total_bytes().unwrap() as i64, chunk_bytes(&vault, &b));
    vault.clear_session_logs().unwrap();
    assert_eq!(vault.session_logs_total_bytes().unwrap(), 0);
}

#[test]
fn session_log_size_survives_both_prunes() {
    let vault = unlocked_vault();
    let old = Uuid::new_v4();
    let live = Uuid::new_v4();
    vault.create_session_log(&old, &Uuid::new_v4(), "old").unwrap();
    vault.append_session_data(&old, &[b'o'; 2048], None, false).unwrap();
    vault.end_session_log(&old).unwrap();
    vault
        .db
        .execute(
            "UPDATE session_logs SET started_at = '2020-01-01T00:00:00+00:00' WHERE id = ?1",
            params![old.to_string()],
        )
        .unwrap();
    vault.create_session_log(&live, &Uuid::new_v4(), "live").unwrap();
    vault.append_session_data(&live, &[b'l'; 1024], None, false).unwrap();

    vault.prune_logs_older_than(chrono::Utc::now() - chrono::Duration::days(1)).unwrap();
    assert_eq!(vault.session_logs_total_bytes().unwrap() as i64, chunk_bytes(&vault, &live));

    // The size cap counts the same column: a cap below the live log
    // has nothing finished left to drop and reports so.
    let total = vault.session_logs_total_bytes().unwrap();
    assert_eq!(vault.prune_session_logs_to_fit(total).unwrap(), 0);
    assert_eq!(bytes_column(&vault, &live), chunk_bytes(&vault, &live));
}

#[test]
fn session_log_size_is_backfilled_on_a_vault_that_predates_it() {
    let mut vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &Uuid::new_v4(), "host-a").unwrap();
    // Back to the shape of a vault written before the column existed:
    // no triggers, no column, chunks already on disk.
    vault
        .db
        .execute_batch(
            "DROP TRIGGER session_log_bytes_insert;
             DROP TRIGGER session_log_bytes_delete;
             DROP TRIGGER session_log_bytes_update;
             ALTER TABLE session_logs DROP COLUMN bytes;",
        )
        .unwrap();
    vault.append_session_data(&log_id, b"recorded by an older build\n", None, false).unwrap();
    vault.append_session_data(&log_id, &[b'q'; 3000], None, true).unwrap();
    let before = chunk_bytes(&vault, &log_id);
    assert!(before > 0);

    vault.create_tables().unwrap();
    assert_eq!(bytes_column(&vault, &log_id), before);
    // And the triggers are back for what comes next.
    vault.append_session_data(&log_id, b"recorded after\n", None, false).unwrap();
    assert_eq!(bytes_column(&vault, &log_id), chunk_bytes(&vault, &log_id));
    // Opening again never re-runs the backfill over a counted column.
    vault.create_tables().unwrap();
    assert_eq!(bytes_column(&vault, &log_id), chunk_bytes(&vault, &log_id));
}
