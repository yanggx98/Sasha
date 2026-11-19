use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Value, VmState};

use crate::session_service::{RxRecvError, RxSubscriber, SessionService};

use super::api::register_api;
use super::{
    LUA_MEMORY_LIMIT_BYTES, SCRIPT_EVENT_QUEUE_MAX, SCRIPT_LOG_MAX_LINES,
    SCRIPT_RX_BUFFER_MAX_BYTES, ScriptEvent, ScriptLogEntry, ScriptLogLevel, ScriptStatus,
    StopReason,
};

const INSTRUCTION_HOOK_INTERVAL: u32 = 1_000;
const RX_CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(25);
const SLEEP_DISCONNECT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// `Instant::now() + duration` that saturates instead of panicking on overflow
/// (e.g. a script passing `1e18` milliseconds).
pub(crate) fn deadline_after(duration: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(duration)
        .or_else(|| now.checked_add(Duration::from_secs(60 * 60 * 24 * 365)))
        .unwrap_or(now)
}

/// Lua prelude: re-raises a pending abort after `pcall`/`xpcall`/`coroutine.resume`
/// so scripts cannot swallow cancellation or disconnect.
const ABORT_GUARD_LUA: &str = r#"
local raw_pcall, raw_xpcall, raw_resume, check = pcall, xpcall, coroutine.resume, ...
local function guard(...) check() return ... end
pcall = function(f, ...) return guard(raw_pcall(f, ...)) end
xpcall = function(f, h, ...) return guard(raw_xpcall(f, h, ...)) end
coroutine.resume = function(co, ...) return guard(raw_resume(co, ...)) end
"#;

/// Internal abort sentinel used to unwind Lua execution cleanly on cancellation,
/// session disconnect, or RX buffer overflow.
#[derive(Debug, Clone)]
pub(crate) enum ScriptAbort {
    Cancelled,
    Disconnected,
    BufferOverflow(String),
}

impl fmt::Display for ScriptAbort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(f, "script cancelled"),
            Self::Disconnected => write!(f, "session disconnected"),
            Self::BufferOverflow(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ScriptAbort {}

/// Condvar-backed sleeper that wakes immediately when `wake()` is called.
pub(crate) struct CancellableSleeper {
    woken: Mutex<bool>,
    condvar: Condvar,
}

impl CancellableSleeper {
    pub(crate) fn new() -> Self {
        Self {
            woken: Mutex::new(false),
            condvar: Condvar::new(),
        }
    }

    pub(crate) fn wake(&self) {
        let mut guard = self
            .woken
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = true;
        self.condvar.notify_all();
    }

    /// Sleeps up to `duration`, returning `Err(ScriptAbort)` if cancelled or disconnected.
    pub(crate) fn sleep_until_or_abort(
        &self,
        duration: Duration,
        cancel_token: &AtomicBool,
        session_service: &SessionService,
        generation: u64,
    ) -> Result<(), ScriptAbort> {
        let deadline = deadline_after(duration);
        let mut guard = self
            .woken
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if cancel_token.load(Ordering::Acquire) {
                return Err(ScriptAbort::Cancelled);
            }
            if !session_service.is_connected() || session_service.generation() != generation {
                return Err(ScriptAbort::Disconnected);
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(());
            }
            *guard = false;
            let wait = (deadline - now).min(SLEEP_DISCONNECT_POLL_INTERVAL);
            let (next_guard, _) = self
                .condvar
                .wait_timeout(guard, wait)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard = next_guard;
        }
    }
}

/// Per-instance byte accumulation buffer supporting cross-chunk pattern matching
/// and binary-safe reads with an explicit upper bound.
pub(crate) struct RxBuffer {
    buf: Vec<u8>,
    max_bytes: usize,
}

impl RxBuffer {
    pub(crate) fn new(max_bytes: usize) -> Self {
        Self {
            buf: Vec::new(),
            max_bytes,
        }
    }

    pub(crate) fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), String> {
        if self.buf.len().saturating_add(chunk.len()) > self.max_bytes {
            return Err(format!(
                "script RX buffer overflow (limit: {} bytes)",
                self.max_bytes
            ));
        }
        self.buf.extend_from_slice(chunk);
        Ok(())
    }

    pub(crate) fn drain_available(&mut self, max_bytes: Option<usize>) -> Option<Vec<u8>> {
        if self.buf.is_empty() {
            return None;
        }
        let count = max_bytes.map_or(self.buf.len(), |n| n.min(self.buf.len()));
        Some(self.buf.drain(..count).collect())
    }

    pub(crate) fn take_match(&mut self, pattern: &[u8]) -> Option<Vec<u8>> {
        if pattern.is_empty() || self.buf.len() < pattern.len() {
            return None;
        }
        let pos = self.buf.windows(pattern.len()).position(|w| w == pattern)?;
        let end = pos + pattern.len();
        Some(self.buf.drain(..end).collect())
    }

    pub(crate) fn clear(&mut self) {
        self.buf.clear();
    }
}

