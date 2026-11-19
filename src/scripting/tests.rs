use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::session_service::{PortWriter, SessionService};

use super::{
    SCRIPT_LOG_MAX_LINES, ScriptEvent, ScriptInstanceSnapshot, ScriptLogLevel, ScriptManager,
    ScriptStatus, StopReason,
};

struct RecordingPortWriter {
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl RecordingPortWriter {
    fn new() -> (Self, Arc<Mutex<Vec<Vec<u8>>>>) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                writes: Arc::clone(&writes),
            },
            writes,
        )
    }
}

impl PortWriter for RecordingPortWriter {
    fn write_all_and_flush(&mut self, data: &[u8]) -> Result<(), String> {
        self.writes.lock().unwrap().push(data.to_vec());
        Ok(())
    }
}

fn setup_connected_service() -> (SessionService, u64, Arc<Mutex<Vec<Vec<u8>>>>) {
    let service = SessionService::new();
    let (writer, writes) = RecordingPortWriter::new();
    let generation = service.attach_writer(writer);
    (service, generation, writes)
}

fn wait_until_finished(
    manager: &ScriptManager,
    run_id: u64,
    timeout: Duration,
) -> ScriptInstanceSnapshot {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(snap) = manager.snapshots().into_iter().find(|s| s.run_id == run_id)
            && !snap.status.is_active()
        {
            return snap;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for script run_id={run_id} to finish"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_until_log_contains(manager: &ScriptManager, run_id: u64, needle: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(snap) = manager.snapshots().into_iter().find(|s| s.run_id == run_id)
            && snap.logs.iter().any(|entry| entry.message.contains(needle))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for log '{needle}' on run_id={run_id}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn send_and_send_hex_transmit_expected_bytes_and_reject_invalid_hex() {
    let (service, generation, writes) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let source = r#"
        serial.send("AT\r\n")
        serial.write("\0\xFF")
        serial.send_hex("AA bb 01 0D0A")
    "#
    .to_string();

    let run_id = manager
        .start_script(PathBuf::from("tx_test.lua"), source, &service, generation)
        .unwrap();
    let snap = wait_until_finished(&manager, run_id, Duration::from_secs(3));
    assert_eq!(snap.status, ScriptStatus::Completed);

    let recorded = writes.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec![
            b"AT\r\n".to_vec(),
            vec![0x00, 0xFF],
            vec![0xAA, 0xBB, 0x01, 0x0D, 0x0A],
        ]
    );

    // Invalid hex must fail with a parameter error.
    for bad_hex in ["", "   ", "A BB", "ZZ 01"] {
        let bad_source = format!("serial.send_hex({bad_hex:?})");
        let bad_id = manager
            .start_script(
                PathBuf::from("bad_hex.lua"),
                bad_source,
                &service,
                generation,
            )
            .unwrap();
        let bad_snap = wait_until_finished(&manager, bad_id, Duration::from_secs(3));
        assert!(
            matches!(bad_snap.status, ScriptStatus::Failed { .. }),
            "expected Failed for bad_hex={bad_hex:?}, got {:?}",
            bad_snap.status
        );
    }
}

#[test]
fn read_preserves_binary_bytes_and_returns_timeout_pair() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let source = r#"
        local none, err = serial.read(8, 30)
        assert(none == nil and err == "timeout")

        log.info("ready")
        local first = serial.read(2, 1000)
        assert(first ~= nil and #first == 2)
        assert(string.byte(first, 1) == 0x00 and string.byte(first, 2) == 0xFF)

        local rest = serial.read(nil, 1000)
        assert(rest ~= nil and #rest == 2)
        assert(string.byte(rest, 1) == 0xFE and string.byte(rest, 2) == 0x01)
    "#
    .to_string();

    let run_id = manager
        .start_script(
            PathBuf::from("binary_read.lua"),
            source,
            &service,
            generation,
        )
        .unwrap();
    wait_until_log_contains(&manager, run_id, "ready", Duration::from_secs(2));

    service.publish_rx(generation, &[0x00, 0xFF, 0xFE, 0x01]);

    let snap = wait_until_finished(&manager, run_id, Duration::from_secs(3));
    assert_eq!(snap.status, ScriptStatus::Completed);
}

#[test]
fn wait_text_matches_across_chunks_and_preserves_trailing_bytes() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let source = r#"
        log.info("ready")
        local matched = serial.wait_text("OK\r\n", 1500)
        assert(matched == "BOOT\r\nOK\r\n", "unexpected match: " .. tostring(matched))

        local trailing = serial.read(nil, 200)
        assert(trailing == "TRAIL", "unexpected trailing: " .. tostring(trailing))

        local miss, err = serial.wait_text("NOPE", 30)
        assert(miss == nil and err == "timeout")

        serial.clear_rx()
        local empty, err2 = serial.read(nil, 20)
        assert(empty == nil and err2 == "timeout")
    "#
    .to_string();

    let run_id = manager
        .start_script(PathBuf::from("wait_text.lua"), source, &service, generation)
        .unwrap();
    wait_until_log_contains(&manager, run_id, "ready", Duration::from_secs(2));

    // Split "OK\r\n" across two chunks and include trailing bytes in the second chunk.
    service.publish_rx(generation, b"BOOT\r\nO");
    thread::sleep(Duration::from_millis(20));
    service.publish_rx(generation, b"K\r\nTRAIL");

    let snap = wait_until_finished(&manager, run_id, Duration::from_secs(3));
    assert_eq!(snap.status, ScriptStatus::Completed);
}

#[test]
fn log_and_print_are_bounded_and_emit_events() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let source = format!(
        r#"
        print("hello", 42)
        log.warn("careful")
        log.error("boom")
        for i = 1, {count} do
            log.info("line-" .. i)
        end
        "#,
        count = SCRIPT_LOG_MAX_LINES + 50
    );

    let run_id = manager
        .start_script(PathBuf::from("logs.lua"), source, &service, generation)
        .unwrap();
    let snap = wait_until_finished(&manager, run_id, Duration::from_secs(3));
    assert_eq!(snap.status, ScriptStatus::Completed);
    assert_eq!(snap.logs.len(), SCRIPT_LOG_MAX_LINES);
    assert_eq!(snap.logs.first().unwrap().message, "line-51");
    assert_eq!(
        snap.logs.last().unwrap().message,
        format!("line-{}", SCRIPT_LOG_MAX_LINES + 50)
    );

    let events = manager.drain_events();
    assert!(events.iter().any(|ev| matches!(
        ev,
        ScriptEvent::Log(entry)
            if entry.run_id == run_id
                && entry.level == ScriptLogLevel::Warn
                && entry.message == "careful"
    )));
    assert!(events.iter().any(|ev| matches!(
        ev,
        ScriptEvent::Log(entry)
            if entry.run_id == run_id
                && entry.level == ScriptLogLevel::Error
                && entry.message == "boom"
    )));
}

