#![allow(dead_code)]

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Maximum number of pending TX requests in the unified per-session queue.
pub const MAX_TX_QUEUE_DEPTH: usize = 256;

/// Default maximum number of raw RX chunks buffered per subscriber before overflow.
pub const RX_SUBSCRIBER_MAX_CHUNKS: usize = 256;

/// Default maximum number of raw RX bytes buffered per subscriber before overflow (1 MiB).
pub const RX_SUBSCRIBER_MAX_BYTES: usize = 1024 * 1024;

/// Abstraction over a serial port write handle so `SessionService` can be tested
/// without hardware and decoupled from `serialport::SerialPort`.
pub trait PortWriter: Send + 'static {
    /// Writes all `data` bytes to the port and flushes the transmit buffer.
    fn write_all_and_flush(&mut self, data: &[u8]) -> Result<(), String>;

    /// Sets the DTR output level. Writers without modem control accept and ignore it.
    fn set_dtr(&mut self, _level: bool) -> Result<(), String> {
        Ok(())
    }

    /// Sets the RTS output level. Writers without modem control accept and ignore it.
    fn set_rts(&mut self, _level: bool) -> Result<(), String> {
        Ok(())
    }
}

/// Modem output line addressable through the writer thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlLine {
    Dtr,
    Rts,
}

/// Errors returned when enqueuing or executing a TX request through [`SessionService`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTxError {
    Disconnected,
    StaleGeneration { expected: u64, actual: u64 },
    Cancelled,
    QueueFull,
    WriteFailed(String),
}

impl fmt::Display for SessionTxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => write!(f, "session is disconnected"),
            Self::StaleGeneration { expected, actual } => {
                write!(
                    f,
                    "stale connection generation (expected {expected}, got {actual})"
                )
            }
            Self::Cancelled => write!(f, "transmit request was cancelled"),
            Self::QueueFull => write!(f, "transmit queue is full (capacity {MAX_TX_QUEUE_DEPTH})"),
            Self::WriteFailed(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SessionTxError {}

/// Errors returned when registering a raw RX subscriber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RxSubscribeError {
    Disconnected,
    StaleGeneration { expected: u64, actual: u64 },
}

impl fmt::Display for RxSubscribeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => write!(f, "session is disconnected"),
            Self::StaleGeneration { expected, actual } => {
                write!(
                    f,
                    "stale connection generation (expected {expected}, got {actual})"
                )
            }
        }
    }
}

impl std::error::Error for RxSubscribeError {}

/// Errors returned when reading from an [`RxSubscriber`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RxRecvError {
    Empty,
    Timeout,
    Disconnected(Option<String>),
    Overflow { max_chunks: usize, max_bytes: usize },
}

impl fmt::Display for RxRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "no raw RX data available"),
            Self::Timeout => write!(f, "timed out waiting for raw RX data"),
            Self::Disconnected(Some(reason)) => write!(f, "session disconnected: {reason}"),
            Self::Disconnected(None) => write!(f, "session disconnected"),
            Self::Overflow {
                max_chunks,
                max_bytes,
            } => write!(
                f,
                "RX subscriber buffer overflow (limit: {max_chunks} chunks / {max_bytes} bytes)"
            ),
        }
    }
}

impl std::error::Error for RxRecvError {}

/// Emitted after a queued TX item has been written and flushed successfully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxCompletedEvent {
    pub generation: u64,
    pub bytes: Vec<u8>,
}

/// Emitted when an active connection transitions to disconnected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisconnectEvent {
    pub generation: u64,
    pub reason: Option<String>,
}

struct TxCommand {
    generation: u64,
    bytes: Vec<u8>,
    cancel_token: Option<Arc<AtomicBool>>,
    reply_tx: Sender<Result<(), SessionTxError>>,
}

/// Items processed serially by the writer thread, preserving queue order.
enum WriterCommand {
    Tx(TxCommand),
    Control {
        generation: u64,
        line: ControlLine,
        level: bool,
        reply_tx: Sender<Result<(), SessionTxError>>,
    },
}

struct RxQueueInner {
    chunks: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    max_chunks: usize,
    max_bytes: usize,
    overflowed: bool,
    disconnected: Option<Option<String>>,
    closed: bool,
}

