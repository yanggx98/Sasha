//! Pure logic for the send card's timed (repeating) send feature.
//!
//! Scheduling, validation and stop rules live here so they can be unit tested
//! without a UI; `ui::workspace` only wires them to the session and the TX queue.

use std::time::{Duration, Instant};

use super::log::SendPayloadError;
use crate::session_service::SessionTxError;

/// Smallest accepted repeat interval, in milliseconds.
pub const MIN_TIMER_INTERVAL_MS: u32 = 10;
/// Largest accepted repeat interval, in milliseconds (one hour).
pub const MAX_TIMER_INTERVAL_MS: u32 = 3_600_000;
/// Interval used before the user has saved one.
pub const DEFAULT_TIMER_INTERVAL_MS: u32 = 1000;

/// Why an interval field value was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalError {
    Invalid,
    TooSmall,
    TooLarge,
}

/// Parses the interval field (whole milliseconds) and enforces the allowed range.
pub fn parse_interval_ms(raw: &str) -> Result<u32, IntervalError> {
    let value: u64 = raw
        .trim()
        .parse()
        .map_err(|_| IntervalError::Invalid)?;
    if value < u64::from(MIN_TIMER_INTERVAL_MS) {
        Err(IntervalError::TooSmall)
    } else if value > u64::from(MAX_TIMER_INTERVAL_MS) {
        Err(IntervalError::TooLarge)
    } else {
        Ok(value as u32)
    }
}

/// Result of committing the interval field's draft (Enter / blur).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftCommit {
    /// Valid draft: becomes the interval and applies from the next cycle.
    Commit(u32),
    /// Invalid draft: keep the previous interval, restore the field, report `error`.
    Reject { restore_ms: u32, error: IntervalError },
}

/// Commits the interval draft against the currently committed interval.
pub fn commit_interval_draft(draft: &str, committed_ms: u32) -> DraftCommit {
    match parse_interval_ms(draft) {
        Ok(ms) => DraftCommit::Commit(ms),
        Err(error) => DraftCommit::Reject {
            restore_ms: committed_ms,
            error,
        },
    }
}

/// Whether the field holds an uncommitted draft (differs from the committed value).
pub fn draft_is_pending(draft: &str, committed_ms: u32) -> bool {
    parse_interval_ms(draft) != Ok(committed_ms)
}

/// Interval a start click uses: the current draft (validated), never a stale
/// committed value. Without an input widget the committed value is used.
pub fn interval_for_start(draft: Option<&str>, committed_ms: u32) -> Result<u32, IntervalError> {
    match draft {
        Some(raw) => parse_interval_ms(raw),
        None => Ok(committed_ms),
    }
}

/// Forces a persisted interval into the supported range.
pub fn clamp_interval_ms(value: u32) -> u32 {
    value.clamp(MIN_TIMER_INTERVAL_MS, MAX_TIMER_INTERVAL_MS)
}

/// Per-session timed-send UI/runtime state. The running flag is never persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedSendState {
    /// Timed-send mode is on (capsule enabled). Implied by `running`.
    pub enabled: bool,
    pub running: bool,
    /// Latest valid interval; read before scheduling each following cycle.
    pub interval_ms: u32,
}

impl TimedSendState {
    pub fn new(interval_ms: u32) -> Self {
        Self {
            enabled: false,
            running: false,
            interval_ms: clamp_interval_ms(interval_ms),
        }
    }

    pub fn mode(&self) -> TimerMode {
        if self.running {
            TimerMode::Running
        } else if self.enabled {
            TimerMode::Armed
        } else {
            TimerMode::Off
        }
    }
}

/// The three states of the timed-send feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerMode {
    /// Not enabled: the send button is a normal send.
    Off,
    /// Enabled but idle: the send button is Play.
    Armed,
    /// Enabled and sending: the send button is Pause.
    Running,
}

/// What a click on the capsule's icon segment does (it only toggles enable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconAction {
    Enable,
    /// Cancel the enabled state; a running timer is stopped as well.
    Disable,
    /// Not connected and not enabled: greyed out, ignores clicks.
    Ignore,
}