#[test]
fn unsafe_stdlib_and_dynamic_loaders_are_disabled() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let source = r#"
        assert(os == nil, "os must be nil")
        assert(io == nil, "io must be nil")
        assert(package == nil, "package must be nil")
        assert(debug == nil, "debug must be nil")
        assert(dofile == nil, "dofile must be nil")
        assert(loadfile == nil, "loadfile must be nil")
        assert(load == nil, "load must be nil")
        assert(string.upper("ok") == "OK")
        assert(math.abs(-5) == 5)
        assert(utf8.len("你好") == 2)
    "#
    .to_string();

    let run_id = manager
        .start_script(PathBuf::from("sandbox.lua"), source, &service, generation)
        .unwrap();
    let snap = wait_until_finished(&manager, run_id, Duration::from_secs(3));
    assert_eq!(snap.status, ScriptStatus::Completed);
}

#[test]
fn multiple_instances_observe_same_rx_without_stealing() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let source_a = r#"
        log.info("ready_a")
        local m = serial.wait_text("PING", 1500)
        assert(m == "PING")
        log.info("got_a")
    "#
    .to_string();
    let source_b = r#"
        log.info("ready_b")
        local m = serial.wait_text("PING", 1500)
        assert(m == "PING")
        log.info("got_b")
    "#
    .to_string();

    let id_a = manager
        .start_script(PathBuf::from("a.lua"), source_a, &service, generation)
        .unwrap();
    let id_b = manager
        .start_script(PathBuf::from("b.lua"), source_b, &service, generation)
        .unwrap();

    wait_until_log_contains(&manager, id_a, "ready_a", Duration::from_secs(2));
    wait_until_log_contains(&manager, id_b, "ready_b", Duration::from_secs(2));

    service.publish_rx(generation, b"PING");

    let snap_a = wait_until_finished(&manager, id_a, Duration::from_secs(3));
    let snap_b = wait_until_finished(&manager, id_b, Duration::from_secs(3));
    assert_eq!(snap_a.status, ScriptStatus::Completed);
    assert_eq!(snap_b.status, ScriptStatus::Completed);
}