struct RxSubscriberQueue {
    inner: Mutex<RxQueueInner>,
    condvar: Condvar,
}

impl RxSubscriberQueue {
    fn new(max_chunks: usize, max_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(RxQueueInner {
                chunks: VecDeque::new(),
                queued_bytes: 0,
                max_chunks,
                max_bytes,
                overflowed: false,
                disconnected: None,
                closed: false,
            }),
            condvar: Condvar::new(),
        }
    }

    fn is_closed(&self) -> bool {
        self.inner.lock().unwrap().closed
    }

    fn push_bytes(&self, bytes: &[u8]) {
        let mut guard = self.inner.lock().unwrap();
        if guard.closed || guard.overflowed || guard.disconnected.is_some() {
            return;
        }
        if guard.chunks.len() >= guard.max_chunks
            || guard.queued_bytes.saturating_add(bytes.len()) > guard.max_bytes
        {
            guard.overflowed = true;
            guard.chunks.clear();
            guard.queued_bytes = 0;
            self.condvar.notify_all();
            return;
        }
        guard.queued_bytes += bytes.len();
        guard.chunks.push_back(bytes.to_vec());
        self.condvar.notify_all();
    }

    fn mark_disconnected(&self, reason: Option<String>) {
        let mut guard = self.inner.lock().unwrap();
        if guard.disconnected.is_none() {
            guard.disconnected = Some(reason);
            self.condvar.notify_all();
        }
    }
}

/// Independent raw RX stream handle for a single subscriber (e.g. a Lua script instance).
pub struct RxSubscriber {
    queue: Arc<RxSubscriberQueue>,
    generation: u64,
}

impl RxSubscriber {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Wakes any thread blocked in `recv` or `recv_timeout`.
    pub fn wake(&self) {
        self.queue.condvar.notify_all();
    }

    /// Attempts to pop one raw RX chunk without blocking.
    pub fn try_recv(&self) -> Result<Vec<u8>, RxRecvError> {
        let mut guard = self.queue.inner.lock().unwrap();
        Self::pop_locked(&mut guard).ok_or(RxRecvError::Empty)?
    }

    /// Blocks until a raw RX chunk arrives, the session disconnects, or the buffer overflows.
    pub fn recv(&self) -> Result<Vec<u8>, RxRecvError> {
        let mut guard = self.queue.inner.lock().unwrap();
        loop {
            if let Some(res) = Self::pop_locked(&mut guard) {
                return res;
            }
            guard = self.queue.condvar.wait(guard).unwrap();
        }
    }

    /// Blocks up to `timeout` waiting for a raw RX chunk, disconnect, or overflow.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Vec<u8>, RxRecvError> {
        let deadline = Instant::now() + timeout;
        let mut guard = self.queue.inner.lock().unwrap();
        loop {
            if let Some(res) = Self::pop_locked(&mut guard) {
                return res;
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(RxRecvError::Timeout);
            }
            let (next_guard, wait_res) = self
                .queue
                .condvar
                .wait_timeout(guard, deadline - now)
                .unwrap();
            guard = next_guard;
            if wait_res.timed_out()
                && let Some(res) = Self::pop_locked(&mut guard)
            {
                return res;
            }
        }
    }

    fn pop_locked(guard: &mut RxQueueInner) -> Option<Result<Vec<u8>, RxRecvError>> {
        if guard.overflowed {
            return Some(Err(RxRecvError::Overflow {
                max_chunks: guard.max_chunks,
                max_bytes: guard.max_bytes,
            }));
        }
        if let Some(chunk) = guard.chunks.pop_front() {
            guard.queued_bytes = guard.queued_bytes.saturating_sub(chunk.len());
            return Some(Ok(chunk));
        }
        if let Some(reason) = &guard.disconnected {
            return Some(Err(RxRecvError::Disconnected(reason.clone())));
        }
        None
    }
}

impl Drop for RxSubscriber {
    fn drop(&mut self) {
        let mut guard = self.queue.inner.lock().unwrap();
        guard.closed = true;
        guard.chunks.clear();
        guard.queued_bytes = 0;
        self.queue.condvar.notify_all();
    }
}