/// Enabling needs a connection; an enabled capsule can always be cancelled.
pub fn icon_click_action(enabled: bool, connected: bool) -> IconAction {
    if enabled {
        IconAction::Disable
    } else if connected {
        IconAction::Enable
    } else {
        IconAction::Ignore
    }
}

/// What a click on the send button does in each state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendButtonAction {
    SendNow,
    StartTimer,
    /// Pause: stop sending but stay enabled.
    StopTimer,
}

pub fn send_button_action(mode: TimerMode) -> SendButtonAction {
    match mode {
        TimerMode::Off => SendButtonAction::SendNow,
        TimerMode::Armed => SendButtonAction::StartTimer,
        TimerMode::Running => SendButtonAction::StopTimer,
    }
}

/// What Enter (Ctrl+Enter) in the send input does in each state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterAction {
    SendNow,
    StartTimer,
    /// Running: ignored with a hint; never treated as pause to avoid accidents.
    IgnoreRunning,
}

pub fn enter_action(mode: TimerMode) -> EnterAction {
    match mode {
        TimerMode::Off => EnterAction::SendNow,
        TimerMode::Armed => EnterAction::StartTimer,
        TimerMode::Running => EnterAction::IgnoreRunning,
    }
}

/// Manual sends (quick send panel, `Alt+1..9`) stay disabled while enabled,
/// including standby.
pub fn manual_send_blocked(mode: TimerMode) -> bool {
    mode != TimerMode::Off
}

/// Why a running timer stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCause {
    /// User pressed the pause button.
    Paused,
    /// Per-cycle validation failed (empty content / invalid HEX).
    ValidationFailed,
    /// TX queue full or write failure.
    TxFailed,
    Disconnected,
    /// User cancelled the enabled state.
    Disabled,
}

/// Stop cause for a per-cycle validation failure: a dropped connection cancels the
/// enabled state, anything else (empty content, invalid HEX) keeps it.
pub fn stop_cause_for(err: &SendPayloadError) -> StopCause {
    match err {
        SendPayloadError::Disconnected => StopCause::Disconnected,
        _ => StopCause::ValidationFailed,
    }
}

/// Whether the capsule stays enabled (button back to Play) after the stop.
pub fn enabled_after_stop(cause: StopCause) -> bool {
    matches!(
        cause,
        StopCause::Paused | StopCause::ValidationFailed | StopCause::TxFailed
    )
}

impl Default for TimedSendState {
    fn default() -> Self {
        Self::new(DEFAULT_TIMER_INTERVAL_MS)
    }
}

/// What a scheduled cycle should do after re-reading the current draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickOutcome {
    Send(Vec<u8>),
    Stop(SendPayloadError),
}

/// Maps the draft validation result of one cycle to an action. Any validation
/// failure (empty content, invalid HEX, disconnected) stops the timer.
pub fn tick_outcome(prepared: Result<Vec<u8>, SendPayloadError>) -> TickOutcome {
    match prepared {
        Ok(bytes) => TickOutcome::Send(bytes),
        Err(err) => TickOutcome::Stop(err),
    }
}

/// Whether a TX failure must stop the timer. A cancelled write only happens
/// during disconnect, which already stops the timer.
pub fn should_stop_on_tx_error(err: &SessionTxError) -> bool {
    !matches!(err, SessionTxError::Cancelled)
}

/// Whether a TX reply still belongs to the running timer. Replies from an earlier
/// run (e.g. before a quick reconnect and restart) or after a stop are stale and
/// must be ignored instead of stopping or reporting on the current run.
pub fn timer_reply_is_current(reply_run: u64, current_run: u64, running: bool) -> bool {
    running && reply_run == current_run
}

/// Most timed sends allowed to wait in the TX queue (enqueued, write not yet
/// finished). A little slack absorbs reply latency; more would just pile up.
pub const MAX_PENDING_TIMED_SENDS: u32 = 4;

/// Backpressure: when the port cannot keep up (low baud, stalled peer) skip the
/// cycle instead of growing the 256-deep TX queue until it overflows.
pub fn should_skip_cycle(pending: u32) -> bool {
    pending >= MAX_PENDING_TIMED_SENDS
}

