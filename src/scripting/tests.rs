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

#[test]
fn huge_timeouts_do_not_panic_and_remain_cancellable() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let ids: Vec<u64> = [
        "log.info('started')\ntask.sleep(1e18)",
        "log.info('started')\nserial.read(nil, 1e18)",
        "log.info('started')\nserial.wait_text('NEVER', 1e18)",
    ]
    .into_iter()
    .map(|src| {
        manager
            .start_script(
                PathBuf::from("huge.lua"),
                src.to_string(),
                &service,
                generation,
            )
            .unwrap()
    })
    .collect();

    for id in &ids {
        wait_until_log_contains(&manager, *id, "started", Duration::from_secs(2));
    }
    thread::sleep(Duration::from_millis(100));
    assert_eq!(manager.running_count(), 3, "no script may have panicked");
    for id in &ids {
        assert!(manager.stop_instance(*id));
    }
    for id in ids {
        let snap = wait_until_finished(&manager, id, Duration::from_secs(2));
        assert_eq!(
            snap.status,
            ScriptStatus::Stopped {
                reason: StopReason::User
            }
        );
    }
}

#[test]
fn pcall_cannot_swallow_cancellation() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let source = r#"
        log.info('started')
        local ok = pcall(function() return 1 end)
        assert(ok == true)
        local ok2, v = pcall(function(a) return a * 2 end, 21)
        assert(ok2 and v == 42)
        local ok3 = pcall(error, "plain failure")
        assert(ok3 == false)
        local co = coroutine.create(function(x) return x end)
        local ok4, r = coroutine.resume(co, 7)
        assert(ok4 and r == 7)
        while true do
            pcall(task.sleep, 20)
            xpcall(task.sleep, print, 20)
            coroutine.resume(coroutine.create(function() task.sleep(20) end))
        end
    "#;
    let run_id = manager
        .start_script(
            PathBuf::from("pcall.lua"),
            source.to_string(),
            &service,
            generation,
        )
        .unwrap();
    wait_until_log_contains(&manager, run_id, "started", Duration::from_secs(2));
    thread::sleep(Duration::from_millis(100));
    assert!(manager.stop_instance(run_id));
    let snap = wait_until_finished(&manager, run_id, Duration::from_secs(2));
    assert_eq!(
        snap.status,
        ScriptStatus::Stopped {
            reason: StopReason::User
        }
    );
}

#[test]
fn event_queue_is_bounded_but_keeps_lifecycle_events() {
    let (service, generation, _) = setup_connected_service();
    let mut manager = ScriptManager::new();

    let source = format!(
        "for i = 1, {} do log.info('x') end",
        super::SCRIPT_EVENT_QUEUE_MAX + 500
    );
    let run_id = manager
        .start_script(PathBuf::from("flood.lua"), source, &service, generation)
        .unwrap();
    let snap = wait_until_finished(&manager, run_id, Duration::from_secs(5));
    assert_eq!(snap.status, ScriptStatus::Completed);

    let events = manager.drain_events();
    assert!(events.len() <= super::SCRIPT_EVENT_QUEUE_MAX + 2);
    assert!(
        events
            .iter()
            .any(|ev| matches!(ev, ScriptEvent::Started { .. }))
    );
    assert!(events.iter().any(|ev| matches!(
        ev,
        ScriptEvent::StatusChanged {
            status: ScriptStatus::Completed,
            ..
        }
    )));
}

mod monitor_tests {
    use super::*;
    use crate::scripting::MonitorRequest;
    use std::sync::mpsc::Receiver;

    type Records = Arc<Mutex<Vec<String>>>;

    /// Plays the UI thread: records each request and answers `is_paused` / `save`.
    /// With `reply_to_save == false` it keeps the save reply open without answering.
    fn spawn_fake_ui(
        rx: Receiver<MonitorRequest>,
        paused: bool,
        save_result: Result<(), String>,
        reply_to_save: bool,
    ) -> Records {
        let records: Records = Arc::new(Mutex::new(Vec::new()));
        let thread_records = Arc::clone(&records);
        thread::spawn(move || {
            let mut held = Vec::new();
            while let Ok(request) = rx.recv() {
                let mut log = thread_records.lock().unwrap();
                match request {
                    MonitorRequest::Pause => log.push("pause".to_string()),
                    MonitorRequest::Resume => log.push("resume".to_string()),
                    MonitorRequest::Clear => log.push("clear".to_string()),
                    MonitorRequest::IsPaused { reply } => {
                        log.push("is_paused".to_string());
                        let _ = reply.send(paused);
                    }
                    MonitorRequest::Save {
                        rel_path, reply, ..
                    } => {
                        log.push(format!("save:{}", rel_path.display()));
                        if reply_to_save {
                            let _ = reply.send(save_result.clone());
                        } else {
                            held.push(reply);
                        }
                    }
                }
            }
        });
        records
    }

