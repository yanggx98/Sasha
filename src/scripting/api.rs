use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use mlua::{Lua, MultiValue, Value};

use crate::session_service::SessionTxError;

use super::runtime::{ScriptAbort, ScriptContext};
use super::{DEFAULT_RX_TIMEOUT_MS, ScriptLogLevel};

/// Registers `serial`, `task`, `log`, and global `print` into the Lua VM.
pub(crate) fn register_api(lua: &Lua, ctx: ScriptContext) -> mlua::Result<()> {
    register_serial_module(lua, &ctx)?;
    register_task_module(lua, &ctx)?;
    register_log_module(lua, &ctx)?;
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
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
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
                let deadline = Instant::now() + Duration::from_millis(timeout_ms);
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
