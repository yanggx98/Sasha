#![allow(dead_code)]

mod api;
mod monitor;
mod runtime;

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};

use crate::session_service::SessionService;

pub(crate) use monitor::save_text;
use runtime::{CancellableSleeper, InstanceShared, run_script_instance};

/// Per-instance Lua VM memory limit (16 MiB).
pub const LUA_MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// Per-instance accumulated RX buffer cap (1 MiB).
pub const SCRIPT_RX_BUFFER_MAX_BYTES: usize = 1024 * 1024;

/// Maximum number of log lines retained per script instance.
pub const SCRIPT_LOG_MAX_LINES: usize = 500;

/// Maximum buffered events per manager before the oldest log events are shed.
pub const SCRIPT_EVENT_QUEUE_MAX: usize = 10_000;

/// Default timeout in milliseconds when `timeout_ms` is omitted in `serial.read` / `serial.wait_text`.
pub const DEFAULT_RX_TIMEOUT_MS: u64 = 5_000;

/// Pending `monitor.*` requests per manager. When the UI falls behind, the script blocks
/// (abort-aware) instead of queueing without limit.
pub const MONITOR_QUEUE_DEPTH: usize = 256;

/// An action a script asks the UI to perform on the receive area (see the Lua `monitor` module).
#[derive(Debug)]
pub enum MonitorRequest {
    Pause,
    Resume,
    Clear,
    IsPaused {
        reply: Sender<bool>,
    },
    /// `script_dir` is the directory of the script file, the default output directory.
    Save {
        rel_path: PathBuf,
        script_dir: Option<PathBuf>,
        reply: Sender<Result<(), String>>,
    },
    /// Like `Save`, but writes every row (ignoring the filter) and clears the log in the same
    /// step, only if the write succeeded.
    Rotate {
        rel_path: PathBuf,
        script_dir: Option<PathBuf>,
        reply: Sender<Result<(), String>>,
    },
}

/// Reason a script instance stopped before natural completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    User,
    Disconnected,
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "stopped by user"),
            Self::Disconnected => write!(f, "session disconnected"),
        }
    }
}

/// Lifecycle state of a single script execution instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptStatus {
    Running,
    Stopping,
    Completed,
    Stopped { reason: StopReason },
    Failed { error: String },
}

impl ScriptStatus {
    /// Returns `true` while the script thread is still active (`Running` or `Stopping`).
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::Stopping)
    }
}

/// Severity level for script output entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptLogLevel {
    Info,
    Warn,
    Error,
}

/// Single log line emitted by a script instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptLogEntry {
    pub run_id: u64,
    pub level: ScriptLogLevel,
    pub message: String,
}

/// Asynchronous lifecycle and output events produced by script instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptEvent {
    Started {
        run_id: u64,
        path: PathBuf,
        generation: u64,
    },
    StatusChanged {
        run_id: u64,
        status: ScriptStatus,
    },
    Log(ScriptLogEntry),
}

/// Point-in-time snapshot of a script instance for UI or test inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptInstanceSnapshot {
    pub run_id: u64,
    pub path: PathBuf,
    pub generation: u64,
    pub status: ScriptStatus,
    pub logs: Vec<ScriptLogEntry>,
}

struct ManagedInstance {
    shared: Arc<InstanceShared>,
    thread: Option<JoinHandle<()>>,
}

type InstanceMap = BTreeMap<u64, ManagedInstance>;
type EventQueue = VecDeque<ScriptEvent>;

/// Per-session manager for Lua script instances, decoupled from UI state.
pub struct ScriptManager {
    next_run_id: u64,
    instances: Arc<Mutex<InstanceMap>>,
    events: Arc<Mutex<EventQueue>>,
    disconnect_watcher_attached: bool,
    monitor_tx: Option<SyncSender<MonitorRequest>>,
}

impl Default for ScriptManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptManager {
    pub fn new() -> Self {
        Self {
            next_run_id: 1,
            instances: Arc::new(Mutex::new(BTreeMap::new())),
            events: Arc::new(Mutex::new(VecDeque::new())),
            disconnect_watcher_attached: false,
            monitor_tx: None,
        }
    }

    /// Lets scripts started from now on use the Lua `monitor` module; the caller (the GUI)
    /// receives their requests on the returned channel. Without this call (the CLI) every
    /// `monitor.*` function raises "not supported in CLI mode".
    pub fn enable_monitor(&mut self) -> Receiver<MonitorRequest> {
        let (tx, rx) = mpsc::sync_channel(MONITOR_QUEUE_DEPTH);
        self.monitor_tx = Some(tx);
        rx
    }