struct ServiceState {
    generation: u64,
    connected: bool,
    tx_sender: Option<SyncSender<WriterCommand>>,
    writer_thread: Option<JoinHandle<()>>,
    rx_subscribers: Vec<Arc<RxSubscriberQueue>>,
    disconnect_listeners: Vec<Sender<DisconnectEvent>>,
}

struct SessionServiceInner {
    state: Mutex<ServiceState>,
    tx_completed: Mutex<VecDeque<TxCompletedEvent>>,
}

impl Drop for SessionServiceInner {
    fn drop(&mut self) {
        let writer_thread = {
            let mut state = self.state.lock().unwrap();
            state.connected = false;
            state.tx_sender.take();
            for sub in state.rx_subscribers.drain(..) {
                sub.mark_disconnected(None);
            }
            state.writer_thread.take()
        };
        if let Some(handle) = writer_thread
            && thread::current().id() != handle.thread().id()
        {
            let _ = handle.join();
        }
    }
}

/// Decoupled per-session service managing connection generation, unified FIFO TX queue,
/// and independent raw RX broadcast.
#[derive(Clone)]
pub struct SessionService {
    inner: Arc<SessionServiceInner>,
}

impl Default for SessionService {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SessionServiceInner {
                state: Mutex::new(ServiceState {
                    generation: 0,
                    connected: false,
                    tx_sender: None,
                    writer_thread: None,
                    rx_subscribers: Vec::new(),
                    disconnect_listeners: Vec::new(),
                }),
                tx_completed: Mutex::new(VecDeque::new()),
            }),
        }
    }

    pub fn generation(&self) -> u64 {
        self.inner.state.lock().unwrap().generation
    }

    pub fn is_connected(&self) -> bool {
        self.inner.state.lock().unwrap().connected
    }

    /// Attaches a new [`PortWriter`], incrementing `connection_generation` and spawning
    /// a dedicated FIFO writer thread. Any previous active connection is disconnected first.
    pub fn attach_writer<W: PortWriter>(&self, writer: W) -> u64 {
        self.disconnect(None);

        let (tx_sender, tx_rx) = mpsc::sync_channel(MAX_TX_QUEUE_DEPTH);
        let mut state = self.inner.state.lock().unwrap();
        state.generation += 1;
        let generation = state.generation;
        state.connected = true;
        state.tx_sender = Some(tx_sender);

        let inner_clone = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name(format!("sasha-serial-tx-gen{generation}"))
            .spawn(move || run_writer_loop(writer, generation, tx_rx, inner_clone))
            .expect("failed to spawn serial TX writer thread");
        state.writer_thread = Some(handle);
        generation
    }

    /// Disconnects the current session (if connected), incrementing `connection_generation`,
    /// rejecting any queued TX items, notifying all RX subscribers and disconnect listeners,
    /// and waiting for the writer thread to release the port handle.
    pub fn disconnect(&self, reason: Option<String>) {
        let writer_thread = {
            let mut state = self.inner.state.lock().unwrap();
            if !state.connected {
                return;
            }
            state.connected = false;
            state.generation += 1;
            let generation = state.generation;
            state.tx_sender.take();

            for sub in state.rx_subscribers.drain(..) {
                sub.mark_disconnected(reason.clone());
            }
            state.disconnect_listeners.retain(|listener| {
                listener
                    .send(DisconnectEvent {
                        generation,
                        reason: reason.clone(),
                    })
                    .is_ok()
            });
            state.writer_thread.take()
        };

        if let Some(handle) = writer_thread
            && thread::current().id() != handle.thread().id()
        {
            let _ = handle.join();
        }
    }

    /// Enqueues `bytes` for sequential transmission on `generation`.
    /// Non-blocking; returns a receiver that yields the actual write result.
    pub fn enqueue_tx(
        &self,
        generation: u64,
        bytes: Vec<u8>,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<Receiver<Result<(), SessionTxError>>, SessionTxError> {
        if cancel_token
            .as_ref()
            .is_some_and(|token| token.load(Ordering::Acquire))
        {
            return Err(SessionTxError::Cancelled);
        }

        let sender = {
            let state = self.inner.state.lock().unwrap();
            if !state.connected {
                return Err(SessionTxError::Disconnected);
            }
            if state.generation != generation {
                return Err(SessionTxError::StaleGeneration {
                    expected: state.generation,
                    actual: generation,
                });
            }
            match state.tx_sender.as_ref() {
                Some(sender) => sender.clone(),
                None => return Err(SessionTxError::Disconnected),
            }
        };

        let (reply_tx, reply_rx) = mpsc::channel();
        let cmd = WriterCommand::Tx(TxCommand {
            generation,
            bytes,
            cancel_token,
            reply_tx,
        });
        match sender.try_send(cmd) {
            Ok(()) => Ok(reply_rx),
            Err(TrySendError::Full(_)) => Err(SessionTxError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(SessionTxError::Disconnected),
        }
    }

    /// Enqueues a DTR/RTS level change on `generation`, serialized with TX data.
    pub fn enqueue_control(
        &self,
        generation: u64,
        line: ControlLine,
        level: bool,
    ) -> Result<Receiver<Result<(), SessionTxError>>, SessionTxError> {
        let sender = {
            let state = self.inner.state.lock().unwrap();
            if !state.connected {
                return Err(SessionTxError::Disconnected);
            }
            if state.generation != generation {
                return Err(SessionTxError::StaleGeneration {
                    expected: state.generation,
                    actual: generation,
                });
            }
            match state.tx_sender.as_ref() {
                Some(sender) => sender.clone(),
                None => return Err(SessionTxError::Disconnected),
            }
        };
        let (reply_tx, reply_rx) = mpsc::channel();
        let cmd = WriterCommand::Control {
            generation,
            line,
            level,
            reply_tx,
        };
        match sender.try_send(cmd) {
            Ok(()) => Ok(reply_rx),
            Err(TrySendError::Full(_)) => Err(SessionTxError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(SessionTxError::Disconnected),
        }
    }

    /// Sets a modem output line and blocks until the writer thread applied it.
    pub fn set_control_blocking(
        &self,
        generation: u64,
        line: ControlLine,
        level: bool,
    ) -> Result<(), SessionTxError> {
        let reply_rx = self.enqueue_control(generation, line, level)?;
        reply_rx.recv().unwrap_or(Err(SessionTxError::Disconnected))
    }

    /// Enqueues `bytes` and blocks the calling thread until the write finishes.
    pub fn send_tx_blocking(
        &self,
        generation: u64,
        bytes: Vec<u8>,
        cancel_token: Option<Arc<AtomicBool>>,
    ) -> Result<(), SessionTxError> {
        let reply_rx = self.enqueue_tx(generation, bytes, cancel_token)?;
        reply_rx.recv().unwrap_or(Err(SessionTxError::Disconnected))
    }

    /// Registers an independent raw RX subscriber for `generation`.
    pub fn subscribe_rx(&self, generation: u64) -> Result<RxSubscriber, RxSubscribeError> {
        self.subscribe_rx_with_limits(
            generation,
            RX_SUBSCRIBER_MAX_CHUNKS,
            RX_SUBSCRIBER_MAX_BYTES,
        )
    }

    /// Registers an independent raw RX subscriber with custom queue limits.
    pub fn subscribe_rx_with_limits(
        &self,
        generation: u64,
        max_chunks: usize,
        max_bytes: usize,
    ) -> Result<RxSubscriber, RxSubscribeError> {
        let mut state = self.inner.state.lock().unwrap();
        if !state.connected {
            return Err(RxSubscribeError::Disconnected);
        }
        if state.generation != generation {
            return Err(RxSubscribeError::StaleGeneration {
                expected: state.generation,
                actual: generation,
            });
        }
        let queue = Arc::new(RxSubscriberQueue::new(max_chunks, max_bytes));
        state.rx_subscribers.push(Arc::clone(&queue));
        Ok(RxSubscriber { queue, generation })
    }

    /// Broadcasts raw RX bytes to all active subscribers for `generation`.
    pub fn publish_rx(&self, generation: u64, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let subscribers: Vec<Arc<RxSubscriberQueue>> = {
            let mut state = self.inner.state.lock().unwrap();
            if !state.connected || state.generation != generation {
                return;
            }
            state.rx_subscribers.retain(|sub| !sub.is_closed());
            state.rx_subscribers.clone()
        };
        for sub in subscribers {
            sub.push_bytes(bytes);
        }
    }

    /// Registers a lifecycle listener notified on connection disconnect.
    pub fn subscribe_disconnect(&self) -> Receiver<DisconnectEvent> {
        let (tx, rx) = mpsc::channel();
        let mut state = self.inner.state.lock().unwrap();
        state.disconnect_listeners.push(tx);
        rx
    }

    /// Drains all completed TX events since the last call (each successful write appears once).
    pub fn drain_tx_completed(&self) -> Vec<TxCompletedEvent> {
        let mut completed = self.inner.tx_completed.lock().unwrap();
        completed.drain(..).collect()
    }
}

fn run_writer_loop<W: PortWriter>(
    mut writer: W,
    writer_generation: u64,
    rx: Receiver<WriterCommand>,
    inner: Arc<SessionServiceInner>,
) {
    while let Ok(item) = rx.recv() {
        let cmd = match item {
            WriterCommand::Control {
                generation,
                line,
                level,
                reply_tx,
            } => {
                let is_current = {
                    let state = inner.state.lock().unwrap();
                    state.connected
                        && state.generation == writer_generation
                        && generation == writer_generation
                };
                if !is_current {
                    let _ = reply_tx.send(Err(SessionTxError::Disconnected));
                    continue;
                }
                let result = match line {
                    ControlLine::Dtr => writer.set_dtr(level),
                    ControlLine::Rts => writer.set_rts(level),
                };
                let _ = reply_tx.send(result.map_err(SessionTxError::WriteFailed));
                continue;
            }
            WriterCommand::Tx(cmd) => cmd,
        };
        let is_current = {
            let state = inner.state.lock().unwrap();
            state.connected
                && state.generation == writer_generation
                && cmd.generation == writer_generation
        };
        if !is_current {
            let _ = cmd.reply_tx.send(Err(SessionTxError::Disconnected));
            continue;
        }

        if cmd
            .cancel_token
            .as_ref()
            .is_some_and(|token| token.load(Ordering::Acquire))
        {
            let _ = cmd.reply_tx.send(Err(SessionTxError::Cancelled));
            continue;
        }

        match writer.write_all_and_flush(&cmd.bytes) {
            Ok(()) => {
                {
                    let mut completed = inner.tx_completed.lock().unwrap();
                    completed.push_back(TxCompletedEvent {
                        generation: cmd.generation,
                        bytes: cmd.bytes,
                    });
                }
                let _ = cmd.reply_tx.send(Ok(()));
            }
            Err(err) => {
                let _ = cmd.reply_tx.send(Err(SessionTxError::WriteFailed(err)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    type RecordedWrites = Arc<Mutex<Vec<Vec<u8>>>>;
    type WriteGate = (mpsc::Sender<()>, Arc<Mutex<Option<mpsc::Receiver<()>>>>);

    struct MockPortWriter {
        writes: RecordedWrites,
        fail_next: Arc<AtomicBool>,
        gate: Option<WriteGate>,
    }

    impl MockPortWriter {
        fn new() -> (Self, RecordedWrites, Arc<AtomicBool>) {
            let writes = Arc::new(Mutex::new(Vec::new()));
            let fail_next = Arc::new(AtomicBool::new(false));
            (
                Self {
                    writes: Arc::clone(&writes),
                    fail_next: Arc::clone(&fail_next),
                    gate: None,
                },
                writes,
                fail_next,
            )
        }

        fn with_gate() -> (Self, RecordedWrites, mpsc::Receiver<()>, mpsc::Sender<()>) {
            let writes = Arc::new(Mutex::new(Vec::new()));
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            (
                Self {
                    writes: Arc::clone(&writes),
                    fail_next: Arc::new(AtomicBool::new(false)),
                    gate: Some((entered_tx, Arc::new(Mutex::new(Some(release_rx))))),
                },
                writes,
                entered_rx,
                release_tx,
            )
        }
    }

    impl PortWriter for MockPortWriter {
        fn write_all_and_flush(&mut self, data: &[u8]) -> Result<(), String> {
            if let Some((entered_tx, release_rx_holder)) = &self.gate {
                let _ = entered_tx.send(());
                if let Some(rx) = release_rx_holder.lock().unwrap().take() {
                    let _ = rx.recv();
                }
            }
            if self.fail_next.load(Ordering::Acquire) {
                return Err("mock write error".to_string());
            }
            self.writes.lock().unwrap().push(data.to_vec());
            Ok(())
        }
    }

    struct OrderedPortWriter {
        log: Arc<Mutex<Vec<String>>>,
        fail_control: bool,
    }

    impl PortWriter for OrderedPortWriter {
        fn write_all_and_flush(&mut self, data: &[u8]) -> Result<(), String> {
            self.log
                .lock()
                .unwrap()
                .push(format!("tx:{}", String::from_utf8_lossy(data)));
            Ok(())
        }

        fn set_dtr(&mut self, level: bool) -> Result<(), String> {
            if self.fail_control {
                return Err("dtr unsupported".to_string());
            }
            self.log.lock().unwrap().push(format!("dtr:{level}"));
            Ok(())
        }

        fn set_rts(&mut self, level: bool) -> Result<(), String> {
            self.log.lock().unwrap().push(format!("rts:{level}"));
            Ok(())
        }
    }

    #[test]
    fn control_commands_are_serialized_with_tx_in_queue_order() {
        let service = SessionService::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        let gen1 = service.attach_writer(OrderedPortWriter {
            log: Arc::clone(&log),
            fail_control: false,
        });

        let r1 = service.enqueue_tx(gen1, b"A".to_vec(), None).unwrap();
        let r2 = service
            .enqueue_control(gen1, ControlLine::Dtr, false)
            .unwrap();
        let r3 = service.enqueue_tx(gen1, b"B".to_vec(), None).unwrap();
        let r4 = service
            .enqueue_control(gen1, ControlLine::Rts, true)
            .unwrap();
        for r in [r1, r2, r3, r4] {
            assert_eq!(r.recv().unwrap(), Ok(()));
        }
        assert_eq!(
            *log.lock().unwrap(),
            vec!["tx:A", "dtr:false", "tx:B", "rts:true"]
        );
        assert!(service.is_connected());
    }

    #[test]
    fn control_failure_reports_error_and_keeps_connection() {
        let service = SessionService::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        let gen1 = service.attach_writer(OrderedPortWriter {
            log: Arc::clone(&log),
            fail_control: true,
        });

        let err = service
            .set_control_blocking(gen1, ControlLine::Dtr, true)
            .unwrap_err();
        assert_eq!(err, SessionTxError::WriteFailed("dtr unsupported".into()));
        assert!(service.is_connected());
        assert_eq!(service.send_tx_blocking(gen1, b"X".to_vec(), None), Ok(()));

        service.disconnect(None);
        assert_eq!(
            service
                .enqueue_control(gen1, ControlLine::Dtr, true)
                .unwrap_err(),
            SessionTxError::Disconnected
        );
    }

    #[test]
    fn fifo_non_interleaved_tx_ordering_and_single_completion_event() {
        let service = SessionService::new();
        assert!(!service.is_connected());
        assert_eq!(service.generation(), 0);

        let (writer, writes, _) = MockPortWriter::new();
        let gen1 = service.attach_writer(writer);
        assert_eq!(gen1, 1);
        assert!(service.is_connected());

        let r1 = service
            .enqueue_tx(gen1, b"FIRST".to_vec(), None)
            .expect("enqueue 1");
        let r2 = service
            .enqueue_tx(gen1, b"SECOND".to_vec(), None)
            .expect("enqueue 2");
        let r3 = service
            .enqueue_tx(gen1, b"THIRD".to_vec(), None)
            .expect("enqueue 3");

        assert_eq!(r1.recv().unwrap(), Ok(()));
        assert_eq!(r2.recv().unwrap(), Ok(()));
        assert_eq!(r3.recv().unwrap(), Ok(()));

        let recorded_writes = writes.lock().unwrap().clone();
        assert_eq!(
            recorded_writes,
            vec![b"FIRST".to_vec(), b"SECOND".to_vec(), b"THIRD".to_vec()]
        );

        let completed = service.drain_tx_completed();
        assert_eq!(
            completed,
            vec![
                TxCompletedEvent {
                    generation: gen1,
                    bytes: b"FIRST".to_vec(),
                },
                TxCompletedEvent {
                    generation: gen1,
                    bytes: b"SECOND".to_vec(),
                },
                TxCompletedEvent {
                    generation: gen1,
                    bytes: b"THIRD".to_vec(),
                },
            ]
        );
        assert!(service.drain_tx_completed().is_empty());
    }

    #[test]
    fn stale_generation_and_disconnected_requests_are_rejected() {
        let service = SessionService::new();
        assert_eq!(
            service.enqueue_tx(0, b"AT".to_vec(), None).unwrap_err(),
            SessionTxError::Disconnected
        );
        assert_eq!(
            service.subscribe_rx(0).err(),
            Some(RxSubscribeError::Disconnected)
        );

        let (writer1, _, _) = MockPortWriter::new();
        let gen1 = service.attach_writer(writer1);
        assert_eq!(gen1, 1);

        assert_eq!(
            service.enqueue_tx(0, b"AT".to_vec(), None).unwrap_err(),
            SessionTxError::StaleGeneration {
                expected: 1,
                actual: 0
            }
        );
        assert_eq!(
            service.subscribe_rx(0).err(),
            Some(RxSubscribeError::StaleGeneration {
                expected: 1,
                actual: 0
            })
        );

        service.disconnect(Some("manual".to_string()));
        assert!(!service.is_connected());
        assert_eq!(service.generation(), 2);

        assert_eq!(
            service.enqueue_tx(gen1, b"AT".to_vec(), None).unwrap_err(),
            SessionTxError::Disconnected
        );

        let (writer2, _, _) = MockPortWriter::new();
        let gen2 = service.attach_writer(writer2);
        assert_eq!(gen2, 3);
        assert_eq!(
            service.enqueue_tx(gen1, b"OLD".to_vec(), None).unwrap_err(),
            SessionTxError::StaleGeneration {
                expected: 3,
                actual: gen1
            }
        );
    }

    #[test]
    fn pre_write_cancellation_rejects_without_writing_or_recording_tx() {
        let service = SessionService::new();
        let (writer, writes, entered_rx, release_tx) = MockPortWriter::with_gate();
        let gen1 = service.attach_writer(writer);

        // Pre-cancelled token is rejected at enqueue time.
        let pre_cancelled = Arc::new(AtomicBool::new(true));
        assert_eq!(
            service
                .enqueue_tx(gen1, b"SKIP".to_vec(), Some(pre_cancelled))
                .unwrap_err(),
            SessionTxError::Cancelled
        );

        // Item 1 blocks in writer gate; item 2 is queued and cancelled before execution.
        let r1 = service
            .enqueue_tx(gen1, b"INFLIGHT".to_vec(), None)
            .expect("enqueue 1");
        entered_rx.recv().unwrap();

        let token2 = Arc::new(AtomicBool::new(false));
        let r2 = service
            .enqueue_tx(gen1, b"CANCEL_QUEUED".to_vec(), Some(Arc::clone(&token2)))
            .expect("enqueue 2");
        token2.store(true, Ordering::Release);

        release_tx.send(()).unwrap();
        assert_eq!(r1.recv().unwrap(), Ok(()));
        assert_eq!(r2.recv().unwrap(), Err(SessionTxError::Cancelled));

        assert_eq!(*writes.lock().unwrap(), vec![b"INFLIGHT".to_vec()]);
        let completed = service.drain_tx_completed();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].bytes, b"INFLIGHT");
    }

    #[test]
    fn write_error_reports_failure_and_does_not_record_tx() {
        let service = SessionService::new();
        let (writer, writes, fail_next) = MockPortWriter::new();
        let gen1 = service.attach_writer(writer);
        fail_next.store(true, Ordering::Release);

        let rx = service
            .enqueue_tx(gen1, b"FAIL".to_vec(), None)
            .expect("enqueue");
        let res = rx.recv().unwrap();
        assert_eq!(
            res,
            Err(SessionTxError::WriteFailed("mock write error".to_string()))
        );
        assert!(writes.lock().unwrap().is_empty());
        assert!(service.drain_tx_completed().is_empty());
    }

    #[test]
    fn multi_subscriber_raw_rx_broadcast_does_not_steal() {
        let service = SessionService::new();
        let (writer, _, _) = MockPortWriter::new();
        let gen1 = service.attach_writer(writer);

        let sub_a = service.subscribe_rx(gen1).expect("sub A");
        let sub_b = service.subscribe_rx(gen1).expect("sub B");

        service.publish_rx(gen1, b"\x00\xFFhello");
        service.publish_rx(gen1, b"world");
        // Stale generation publish is ignored.
        service.publish_rx(gen1 + 1, b"ignored");

        assert_eq!(sub_a.try_recv(), Ok(b"\x00\xFFhello".to_vec()));
        assert_eq!(sub_a.try_recv(), Ok(b"world".to_vec()));
        assert_eq!(sub_a.try_recv(), Err(RxRecvError::Empty));

        assert_eq!(sub_b.try_recv(), Ok(b"\x00\xFFhello".to_vec()));
        assert_eq!(sub_b.try_recv(), Ok(b"world".to_vec()));
        assert_eq!(sub_b.try_recv(), Err(RxRecvError::Empty));
    }

    #[test]
    fn bounded_rx_queue_surfaces_overflow_explicitly() {
        let service = SessionService::new();
        let (writer, _, _) = MockPortWriter::new();
        let gen1 = service.attach_writer(writer);

        let sub = service
            .subscribe_rx_with_limits(gen1, 2, 10)
            .expect("sub with limits");

        service.publish_rx(gen1, b"1234");
        service.publish_rx(gen1, b"5678");
        // Exceeds max_chunks (2)
        service.publish_rx(gen1, b"9");

        assert_eq!(
            sub.try_recv(),
            Err(RxRecvError::Overflow {
                max_chunks: 2,
                max_bytes: 10
            })
        );

        let sub_bytes = service
            .subscribe_rx_with_limits(gen1, 16, 5)
            .expect("sub byte limit");
        service.publish_rx(gen1, b"1234");
        service.publish_rx(gen1, b"56");
        assert_eq!(
            sub_bytes.try_recv(),
            Err(RxRecvError::Overflow {
                max_chunks: 16,
                max_bytes: 5
            })
        );
    }

    #[test]
    fn disconnect_rejects_pending_tx_and_notifies_subscribers_and_listeners() {
        let service = SessionService::new();
        let (writer, writes, entered_rx, release_tx) = MockPortWriter::with_gate();
        let gen1 = service.attach_writer(writer);

        let sub = service.subscribe_rx(gen1).expect("sub");
        let disconnect_rx = service.subscribe_disconnect();

        // Block writer on item 1, queue item 2, then disconnect.
        let r1 = service
            .enqueue_tx(gen1, b"ONE".to_vec(), None)
            .expect("enqueue 1");
        entered_rx.recv().unwrap();
        let r2 = service
            .enqueue_tx(gen1, b"TWO".to_vec(), None)
            .expect("enqueue 2");

        // Release gate from another thread while disconnect runs, or release then disconnect.
        let releaser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            let _ = release_tx.send(());
        });

        service.disconnect(Some("port closed".to_string()));
        releaser.join().unwrap();

        assert_eq!(r1.recv().unwrap(), Ok(()));
        assert_eq!(r2.recv().unwrap(), Err(SessionTxError::Disconnected));
        assert_eq!(*writes.lock().unwrap(), vec![b"ONE".to_vec()]);

        assert_eq!(
            sub.try_recv(),
            Err(RxRecvError::Disconnected(Some("port closed".to_string())))
        );
        assert_eq!(
            disconnect_rx.try_recv().unwrap(),
            DisconnectEvent {
                generation: 2,
                reason: Some("port closed".to_string())
            }
        );
    }
}