#[test]
fn cancellation_stops_infinite_loop_sleep_and_wait_text() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let loop_id = manager
        .start_script(
            PathBuf::from("loop.lua"),
            "log.info('started')\nwhile true do end".to_string(),
            &service,
            generation,
        )
        .unwrap();
    let sleep_id = manager
        .start_script(
            PathBuf::from("sleep.lua"),
            "log.info('started')\ntask.sleep(10000)".to_string(),
            &service,
            generation,
        )
        .unwrap();
    let wait_id = manager
        .start_script(
            PathBuf::from("wait.lua"),
            "log.info('started')\nserial.wait_text('NEVER', 10000)".to_string(),
            &service,
            generation,
        )
        .unwrap();

    wait_until_log_contains(&manager, loop_id, "started", Duration::from_secs(2));
    wait_until_log_contains(&manager, sleep_id, "started", Duration::from_secs(2));
    wait_until_log_contains(&manager, wait_id, "started", Duration::from_secs(2));

    assert_eq!(manager.running_count(), 3);
    assert!(manager.stop_instance(loop_id));
    assert!(manager.stop_instance(sleep_id));
    assert!(manager.stop_instance(wait_id));

    for id in [loop_id, sleep_id, wait_id] {
        let snap = wait_until_finished(&manager, id, Duration::from_secs(2));
        assert_eq!(
            snap.status,
            ScriptStatus::Stopped {
                reason: StopReason::User
            }
        );
    }
    assert_eq!(manager.running_count(), 0);
}

#[test]
fn disconnect_stops_all_instances_and_rejects_stale_generation() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let sleep_id = manager
        .start_script(
            PathBuf::from("sleep.lua"),
            "log.info('started')\ntask.sleep(10000)".to_string(),
            &service,
            generation,
        )
        .unwrap();
    let wait_id = manager
        .start_script(
            PathBuf::from("wait.lua"),
            "log.info('started')\nserial.wait_text('NEVER', 10000)".to_string(),
            &service,
            generation,
        )
        .unwrap();

    wait_until_log_contains(&manager, sleep_id, "started", Duration::from_secs(2));
    wait_until_log_contains(&manager, wait_id, "started", Duration::from_secs(2));

    service.disconnect(Some("cable unplugged".to_string()));

    for id in [sleep_id, wait_id] {
        let snap = wait_until_finished(&manager, id, Duration::from_secs(2));
        assert_eq!(
            snap.status,
            ScriptStatus::Stopped {
                reason: StopReason::Disconnected
            }
        );
    }

    // Starting with disconnected service or stale generation must be rejected.
    assert!(
        manager
            .start_script(
                PathBuf::from("after_disconnect.lua"),
                "serial.send('AT')".to_string(),
                &service,
                generation,
            )
            .is_err()
    );

    let (writer2, _writes2) = RecordingPortWriter::new();
    let gen2 = service.attach_writer(writer2);
    assert_ne!(gen2, generation);
    assert!(
        manager
            .start_script(
                PathBuf::from("stale_gen.lua"),
                "serial.send('AT')".to_string(),
                &service,
                generation,
            )
            .is_err()
    );
}

