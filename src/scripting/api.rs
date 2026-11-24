use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TrySendError};
use std::thread;
use std::time::Duration;

use mlua::{Lua, MultiValue, Value};

use crate::session_service::SessionTxError;

use super::monitor::validate_relative_path;
use super::runtime::{ScriptAbort, ScriptContext, deadline_after};
use super::{DEFAULT_RX_TIMEOUT_MS, MonitorRequest, ScriptLogLevel};

/// Registers `serial`, `task`, `log`, and global `print` into the Lua VM.
pub(crate) fn register_api(lua: &Lua, ctx: ScriptContext) -> mlua::Result<()> {
    register_serial_module(lua, &ctx)?;
    register_task_module(lua, &ctx)?;
    register_log_module(lua, &ctx)?;
    register_checksum_module(lua)?;
    register_monitor_module(lua, &ctx)?;
    register_time_module(lua)?;
    register_output_module(lua, &ctx)?;
    Ok(())
}

/// `output.print(...)`: content the script wants the user to see. Unlike `log.*` and `print`
/// (debug logs), hosts always show it.
fn register_output_module(lua: &Lua, ctx: &ScriptContext) -> mlua::Result<()> {
    let output = lua.create_table()?;
    let print_ctx = ctx.clone();
    output.set(
        "print",
        lua.create_function(move |_, args: MultiValue| {
            let message = format_lua_args(args)?;
            print_ctx.append_output(message);
            Ok(())
        })?,
    )?;
    lua.globals().set("output", output)?;
    Ok(())
}

/// Reads an optional epoch-milliseconds argument (`nil` means "now").
fn parse_optional_epoch_ms(val: Value, fn_name: &str, idx: usize) -> mlua::Result<Option<i64>> {
    let bad = |got: &str| {
        mlua::Error::RuntimeError(format!(
            "bad argument #{idx} to '{fn_name}' (integer milliseconds expected, got {got})"
        ))
    };
    match val {
        Value::Nil => Ok(None),
        Value::Integer(n) => Ok(Some(n)),
        Value::Number(f) if f.is_finite() && f.abs() < 9.0e15 => Ok(Some(f as i64)),
        other => Err(bad(other.type_name())),
    }
}

fn local_time(
    ms: Option<i64>,
    fn_name: &str,
    idx: usize,
) -> mlua::Result<chrono::DateTime<chrono::Local>> {
    use chrono::TimeZone;
    match ms {
        None => Ok(chrono::Local::now()),
        Some(ms) => chrono::Local
            .timestamp_millis_opt(ms)
            .single()
            .ok_or_else(|| {
                mlua::Error::RuntimeError(format!(
                    "bad argument #{idx} to '{fn_name}' (timestamp out of range)"
                ))
            }),
    }
}

/// Read-only clock access; `os` stays disabled. Local time zone, millisecond resolution.
fn register_time_module(lua: &Lua) -> mlua::Result<()> {
    use chrono::{Datelike, Timelike};
    use std::fmt::Write as _;

    let time = lua.create_table()?;

    time.set(
        "now",
        lua.create_function(|_, ()| Ok(chrono::Local::now().timestamp_millis()))?,
    )?;

    let started = std::time::Instant::now();
    time.set(
        "monotonic",
        lua.create_function(move |_, ()| Ok(started.elapsed().as_millis() as i64))?,
    )?;

    time.set(
        "format",
        lua.create_function(|_, (fmt, ms): (Value, Value)| {
            let fmt = expect_utf8_arg(fmt, "time.format", 1)?;
            let when = local_time(
                parse_optional_epoch_ms(ms, "time.format", 2)?,
                "time.format",
                2,
            )?;
            let mut out = String::new();
            // An invalid specifier makes the formatter fail; surface it instead of panicking.
            write!(out, "{}", when.format(&fmt)).map_err(|_| {
                mlua::Error::RuntimeError(
                    "bad argument #1 to 'time.format' (invalid format string)".to_string(),
                )
            })?;
            Ok(out)
        })?,
    )?;

    time.set(
        "date",
        lua.create_function(|lua, ms: Value| {
            let when = local_time(parse_optional_epoch_ms(ms, "time.date", 1)?, "time.date", 1)?;
            let table = lua.create_table()?;
            table.set("year", when.year())?;
            table.set("month", when.month())?;
            table.set("day", when.day())?;
            table.set("hour", when.hour())?;
            table.set("min", when.minute())?;
            table.set("sec", when.second())?;
            table.set("ms", when.timestamp_subsec_millis().min(999))?;
            table.set("wday", when.weekday().num_days_from_sunday() + 1)?;
            Ok(table)
        })?,
    )?;

    lua.globals().set("time", time)?;
    Ok(())
}