/// Shared control and observation state between `ScriptManager` and a worker thread.
pub(crate) struct InstanceShared {
    pub(crate) run_id: u64,
    pub(crate) path: PathBuf,
    pub(crate) generation: u64,
    pub(crate) cancel_token: Arc<AtomicBool>,
    pub(crate) stop_reason: Mutex<Option<StopReason>>,
    pub(crate) sleeper: Arc<CancellableSleeper>,
    pub(crate) rx_subscriber: Arc<RxSubscriber>,
    pub(crate) status: Mutex<ScriptStatus>,
    pub(crate) logs: Mutex<VecDeque<ScriptLogEntry>>,
}

/// Worker-thread-local context captured by Lua host API closures.
#[derive(Clone)]
pub(crate) struct ScriptContext {
    pub(crate) shared: Arc<InstanceShared>,
    pub(crate) session_service: SessionService,
    pub(crate) rx_buffer: Rc<RefCell<RxBuffer>>,
    pub(crate) events: Arc<Mutex<VecDeque<ScriptEvent>>>,
}

impl ScriptContext {
    pub(crate) fn check_abort(&self) -> Result<(), ScriptAbort> {
        if self.shared.cancel_token.load(Ordering::Acquire) {
            return Err(ScriptAbort::Cancelled);
        }
        if !self.session_service.is_connected()
            || self.session_service.generation() != self.shared.generation
        {
            return Err(ScriptAbort::Disconnected);
        }
        Ok(())
    }

    pub(crate) fn append_log(&self, level: ScriptLogLevel, message: String) {
        let entry = ScriptLogEntry {
            run_id: self.shared.run_id,
            level,
            message,
        };
        {
            let mut logs = self
                .shared
                .logs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if logs.len() >= SCRIPT_LOG_MAX_LINES {
                logs.pop_front();
            }
            logs.push_back(entry.clone());
        }
        let mut events = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if events.len() >= SCRIPT_EVENT_QUEUE_MAX {
            // Shed the oldest log line; lifecycle events are never dropped.
            if let Some(pos) = events
                .iter()
                .position(|ev| matches!(ev, ScriptEvent::Log(_)))
            {
                events.remove(pos);
            }
        }
        events.push_back(ScriptEvent::Log(entry));
    }

    /// Drains all currently queued chunks from `RxSubscriber` into `RxBuffer`.
    pub(crate) fn drain_pending_rx(&self) -> Result<(), ScriptAbort> {
        loop {
            match self.shared.rx_subscriber.try_recv() {
                Ok(chunk) => {
                    self.rx_buffer
                        .borrow_mut()
                        .push_chunk(&chunk)
                        .map_err(ScriptAbort::BufferOverflow)?;
                }
                Err(RxRecvError::Empty) => return Ok(()),
                Err(RxRecvError::Disconnected(_)) => return Err(ScriptAbort::Disconnected),
                Err(RxRecvError::Overflow {
                    max_chunks,
                    max_bytes,
                }) => {
                    return Err(ScriptAbort::BufferOverflow(format!(
                        "RX subscriber buffer overflow (limit: {max_chunks} chunks / {max_bytes} bytes)"
                    )));
                }
                Err(RxRecvError::Timeout) => return Ok(()),
            }
        }
    }

    /// Waits for at least one new RX chunk up to `deadline`, pushing it into `RxBuffer`.
    /// Returns `Ok(true)` if new data arrived, or `Ok(false)` on timeout.
    pub(crate) fn wait_for_rx_chunk(&self, deadline: Instant) -> Result<bool, ScriptAbort> {
        loop {
            self.check_abort()?;
            let now = Instant::now();
            if now >= deadline {
                return Ok(false);
            }
            let slice = (deadline - now).min(RX_CANCEL_POLL_INTERVAL);
            match self.shared.rx_subscriber.recv_timeout(slice) {
                Ok(chunk) => {
                    self.rx_buffer
                        .borrow_mut()
                        .push_chunk(&chunk)
                        .map_err(ScriptAbort::BufferOverflow)?;
                    self.drain_pending_rx()?;
                    return Ok(true);
                }
                Err(RxRecvError::Timeout) => continue,
                Err(RxRecvError::Disconnected(_)) => return Err(ScriptAbort::Disconnected),
                Err(RxRecvError::Overflow {
                    max_chunks,
                    max_bytes,
                }) => {
                    return Err(ScriptAbort::BufferOverflow(format!(
                        "RX subscriber buffer overflow (limit: {max_chunks} chunks / {max_bytes} bytes)"
                    )));
                }
                Err(RxRecvError::Empty) => continue,
            }
        }
    }
}