/// Fixed-rate schedule: the next deadline is one `interval` after `prev`.
/// When the app fell behind (`now` is already past it) missed cycles are
/// skipped rather than replayed, so a stall never causes a burst of sends.
pub fn next_deadline(prev: Instant, now: Instant, interval: Duration) -> Instant {
    let next = prev + interval;
    if next > now {
        return next;
    }
    let interval_nanos = interval.as_nanos().max(1);
    let elapsed = now.saturating_duration_since(prev).as_nanos();
    let steps = u32::try_from(elapsed / interval_nanos + 1).unwrap_or(u32::MAX);
    prev + interval.saturating_mul(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_parsing_enforces_range() {
        assert_eq!(parse_interval_ms("1000"), Ok(1000));
        assert_eq!(parse_interval_ms("  10 "), Ok(10));
        assert_eq!(parse_interval_ms("3600000"), Ok(3_600_000));
        assert_eq!(parse_interval_ms("9"), Err(IntervalError::TooSmall));
        assert_eq!(parse_interval_ms("0"), Err(IntervalError::TooSmall));
        assert_eq!(parse_interval_ms("3600001"), Err(IntervalError::TooLarge));
        assert_eq!(
            parse_interval_ms("99999999999999"),
            Err(IntervalError::TooLarge)
        );
        assert_eq!(parse_interval_ms(""), Err(IntervalError::Invalid));
        assert_eq!(parse_interval_ms("-5"), Err(IntervalError::Invalid));
        assert_eq!(parse_interval_ms("1.5"), Err(IntervalError::Invalid));
        assert_eq!(parse_interval_ms("abc"), Err(IntervalError::Invalid));
    }

    #[test]
    fn persisted_interval_is_clamped() {
        assert_eq!(clamp_interval_ms(0), MIN_TIMER_INTERVAL_MS);
        assert_eq!(clamp_interval_ms(500), 500);
        assert_eq!(clamp_interval_ms(u32::MAX), MAX_TIMER_INTERVAL_MS);
        assert_eq!(TimedSendState::new(1).interval_ms, MIN_TIMER_INTERVAL_MS);
    }

    #[test]
    fn state_defaults_to_idle_with_default_interval() {
        let state = TimedSendState::default();
        assert!(!state.running);
        assert_eq!(state.interval_ms, DEFAULT_TIMER_INTERVAL_MS);
    }

    #[test]
    fn valid_draft_commits_and_applies_from_next_cycle() {
        assert_eq!(commit_interval_draft("250", 1000), DraftCommit::Commit(250));
        assert_eq!(commit_interval_draft(" 0500 ", 1000), DraftCommit::Commit(500));
        // Committing the same value is a no-op commit.
        assert_eq!(commit_interval_draft("1000", 1000), DraftCommit::Commit(1000));
    }

    #[test]
    fn invalid_draft_is_rejected_and_restores_previous_interval() {
        for (draft, err) in [
            ("", IntervalError::Invalid),
            ("abc", IntervalError::Invalid),
            ("1", IntervalError::TooSmall),
            ("99999999", IntervalError::TooLarge),
        ] {
            assert_eq!(
                commit_interval_draft(draft, 700),
                DraftCommit::Reject {
                    restore_ms: 700,
                    error: err
                },
                "draft {draft:?}"
            );
        }
    }

    #[test]
    fn typing_midway_values_is_only_a_pending_draft() {
        // "1000" typed from empty passes through 1, 10, 100 - none is committed
        // by typing; they only mark the draft as pending.
        for partial in ["", "1", "10", "100"] {
            assert!(draft_is_pending(partial, 5000), "{partial:?}");
        }
        assert!(!draft_is_pending("5000", 5000));
        assert!(!draft_is_pending(" 05000 ", 5000));
        assert!(draft_is_pending("abc", 5000));
    }

    #[test]
    fn start_uses_the_draft_not_a_stale_committed_value() {
        assert_eq!(interval_for_start(Some("300"), 1000), Ok(300));
        assert_eq!(
            interval_for_start(Some("5"), 1000),
            Err(IntervalError::TooSmall)
        );
        assert_eq!(
            interval_for_start(Some(""), 1000),
            Err(IntervalError::Invalid)
        );
        // No input widget yet: fall back to the committed value.
        assert_eq!(interval_for_start(None, 1000), Ok(1000));
    }

    #[test]
    fn mode_is_derived_from_enabled_and_running() {
        let mut state = TimedSendState::default();
        assert_eq!(state.mode(), TimerMode::Off);
        state.enabled = true;
        assert_eq!(state.mode(), TimerMode::Armed);
        state.running = true;
        assert_eq!(state.mode(), TimerMode::Running);
    }

    #[test]
    fn icon_click_only_toggles_enable() {
        assert_eq!(icon_click_action(false, true), IconAction::Enable);
        assert_eq!(icon_click_action(true, true), IconAction::Disable);
        // Disconnected: cannot enable, but an enabled capsule can still be cancelled.
        assert_eq!(icon_click_action(false, false), IconAction::Ignore);
        assert_eq!(icon_click_action(true, false), IconAction::Disable);
    }

    #[test]
    fn send_button_follows_the_three_states() {
        assert_eq!(send_button_action(TimerMode::Off), SendButtonAction::SendNow);
        assert_eq!(
            send_button_action(TimerMode::Armed),
            SendButtonAction::StartTimer
        );
        assert_eq!(
            send_button_action(TimerMode::Running),
            SendButtonAction::StopTimer
        );
    }

    #[test]
    fn enter_never_pauses_a_running_timer() {
        assert_eq!(enter_action(TimerMode::Off), EnterAction::SendNow);
        assert_eq!(enter_action(TimerMode::Armed), EnterAction::StartTimer);
        assert_eq!(enter_action(TimerMode::Running), EnterAction::IgnoreRunning);
    }

    #[test]
    fn manual_send_is_blocked_while_enabled_including_standby() {
        assert!(!manual_send_blocked(TimerMode::Off));
        assert!(manual_send_blocked(TimerMode::Armed));
        assert!(manual_send_blocked(TimerMode::Running));
    }

    #[test]
    fn validation_failure_keeps_enabled_but_lost_connection_cancels_it() {
        assert_eq!(
            stop_cause_for(&SendPayloadError::EmptyInput),
            StopCause::ValidationFailed
        );
        assert_eq!(
            stop_cause_for(&SendPayloadError::InvalidHex("zz")),
            StopCause::ValidationFailed
        );
        assert_eq!(
            stop_cause_for(&SendPayloadError::Disconnected),
            StopCause::Disconnected
        );
        assert!(enabled_after_stop(stop_cause_for(&SendPayloadError::EmptyInput)));
        assert!(!enabled_after_stop(stop_cause_for(&SendPayloadError::Disconnected)));
    }

    #[test]
    fn only_disconnect_and_cancel_drop_the_enabled_state() {
        assert!(enabled_after_stop(StopCause::Paused));
        assert!(enabled_after_stop(StopCause::ValidationFailed));
        assert!(enabled_after_stop(StopCause::TxFailed));
        assert!(!enabled_after_stop(StopCause::Disconnected));
        assert!(!enabled_after_stop(StopCause::Disabled));
    }

    #[test]
    fn tick_sends_valid_payload_and_stops_on_any_validation_failure() {
        assert_eq!(
            tick_outcome(Ok(vec![0x41])),
            TickOutcome::Send(vec![0x41])
        );
        assert_eq!(
            tick_outcome(Err(SendPayloadError::EmptyInput)),
            TickOutcome::Stop(SendPayloadError::EmptyInput)
        );
        assert_eq!(
            tick_outcome(Err(SendPayloadError::Disconnected)),
            TickOutcome::Stop(SendPayloadError::Disconnected)
        );
        assert!(matches!(
            tick_outcome(Err(SendPayloadError::InvalidHex("zz"))),
            TickOutcome::Stop(SendPayloadError::InvalidHex(_))
        ));
    }

    #[test]
    fn tx_failures_stop_except_cancel() {
        assert!(should_stop_on_tx_error(&SessionTxError::QueueFull));
        assert!(should_stop_on_tx_error(&SessionTxError::Disconnected));
        assert!(should_stop_on_tx_error(&SessionTxError::WriteFailed(
            "x".into()
        )));
        assert!(should_stop_on_tx_error(&SessionTxError::StaleGeneration {
            expected: 2,
            actual: 1
        }));
        assert!(!should_stop_on_tx_error(&SessionTxError::Cancelled));
    }

    #[test]
    fn stale_tx_replies_never_stop_the_current_run() {
        // Current run replies are honoured: a failure stops, cancel does not.
        let current = timer_reply_is_current(3, 3, true);
        assert!(current && should_stop_on_tx_error(&SessionTxError::WriteFailed("x".into())));
        assert!(current && !should_stop_on_tx_error(&SessionTxError::Cancelled));
        // Reply from run 2 arriving while run 3 is active is ignored.
        assert!(!timer_reply_is_current(2, 3, true));
        // Reply after the timer stopped is ignored too.
        assert!(!timer_reply_is_current(3, 3, false));
    }

    /// Models the producer (timer ticks) against a consumer that needs `write_ms`
    /// per item (baud-limited write + flush). Returns the highest queue depth.
    fn simulate_max_queue_depth(
        interval_ms: u64,
        write_ms: u64,
        duration_ms: u64,
        gated: bool,
    ) -> u32 {
        let mut queued: u32 = 0;
        let mut max_depth: u32 = 0;
        let mut next_write_done: Option<u64> = None;
        for now in 0..duration_ms {
            if next_write_done == Some(now) {
                queued -= 1;
                next_write_done = None;
            }
            if next_write_done.is_none() && queued > 0 {
                next_write_done = Some(now + write_ms);
            }
            if now % interval_ms == 0 && !(gated && should_skip_cycle(queued)) {
                queued += 1;
                max_depth = max_depth.max(queued);
            }
        }
        max_depth
    }

    #[test]
    fn unbounded_producer_overruns_the_256_deep_tx_queue() {
        // Root cause of "transmit queue is full (capacity 256)": 10 ms ticks against a
        // port that needs 25 ms per write (e.g. low baud) fill the queue in ~20 s.
        assert!(simulate_max_queue_depth(10, 25, 60_000, false) > 256);
    }

    #[test]
    fn backpressure_keeps_pending_sends_bounded_when_port_is_slower_than_interval() {
        let depth = simulate_max_queue_depth(10, 25, 60_000, true);
        assert!(depth <= MAX_PENDING_TIMED_SENDS, "depth {depth}");
        // A stalled port (never completes) must not pile up either.
        assert!(simulate_max_queue_depth(10, u64::MAX / 4, 60_000, true) <= MAX_PENDING_TIMED_SENDS);
    }

    #[test]
    fn backpressure_does_not_skip_when_port_keeps_up() {
        assert!(!should_skip_cycle(0));
        assert!(!should_skip_cycle(MAX_PENDING_TIMED_SENDS - 1));
        assert!(should_skip_cycle(MAX_PENDING_TIMED_SENDS));
        // 100 ms interval against 11 ms writes: every cycle is sent.
        assert_eq!(simulate_max_queue_depth(100, 11, 10_000, true), 1);
    }

    #[test]
    fn deadline_advances_one_interval_when_on_time() {
        let t0 = Instant::now();
        let interval = Duration::from_millis(100);
        assert_eq!(
            next_deadline(t0, t0 + Duration::from_millis(5), interval),
            t0 + interval
        );
        assert_eq!(next_deadline(t0, t0, interval), t0 + interval);
    }

    #[test]
    fn deadline_skips_missed_cycles_without_catching_up() {
        let t0 = Instant::now();
        let interval = Duration::from_millis(100);
        // Slightly late: next deadline already passed, land on the following slot.
        assert_eq!(
            next_deadline(t0, t0 + Duration::from_millis(100), interval),
            t0 + Duration::from_millis(200)
        );
        // Stalled for 3.5 cycles: skip to the next grid slot after `now`.
        assert_eq!(
            next_deadline(t0, t0 + Duration::from_millis(350), interval),
            t0 + Duration::from_millis(400)
        );
    }

    #[test]
    fn deadline_never_falls_in_the_past() {
        let t0 = Instant::now();
        let interval = Duration::from_millis(10);
        let now = t0 + Duration::from_secs(60);
        assert!(next_deadline(t0, now, interval) > now);
    }
}