const MONITOR_REPLY_POLL_INTERVAL: Duration = Duration::from_millis(25);
const MONITOR_FULL_RETRY_INTERVAL: Duration = Duration::from_millis(5);

fn monitor_unavailable() -> mlua::Error {
    mlua::Error::RuntimeError("monitor is unavailable: the session tab is gone".to_string())
}

/// Sends `request` to the UI. Errors when the host has no receive area (CLI) or the UI is gone.
fn send_monitor_request(
    ctx: &ScriptContext,
    name: &str,
    request: MonitorRequest,
) -> mlua::Result<()> {
    ctx.check_abort().map_err(mlua::Error::external)?;
    let tx = ctx.shared.monitor.as_ref().ok_or_else(|| {
        mlua::Error::RuntimeError(format!("monitor.{name} is not supported in CLI mode"))
    })?;
    let mut request = request;
    loop {
        match tx.try_send(request) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(_)) => return Err(monitor_unavailable()),
            Err(TrySendError::Full(returned)) => {
                // The UI is behind: wait for room, but stay stoppable.
                request = returned;
                thread::sleep(MONITOR_FULL_RETRY_INTERVAL);
                ctx.check_abort().map_err(mlua::Error::external)?;
            }
        }
    }
}

/// Waits for the UI's reply, waking on cancellation or disconnect.
fn wait_monitor_reply<T>(ctx: &ScriptContext, rx: &Receiver<T>) -> mlua::Result<T> {
    loop {
        ctx.check_abort().map_err(mlua::Error::external)?;
        match rx.recv_timeout(MONITOR_REPLY_POLL_INTERVAL) {
            Ok(value) => return Ok(value),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(monitor_unavailable()),
        }
    }
}