    /// Starts a new Lua script instance using the provided `source` snapshot.
    ///
    /// Registers an independent [`RxSubscriber`](crate::session_service::RxSubscriber)
    /// before executing the first Lua statement so RX bytes arriving right after start
    /// are never missed.
    pub fn start_script(
        &mut self,
        path: PathBuf,
        source: String,
        session_service: &SessionService,
        generation: u64,
    ) -> Result<u64, String> {
        let rx_subscriber = session_service
            .subscribe_rx(generation)
            .map_err(|err| err.to_string())?;
        let rx_subscriber = Arc::new(rx_subscriber);

        self.ensure_disconnect_watcher(session_service);

        let run_id = self.next_run_id;
        self.next_run_id += 1;

        let shared = Arc::new(InstanceShared {
            run_id,
            path: path.clone(),
            generation,
            cancel_token: Arc::new(AtomicBool::new(false)),
            stop_reason: Mutex::new(None),
            sleeper: Arc::new(CancellableSleeper::new()),
            rx_subscriber: Arc::clone(&rx_subscriber),
            status: Mutex::new(ScriptStatus::Running),
            logs: Mutex::new(VecDeque::new()),
            monitor: self.monitor_tx.clone(),
        });

        {
            let mut events = self
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            events.push_back(ScriptEvent::Started {
                run_id,
                path,
                generation,
            });
        }

        let shared_for_thread = Arc::clone(&shared);
        let events_for_thread = Arc::clone(&self.events);
        let service_for_thread = session_service.clone();

        let handle = thread::Builder::new()
            .name(format!("sasha-lua-run-{run_id}"))
            .spawn(move || {
                run_script_instance(
                    shared_for_thread,
                    source,
                    service_for_thread,
                    events_for_thread,
                );
            })
            .map_err(|err| format!("failed to spawn script thread: {err}"))?;

        let mut instances = self
            .instances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        instances.insert(
            run_id,
            ManagedInstance {
                shared,
                thread: Some(handle),
            },
        );

        Ok(run_id)
    }

    /// Requests cancellation of a single active script instance.
    pub fn stop_instance(&mut self, run_id: u64) -> bool {
        let instances = self
            .instances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(instance) = instances.get(&run_id) else {
            return false;
        };
        request_stop(&instance.shared, &self.events, StopReason::User)
    }

    /// Requests cancellation of all active script instances in this manager.
    pub fn stop_all(&mut self, reason: StopReason) {
        stop_all_instances(&self.instances, &self.events, reason);
    }

    /// Handles a session disconnect by stopping all active script instances.
    pub fn handle_disconnect(&mut self, _reason: Option<String>) {
        self.stop_all(StopReason::Disconnected);
    }

    /// Returns the number of currently active (`Running` or `Stopping`) script instances.
    pub fn running_count(&self) -> usize {
        let instances = self
            .instances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        instances
            .values()
            .filter(|inst| {
                inst.shared
                    .status
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_active()
            })
            .count()
    }

    /// Returns snapshots of all known script instances ordered by `run_id`.
    pub fn snapshots(&self) -> Vec<ScriptInstanceSnapshot> {
        let instances = self
            .instances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        instances
            .values()
            .map(|inst| {
                let status = inst
                    .shared
                    .status
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                let logs = inst
                    .shared
                    .logs
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .iter()
                    .cloned()
                    .collect();
                ScriptInstanceSnapshot {
                    run_id: inst.shared.run_id,
                    path: inst.shared.path.clone(),
                    generation: inst.shared.generation,
                    status,
                    logs,
                }
            })
            .collect()
    }

    /// Drains all buffered [`ScriptEvent`]s since the last call.
    pub fn drain_events(&mut self) -> Vec<ScriptEvent> {
        let mut events = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        events.drain(..).collect()
    }

    /// Joins all worker threads that have been spawned by this manager.
    pub fn join_all(&mut self) {
        let handles: Vec<JoinHandle<()>> = {
            let mut instances = self
                .instances
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            instances
                .values_mut()
                .filter_map(|inst| inst.thread.take())
                .collect()
        };
        let current_id = thread::current().id();
        for handle in handles {
            if handle.thread().id() != current_id {
                let _ = handle.join();
            }
        }
    }

    fn ensure_disconnect_watcher(&mut self, session_service: &SessionService) {
        if self.disconnect_watcher_attached {
            return;
        }
        self.disconnect_watcher_attached = true;
        let disconnect_rx = session_service.subscribe_disconnect();
        let instances_weak: Weak<Mutex<InstanceMap>> = Arc::downgrade(&self.instances);
        let events_weak: Weak<Mutex<EventQueue>> = Arc::downgrade(&self.events);

        let _ = thread::Builder::new()
            .name("sasha-lua-disconnect-watcher".to_string())
            .spawn(move || {
                while disconnect_rx.recv().is_ok() {
                    let (Some(instances), Some(events)) =
                        (instances_weak.upgrade(), events_weak.upgrade())
                    else {
                        break;
                    };
                    stop_all_instances(&instances, &events, StopReason::Disconnected);
                }
            });
    }
}

impl Drop for ScriptManager {
    fn drop(&mut self) {
        self.stop_all(StopReason::User);
        self.join_all();
    }
}

fn request_stop(
    shared: &Arc<InstanceShared>,
    events: &Arc<Mutex<EventQueue>>,
    reason: StopReason,
) -> bool {
    let transition = {
        let mut status = shared
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !status.is_active() {
            return false;
        }
        let mut stop_reason = shared
            .stop_reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if stop_reason.is_none() || reason == StopReason::Disconnected {
            *stop_reason = Some(reason);
        }
        if *status == ScriptStatus::Running {
            *status = ScriptStatus::Stopping;
            true
        } else {
            false
        }
    };

    shared.cancel_token.store(true, Ordering::Release);
    shared.sleeper.wake();
    shared.rx_subscriber.wake();

    if transition {
        let mut queue = events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.push_back(ScriptEvent::StatusChanged {
            run_id: shared.run_id,
            status: ScriptStatus::Stopping,
        });
    }
    true
}

fn stop_all_instances(
    instances: &Arc<Mutex<InstanceMap>>,
    events: &Arc<Mutex<EventQueue>>,
    reason: StopReason,
) {
    let shared_list: Vec<Arc<InstanceShared>> = {
        let guard = instances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .values()
            .map(|inst| Arc::clone(&inst.shared))
            .collect()
    };
    for shared in shared_list {
        request_stop(&shared, events, reason);
    }
}

#[cfg(test)]
mod tests;