    fn wait_for_records(records: &Records, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while records.lock().unwrap().len() < count {
            assert!(Instant::now() < deadline, "timed out waiting for records");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn run(
        manager: &mut ScriptManager,
        service: &SessionService,
        generation: u64,
        src: &str,
    ) -> u64 {
        manager
            .start_script(
                PathBuf::from("monitor.lua"),
                src.to_string(),
                service,
                generation,
            )
            .unwrap()
    }

    fn log_messages(snap: &ScriptInstanceSnapshot) -> Vec<String> {
        snap.logs
            .iter()
            .map(|entry| entry.message.clone())
            .collect()
    }

    #[test]
    fn monitor_calls_are_forwarded_in_order() {
        let (service, generation, _writes) = setup_connected_service();
        let mut manager = ScriptManager::new();
        let records = spawn_fake_ui(manager.enable_monitor(), false, Ok(()), true);
        let id = run(
            &mut manager,
            &service,
            generation,
            "monitor.pause() monitor.clear() monitor.resume()",
        );
        let snap = wait_until_finished(&manager, id, Duration::from_secs(3));
        assert_eq!(snap.status, ScriptStatus::Completed);
        wait_for_records(&records, 3);
        assert_eq!(*records.lock().unwrap(), vec!["pause", "clear", "resume"]);
    }

    #[test]
    fn monitor_is_paused_returns_ui_reply() {
        let (service, generation, _writes) = setup_connected_service();
        let mut manager = ScriptManager::new();
        let _records = spawn_fake_ui(manager.enable_monitor(), true, Ok(()), true);
        let id = run(
            &mut manager,
            &service,
            generation,
            r#"log.info("paused=" .. tostring(monitor.is_paused()))"#,
        );
        let snap = wait_until_finished(&manager, id, Duration::from_secs(3));
        assert_eq!(snap.status, ScriptStatus::Completed);
        assert!(log_messages(&snap).contains(&"paused=true".to_string()));
    }

    #[test]
    fn monitor_save_returns_true_or_nil_and_message() {
        let (service, generation, _writes) = setup_connected_service();
        let src = r#"local ok, err = monitor.save("a.txt") log.info("saved=" .. tostring(ok) .. " " .. tostring(err))"#;

        let mut manager = ScriptManager::new();
        let records = spawn_fake_ui(manager.enable_monitor(), false, Ok(()), true);
        let id = run(&mut manager, &service, generation, src);
        let snap = wait_until_finished(&manager, id, Duration::from_secs(3));
        assert_eq!(snap.status, ScriptStatus::Completed);
        assert!(log_messages(&snap).contains(&"saved=true nil".to_string()));
        assert_eq!(*records.lock().unwrap(), vec!["save:a.txt"]);

        let mut failing = ScriptManager::new();
        let _records = spawn_fake_ui(
            failing.enable_monitor(),
            false,
            Err("disk full".into()),
            true,
        );
        let id = run(&mut failing, &service, generation, src);
        let snap = wait_until_finished(&failing, id, Duration::from_secs(3));
        assert_eq!(snap.status, ScriptStatus::Completed);
        assert!(log_messages(&snap).contains(&"saved=nil disk full".to_string()));
    }

    #[test]
    fn monitor_save_rejects_bad_paths_without_contacting_ui() {
        let (service, generation, _writes) = setup_connected_service();
        let mut manager = ScriptManager::new();
        let records = spawn_fake_ui(manager.enable_monitor(), false, Ok(()), true);
        let sources = [
            r#"monitor.save("")"#,
            r#"monitor.save("../x")"#,
            r#"monitor.save("a/../../x")"#,
            r#"monitor.save("/x")"#,
            r#"monitor.save("C:\\x")"#,
            r#"monitor.save("\\\\srv\\sh\\x")"#,
            r#"monitor.save("a.txt:stream")"#,
            r#"monitor.save("a\0b")"#,
            "monitor.save(nil)",
            "monitor.save(5)",
        ];
        for src in sources {
            let id = run(&mut manager, &service, generation, src);
            let snap = wait_until_finished(&manager, id, Duration::from_secs(3));
            match snap.status {
                ScriptStatus::Failed { ref error } => assert!(
                    error.contains("bad argument #1 to 'monitor.save'"),
                    "{src}: unexpected error {error}"
                ),
                other => panic!("{src}: expected Failed, got {other:?}"),
            }
        }
        thread::sleep(Duration::from_millis(100));
        assert!(records.lock().unwrap().is_empty());
    }

    #[test]
    fn monitor_save_is_interrupted_by_stop() {
        let (service, generation, _writes) = setup_connected_service();
        let mut manager = ScriptManager::new();
        let records = spawn_fake_ui(manager.enable_monitor(), false, Ok(()), false);
        let id = run(
            &mut manager,
            &service,
            generation,
            r#"monitor.save("a.txt")"#,
        );
        wait_for_records(&records, 1);
        assert!(manager.stop_instance(id));
        let snap = wait_until_finished(&manager, id, Duration::from_secs(3));
        assert_eq!(
            snap.status,
            ScriptStatus::Stopped {
                reason: StopReason::User
            }
        );
    }

    #[test]
    fn monitor_reports_unavailable_when_ui_receiver_dropped() {
        let (service, generation, _writes) = setup_connected_service();
        let mut manager = ScriptManager::new();
        drop(manager.enable_monitor());
        let id = run(
            &mut manager,
            &service,
            generation,
            r#"local ok, err = pcall(monitor.save, "a.txt") log.info("ok=" .. tostring(ok) .. " " .. tostring(err))"#,
        );
        let snap = wait_until_finished(&manager, id, Duration::from_secs(3));
        assert_eq!(snap.status, ScriptStatus::Completed);
        assert!(
            log_messages(&snap)
                .iter()
                .any(|m| m.starts_with("ok=false") && m.contains("unavailable"))
        );
    }

    #[test]
    fn monitor_requests_are_bounded_and_script_blocks_when_ui_is_slow() {
        let (service, generation, _writes) = setup_connected_service();
        let mut manager = ScriptManager::new();
        let _rx = manager.enable_monitor(); // never drained: a stalled UI
        let id = run(
            &mut manager,
            &service,
            generation,
            r#"for i = 1, 100000 do monitor.clear() end log.info("done")"#,
        );
        thread::sleep(Duration::from_millis(300));
        let snap = manager
            .snapshots()
            .into_iter()
            .find(|s| s.run_id == id)
            .unwrap();
        assert!(
            snap.status.is_active(),
            "script should be blocked on a full queue"
        );
        assert!(!log_messages(&snap).contains(&"done".to_string()));
        assert!(manager.stop_instance(id));
        let snap = wait_until_finished(&manager, id, Duration::from_secs(3));
        assert_eq!(
            snap.status,
            ScriptStatus::Stopped {
                reason: StopReason::User
            }
        );
    }

    #[test]
    fn monitor_is_not_supported_without_enable_monitor() {
        let (service, generation, _writes) = setup_connected_service();
        let mut manager = ScriptManager::new();
        let id = run(
            &mut manager,
            &service,
            generation,
            r#"
            for _, name in ipairs({"pause", "resume", "clear", "is_paused"}) do
                local ok, err = pcall(monitor[name])
                log.info(name .. ":" .. tostring(ok) .. ":" .. tostring(err))
            end
            local ok, err = pcall(monitor.save, "a.txt")
            log.info("save:" .. tostring(ok) .. ":" .. tostring(err))
            "#,
        );
        let snap = wait_until_finished(&manager, id, Duration::from_secs(3));
        assert_eq!(snap.status, ScriptStatus::Completed);
        let messages = log_messages(&snap);
        assert_eq!(messages.len(), 5);
        for message in messages {
            assert!(
                message.contains(":false:") && message.contains("not supported in CLI mode"),
                "{message}"
            );
        }
    }
}