fn register_monitor_module(lua: &Lua, ctx: &ScriptContext) -> mlua::Result<()> {
    let monitor = lua.create_table()?;

    let simple: [(&'static str, fn() -> MonitorRequest); 3] = [
        ("pause", || MonitorRequest::Pause),
        ("resume", || MonitorRequest::Resume),
        ("clear", || MonitorRequest::Clear),
    ];
    for (name, make_request) in simple {
        let call_ctx = ctx.clone();
        let f = lua
            .create_function(move |_, ()| send_monitor_request(&call_ctx, name, make_request()))?;
        monitor.set(name, f)?;
    }

    let paused_ctx = ctx.clone();
    let is_paused_fn = lua.create_function(move |_, ()| {
        let (reply, rx) = mpsc::channel();
        send_monitor_request(&paused_ctx, "is_paused", MonitorRequest::IsPaused { reply })?;
        wait_monitor_reply(&paused_ctx, &rx)
    })?;
    monitor.set("is_paused", is_paused_fn)?;

    type PathRequest = fn(PathBuf, Option<PathBuf>, Sender<Result<(), String>>) -> MonitorRequest;
    let path_requests: [(&'static str, PathRequest); 2] = [
        ("save", |rel_path, script_dir, reply| MonitorRequest::Save {
            rel_path,
            script_dir,
            reply,
        }),
        ("rotate", |rel_path, script_dir, reply| {
            MonitorRequest::Rotate {
                rel_path,
                script_dir,
                reply,
            }
        }),
    ];
    for (name, make_request) in path_requests {
        let call_ctx = ctx.clone();
        let f = lua.create_function(move |lua, val: Value| {
            let qualified = format!("monitor.{name}");
            let raw = expect_utf8_arg(val, &qualified, 1)?;
            let rel_path = validate_relative_path(&raw).map_err(|reason| {
                mlua::Error::RuntimeError(format!("bad argument #1 to '{qualified}' ({reason})"))
            })?;
            let script_dir = call_ctx
                .shared
                .path
                .parent()
                .filter(|dir| !dir.as_os_str().is_empty())
                .map(Path::to_path_buf);
            let (reply, rx) = mpsc::channel();
            send_monitor_request(&call_ctx, name, make_request(rel_path, script_dir, reply))?;
            match wait_monitor_reply(&call_ctx, &rx)? {
                Ok(()) => Ok(MultiValue::from_vec(vec![Value::Boolean(true)])),
                Err(message) => Ok(MultiValue::from_vec(vec![
                    Value::Nil,
                    Value::String(lua.create_string(&message)?),
                ])),
            }
        })?;
        monitor.set(name, f)?;
    }

    lua.globals().set("monitor", monitor)?;
    Ok(())
}

fn register_serial_module(lua: &Lua, ctx: &ScriptContext) -> mlua::Result<()> {
    let serial = lua.create_table()?;

    let send_ctx = ctx.clone();
    let send_fn = lua.create_function(move |_, val: Value| {
        let bytes = expect_bytes_arg(val, "serial.send", 1)?;
        send_raw_bytes(&send_ctx, bytes)?;
        Ok(true)
    })?;
    serial.set("send", send_fn)?;

    let write_ctx = ctx.clone();
    let write_fn = lua.create_function(move |_, val: Value| {
        let bytes = expect_bytes_arg(val, "serial.write", 1)?;
        send_raw_bytes(&write_ctx, bytes)?;
        Ok(true)
    })?;
    serial.set("write", write_fn)?;

    let send_hex_ctx = ctx.clone();
    let send_hex_fn = lua.create_function(move |_, val: Value| {
        let text = expect_utf8_arg(val, "serial.send_hex", 1)?;
        let bytes = parse_hex_bytes(&text).map_err(mlua::Error::RuntimeError)?;
        send_raw_bytes(&send_hex_ctx, bytes)?;
        Ok(true)
    })?;
    serial.set("send_hex", send_hex_fn)?;

    let read_ctx = ctx.clone();
    let read_fn = lua.create_function(move |lua, (max_val, timeout_val): (Value, Value)| {
        let max_bytes = parse_optional_positive_usize(max_val, "serial.read", 1)?;
        let timeout_ms = parse_optional_timeout_ms(timeout_val, "serial.read", 2)?;

        read_ctx.check_abort().map_err(mlua::Error::external)?;
        read_ctx.drain_pending_rx().map_err(mlua::Error::external)?;

        if let Some(bytes) = read_ctx.rx_buffer.borrow_mut().drain_available(max_bytes) {
            return Ok(MultiValue::from_vec(vec![Value::String(
                lua.create_string(&bytes)?,
            )]));
        }

        if timeout_ms > 0 {
            let deadline = deadline_after(Duration::from_millis(timeout_ms));
            while read_ctx
                .wait_for_rx_chunk(deadline)
                .map_err(mlua::Error::external)?
            {
                if let Some(bytes) = read_ctx.rx_buffer.borrow_mut().drain_available(max_bytes) {
                    return Ok(MultiValue::from_vec(vec![Value::String(
                        lua.create_string(&bytes)?,
                    )]));
                }
            }
        }

        timeout_pair(lua)
    })?;
    serial.set("read", read_fn)?;

    let wait_ctx = ctx.clone();
    let wait_text_fn =
        lua.create_function(move |lua, (pattern_val, timeout_val): (Value, Value)| {
            let pattern = expect_bytes_arg(pattern_val, "serial.wait_text", 1)?;
            if pattern.is_empty() {
                return Err(mlua::Error::RuntimeError(
                    "bad argument #1 to 'serial.wait_text' (pattern must not be empty)".to_string(),
                ));
            }
            let timeout_ms = parse_optional_timeout_ms(timeout_val, "serial.wait_text", 2)?;

            wait_ctx.check_abort().map_err(mlua::Error::external)?;
            wait_ctx.drain_pending_rx().map_err(mlua::Error::external)?;

            if let Some(matched) = wait_ctx.rx_buffer.borrow_mut().take_match(&pattern) {
                return Ok(MultiValue::from_vec(vec![Value::String(
                    lua.create_string(&matched)?,
                )]));
            }

            if timeout_ms > 0 {
                let deadline = deadline_after(Duration::from_millis(timeout_ms));
                while wait_ctx
                    .wait_for_rx_chunk(deadline)
                    .map_err(mlua::Error::external)?
                {
                    if let Some(matched) = wait_ctx.rx_buffer.borrow_mut().take_match(&pattern) {
                        return Ok(MultiValue::from_vec(vec![Value::String(
                            lua.create_string(&matched)?,
                        )]));
                    }
                }
            }

            timeout_pair(lua)
        })?;
    serial.set("wait_text", wait_text_fn)?;

    let clear_ctx = ctx.clone();
    let clear_rx_fn = lua.create_function(move |_, ()| {
        clear_ctx.check_abort().map_err(mlua::Error::external)?;
        clear_ctx
            .drain_pending_rx()
            .map_err(mlua::Error::external)?;
        clear_ctx.rx_buffer.borrow_mut().clear();
        Ok(())
    })?;
    serial.set("clear_rx", clear_rx_fn)?;

    lua.globals().set("serial", serial)?;
    Ok(())
}

fn register_task_module(lua: &Lua, ctx: &ScriptContext) -> mlua::Result<()> {
    let task = lua.create_table()?;

    let sleep_ctx = ctx.clone();
    let sleep_fn = lua.create_function(move |_, ms_val: Value| {
        let ms = parse_required_non_neg_u64(ms_val, "task.sleep", 1)?;
        sleep_ctx
            .shared
            .sleeper
            .sleep_until_or_abort(
                Duration::from_millis(ms),
                &sleep_ctx.shared.cancel_token,
                &sleep_ctx.session_service,
                sleep_ctx.shared.generation,
            )
            .map_err(mlua::Error::external)?;
        Ok(())
    })?;
    task.set("sleep", sleep_fn)?;

    let cancel_ctx = ctx.clone();
    let is_cancelled_fn = lua
        .create_function(move |_, ()| Ok(cancel_ctx.shared.cancel_token.load(Ordering::Acquire)))?;
    task.set("is_cancelled", is_cancelled_fn)?;

    lua.globals().set("task", task)?;
    Ok(())
}

fn register_checksum_module(lua: &Lua) -> mlua::Result<()> {
    use crate::checksum;

    let module = lua.create_table()?;
    type IntChecksumFn = fn(&[u8]) -> i64;
    let int_fns: [(&str, IntChecksumFn); 6] = [
        ("sum8", |d| i64::from(checksum::sum8(d))),
        ("xor8", |d| i64::from(checksum::xor8(d))),
        ("crc8", |d| i64::from(checksum::crc8(d))),
        ("crc16_modbus", |d| i64::from(checksum::crc16_modbus(d))),
        ("crc16_ccitt", |d| i64::from(checksum::crc16_ccitt(d))),
        ("crc32", |d| i64::from(checksum::crc32(d))),
    ];
    for (name, compute) in int_fns {
        let qualified = format!("checksum.{name}");
        let f = lua.create_function(move |_, val: Value| {
            let bytes = expect_bytes_arg(val, &qualified, 1)?;
            Ok(compute(&bytes))
        })?;
        module.set(name, f)?;
    }

    let append_fn = lua.create_function(|lua, (data, algo): (Value, Value)| {
        let bytes = expect_bytes_arg(data, "checksum.append", 1)?;
        let name = expect_utf8_arg(algo, "checksum.append", 2)?;
        let algo = checksum::Checksum::from_name(&name).ok_or_else(|| {
            mlua::Error::RuntimeError(format!(
                "bad argument #2 to 'checksum.append' (unknown algorithm '{name}')"
            ))
        })?;
        lua.create_string(algo.append_to(bytes))
    })?;
    module.set("append", append_fn)?;

    lua.globals().set("checksum", module)?;
    Ok(())
}

fn register_log_module(lua: &Lua, ctx: &ScriptContext) -> mlua::Result<()> {
    let log = lua.create_table()?;

    for (name, level) in [
        ("info", ScriptLogLevel::Info),
        ("warn", ScriptLogLevel::Warn),
        ("error", ScriptLogLevel::Error),
    ] {
        let log_ctx = ctx.clone();
        let log_fn = lua.create_function(move |_, args: MultiValue| {
            let msg = format_lua_args(args)?;
            log_ctx.append_log(level, msg);
            Ok(())
        })?;
        log.set(name, log_fn)?;
    }

    let print_ctx = ctx.clone();
    let print_fn = lua.create_function(move |_, args: MultiValue| {
        let msg = format_lua_args(args)?;
        print_ctx.append_log(ScriptLogLevel::Info, msg);
        Ok(())
    })?;

    let globals = lua.globals();
    globals.set("log", log)?;
    globals.set("print", print_fn)?;
    Ok(())
}

fn send_raw_bytes(ctx: &ScriptContext, bytes: Vec<u8>) -> mlua::Result<()> {
    ctx.check_abort().map_err(mlua::Error::external)?;
    let res = ctx.session_service.send_tx_blocking(
        ctx.shared.generation,
        bytes,
        Some(Arc::clone(&ctx.shared.cancel_token)),
    );
    match res {
        Ok(()) => Ok(()),
        Err(SessionTxError::Cancelled) => Err(mlua::Error::external(ScriptAbort::Cancelled)),
        Err(SessionTxError::Disconnected | SessionTxError::StaleGeneration { .. }) => {
            Err(mlua::Error::external(ScriptAbort::Disconnected))
        }
        Err(err @ (SessionTxError::QueueFull | SessionTxError::WriteFailed(_))) => {
            Err(mlua::Error::RuntimeError(err.to_string()))
        }
    }
}

fn timeout_pair(lua: &Lua) -> mlua::Result<MultiValue> {
    Ok(MultiValue::from_vec(vec![
        Value::Nil,
        Value::String(lua.create_string("timeout")?),
    ]))
}

fn expect_bytes_arg(val: Value, fn_name: &str, idx: usize) -> mlua::Result<Vec<u8>> {
    match val {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        other => Err(mlua::Error::RuntimeError(format!(
            "bad argument #{idx} to '{fn_name}' (string expected, got {})",
            other.type_name()
        ))),
    }
}

fn expect_utf8_arg(val: Value, fn_name: &str, idx: usize) -> mlua::Result<String> {
    match val {
        Value::String(s) => s.to_str().map(|borrowed| borrowed.to_owned()).map_err(|_| {
            mlua::Error::RuntimeError(format!(
                "bad argument #{idx} to '{fn_name}' (valid UTF-8 string expected)"
            ))
        }),
        other => Err(mlua::Error::RuntimeError(format!(
            "bad argument #{idx} to '{fn_name}' (string expected, got {})",
            other.type_name()
        ))),
    }
}

fn parse_optional_positive_usize(
    val: Value,
    fn_name: &str,
    idx: usize,
) -> mlua::Result<Option<usize>> {
    match val {
        Value::Nil => Ok(None),
        Value::Integer(n) if n > 0 => usize::try_from(n).map(Some).map_err(|_| {
            mlua::Error::RuntimeError(format!(
                "bad argument #{idx} to '{fn_name}' (integer out of range)"
            ))
        }),
        Value::Number(f) if f.is_finite() && f >= 1.0 && f.fract() == 0.0 => {
            if f > usize::MAX as f64 {
                return Err(mlua::Error::RuntimeError(format!(
                    "bad argument #{idx} to '{fn_name}' (number out of range)"
                )));
            }
            Ok(Some(f as usize))
        }
        other => Err(mlua::Error::RuntimeError(format!(
            "bad argument #{idx} to '{fn_name}' (positive integer expected, got {})",
            other.type_name()
        ))),
    }
}

fn parse_optional_timeout_ms(val: Value, fn_name: &str, idx: usize) -> mlua::Result<u64> {
    match val {
        Value::Nil => Ok(DEFAULT_RX_TIMEOUT_MS),
        other => parse_required_non_neg_u64(other, fn_name, idx),
    }
}

fn parse_required_non_neg_u64(val: Value, fn_name: &str, idx: usize) -> mlua::Result<u64> {
    match val {
        Value::Integer(n) if n >= 0 => Ok(n as u64),
        Value::Number(f) if f.is_finite() && f >= 0.0 && f <= u64::MAX as f64 => Ok(f as u64),
        other => Err(mlua::Error::RuntimeError(format!(
            "bad argument #{idx} to '{fn_name}' (non-negative number expected, got {})",
            other.type_name()
        ))),
    }
}

fn parse_hex_bytes(input: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut saw_token = false;

    for token in input.split_whitespace() {
        saw_token = true;
        if !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("invalid hex character in '{token}'"));
        }
        if token.len() % 2 != 0 {
            return Err(format!(
                "hex token '{token}' must have an even number of hex digits"
            ));
        }
        for pair in token.as_bytes().as_chunks::<2>().0 {
            let pair_str = std::str::from_utf8(pair).expect("ASCII hex digits are valid UTF-8");
            let byte = u8::from_str_radix(pair_str, 16)
                .map_err(|_| format!("invalid hex byte '{pair_str}'"))?;
            bytes.push(byte);
        }
    }

    if !saw_token {
        return Err("hex string must not be empty".to_string());
    }
    Ok(bytes)
}

fn format_lua_args(args: MultiValue) -> mlua::Result<String> {
    let mut parts = Vec::with_capacity(args.len());
    for val in args {
        parts.push(val.to_string()?);
    }
    Ok(parts.join("\t"))
}