pub(crate) fn run_script_instance(
    shared: Arc<InstanceShared>,
    source: String,
    session_service: SessionService,
    events: Arc<Mutex<VecDeque<ScriptEvent>>>,
) {
    let final_status = match execute_lua(&shared, &source, &session_service, &events) {
        Ok(()) => {
            if shared.cancel_token.load(Ordering::Acquire) {
                let reason = resolve_stop_reason(&shared, &session_service);
                ScriptStatus::Stopped { reason }
            } else {
                ScriptStatus::Completed
            }
        }
        Err(err) => classify_lua_error(&err, &shared, &session_service),
    };

    {
        let mut status = shared
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *status = final_status.clone();
    }
    let mut queue = events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    queue.push_back(ScriptEvent::StatusChanged {
        run_id: shared.run_id,
        status: final_status,
    });
}

fn execute_lua(
    shared: &Arc<InstanceShared>,
    source: &str,
    session_service: &SessionService,
    events: &Arc<Mutex<VecDeque<ScriptEvent>>>,
) -> mlua::Result<()> {
    let stdlib = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8;
    let lua = Lua::new_with(stdlib, LuaOptions::default())?;
    lua.set_memory_limit(LUA_MEMORY_LIMIT_BYTES)?;

    let hook_cancel = Arc::clone(&shared.cancel_token);
    let hook_service = session_service.clone();
    let hook_gen = shared.generation;
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(INSTRUCTION_HOOK_INTERVAL),
        move |_lua, _debug| {
            if hook_cancel.load(Ordering::Acquire) {
                return Err(mlua::Error::external(ScriptAbort::Cancelled));
            }
            if !hook_service.is_connected() || hook_service.generation() != hook_gen {
                return Err(mlua::Error::external(ScriptAbort::Disconnected));
            }
            Ok(VmState::Continue)
        },
    );

    let ctx = ScriptContext {
        shared: Arc::clone(shared),
        session_service: session_service.clone(),
        rx_buffer: Rc::new(RefCell::new(RxBuffer::new(SCRIPT_RX_BUFFER_MAX_BYTES))),
        events: Arc::clone(events),
    };
    let abort_ctx = ctx.clone();
    register_api(&lua, ctx)?;

    // Disable dynamic code loading from globals.
    let globals = lua.globals();
    globals.set("dofile", Value::Nil)?;
    globals.set("loadfile", Value::Nil)?;
    globals.set("load", Value::Nil)?;

    let check_abort =
        lua.create_function(move |_, ()| abort_ctx.check_abort().map_err(mlua::Error::external))?;
    lua.load(ABORT_GUARD_LUA)
        .set_name("=abort_guard")
        .call::<()>(check_abort)?;

    let chunk_name = shared.path.to_string_lossy().into_owned();
    lua.load(source).set_name(&chunk_name).exec()
}

fn resolve_stop_reason(shared: &InstanceShared, session_service: &SessionService) -> StopReason {
    if let Some(reason) = *shared
        .stop_reason
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        return reason;
    }
    if !session_service.is_connected() || session_service.generation() != shared.generation {
        StopReason::Disconnected
    } else {
        StopReason::User
    }
}

fn extract_script_abort(err: &mlua::Error) -> Option<&ScriptAbort> {
    let mut current = err;
    loop {
        match current {
            mlua::Error::ExternalError(ext) => {
                return ext.downcast_ref::<ScriptAbort>();
            }
            mlua::Error::CallbackError { cause, .. } => {
                current = cause.as_ref();
            }
            _ => return None,
        }
    }
}

fn classify_lua_error(
    err: &mlua::Error,
    shared: &InstanceShared,
    session_service: &SessionService,
) -> ScriptStatus {
    if let Some(abort) = extract_script_abort(err) {
        return match abort {
            ScriptAbort::Cancelled => ScriptStatus::Stopped {
                reason: resolve_stop_reason(shared, session_service),
            },
            ScriptAbort::Disconnected => ScriptStatus::Stopped {
                reason: StopReason::Disconnected,
            },
            ScriptAbort::BufferOverflow(msg) => ScriptStatus::Failed { error: msg.clone() },
        };
    }
    if shared.cancel_token.load(Ordering::Acquire) {
        return ScriptStatus::Stopped {
            reason: resolve_stop_reason(shared, session_service),
        };
    }
    ScriptStatus::Failed {
        error: err.to_string(),
    }
}