#[test]
fn errors_and_rx_buffer_overflow_are_isolated_per_instance() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let healthy_id = manager
        .start_script(
            PathBuf::from("healthy.lua"),
            "log.info('ready')\nlocal m = serial.wait_text('OK', 2000)\nassert(m ~= nil)"
                .to_string(),
            &service,
            generation,
        )
        .unwrap();
    let syntax_err_id = manager
        .start_script(
            PathBuf::from("syntax.lua"),
            "if true then".to_string(),
            &service,
            generation,
        )
        .unwrap();
    let runtime_err_id = manager
        .start_script(
            PathBuf::from("runtime.lua"),
            "error('intentional boom')".to_string(),
            &service,
            generation,
        )
        .unwrap();
    let overflow_id = manager
        .start_script(
            PathBuf::from("overflow.lua"),
            "log.info('ready')\nserial.wait_text('NEVER', 3000)".to_string(),
            &service,
            generation,
        )
        .unwrap();

    wait_until_log_contains(&manager, healthy_id, "ready", Duration::from_secs(2));
    wait_until_log_contains(&manager, overflow_id, "ready", Duration::from_secs(2));

    let syntax_snap = wait_until_finished(&manager, syntax_err_id, Duration::from_secs(2));
    assert!(matches!(syntax_snap.status, ScriptStatus::Failed { .. }));

    let runtime_snap = wait_until_finished(&manager, runtime_err_id, Duration::from_secs(2));
    assert!(matches!(
        runtime_snap.status,
        ScriptStatus::Failed { ref error } if error.contains("intentional boom")
    ));

    // Push > 1 MiB in chunks that fit within RxSubscriber's chunk limit to trigger RxBuffer overflow
    // on `overflow_id` (or RxSubscriber overflow), then send "OK" for `healthy_id`?
    // Note: `healthy_id` also receives those bytes! If we send > 1 MiB, `healthy_id` would also
    // overflow if it doesn't consume them. So let's let `healthy_id` finish with "OK" first, OR
    // have `healthy_id` drain via `serial.read`! Even better: let `healthy_id` complete after
    // `syntax_err_id` and `runtime_err_id` failed, and ALSO test a second healthy script that
    // reads in a loop while `overflow_id` never consumes and overflows!
    service.publish_rx(generation, b"OK");
    let healthy_snap = wait_until_finished(&manager, healthy_id, Duration::from_secs(2));
    assert_eq!(healthy_snap.status, ScriptStatus::Completed);

    let drainer_id = manager
        .start_script(
            PathBuf::from("drainer.lua"),
            r#"
            local total = 0
            while total < 1100000 do
                local chunk = serial.read(nil, 1000)
                if not chunk then break end
                total = total + #chunk
            end
            assert(total >= 1100000)
            "#
            .to_string(),
            &service,
            generation,
        )
        .unwrap();

    let chunk = vec![b'A'; 64 * 1024];
    for _ in 0..18 {
        service.publish_rx(generation, &chunk);
        thread::sleep(Duration::from_millis(5));
    }

    let overflow_snap = wait_until_finished(&manager, overflow_id, Duration::from_secs(3));
    assert!(
        matches!(
            overflow_snap.status,
            ScriptStatus::Failed { ref error } if error.contains("overflow")
        ),
        "expected overflow failure, got {:?}",
        overflow_snap.status
    );

    let drainer_snap = wait_until_finished(&manager, drainer_id, Duration::from_secs(3));
    assert_eq!(drainer_snap.status, ScriptStatus::Completed);
}

#[test]
fn checksum_module_computes_values_and_appends_in_wire_order() {
    let (service, generation, writes) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let source = r#"
        assert(checksum.crc16_modbus("123456789") == 0x4B37)
        assert(checksum.crc32("123456789") == 0xCBF43926)
        assert(checksum.crc8("123456789") == 0xF4)
        assert(checksum.sum8("\xFF\x02") == 1)
        assert(checksum.xor8("\x0F\xF0\x01") == 0xFE)
        assert(checksum.crc16_ccitt("123456789") == 0x29B1)
        serial.send(checksum.append("\x01\x03\x00\x00\x00\x0A", "crc16_modbus"))
        assert(not pcall(checksum.append, "x", "nope"))
        assert(not pcall(checksum.crc8, 5))
    "#
    .to_string();

    let run_id = manager
        .start_script(PathBuf::from("checksum.lua"), source, &service, generation)
        .unwrap();
    let snap = wait_until_finished(&manager, run_id, Duration::from_secs(3));
    assert_eq!(snap.status, ScriptStatus::Completed);
    assert_eq!(
        writes.lock().unwrap().clone(),
        vec![vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0xC5, 0xCD]]
    );
}
