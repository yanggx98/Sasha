use std::collections::VecDeque;

use crate::model::{DataBits, Parity, SerialConfig, StopBits};

/// Sampling interval in milliseconds (4 Hz).
pub const SAMPLE_INTERVAL_MS: u64 = 250;

/// Number of samples recorded per second.
pub const SAMPLES_PER_SECOND: usize = 4;

/// Maximum retained history duration in seconds (5 minutes).
pub const MAX_WINDOW_SECS: usize = 300;

/// Maximum number of retained samples per session (+1 leading sample for smooth scrolling).
pub const MAX_SAMPLES: usize = MAX_WINDOW_SECS * SAMPLES_PER_SECOND + 1;

/// Selectable time horizon displayed by the throughput chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThroughputSpan {
    Seconds30,
    #[default]
    Minute1,
    Minutes5,
}

impl ThroughputSpan {
    pub const ALL: [Self; 3] = [Self::Seconds30, Self::Minute1, Self::Minutes5];

    pub fn seconds(self) -> usize {
        match self {
            Self::Seconds30 => 30,
            Self::Minute1 => 60,
            Self::Minutes5 => 300,
        }
    }

    pub fn sample_count(self) -> usize {
        self.seconds() * SAMPLES_PER_SECOND
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Seconds30 => "30s",
            Self::Minute1 => "1m",
            Self::Minutes5 => "5m",
        }
    }
}

/// One sampled throughput point in bytes per second.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ThroughputSample {
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
}

/// Aggregated RX/TX rate summary over a selected time span.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ThroughputSummary {
    pub current_rx: f64,
    pub current_tx: f64,
    pub peak_rx: f64,
    pub peak_tx: f64,
}

/// Bounded ring buffer of per-session RX/TX byte rates.
#[derive(Debug, Clone)]
pub struct ThroughputHistory {
    samples: VecDeque<ThroughputSample>,
    last_rx_total: u64,
    last_tx_total: u64,
}

impl Default for ThroughputHistory {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl ThroughputHistory {
    pub fn new(initial_rx: u64, initial_tx: u64) -> Self {
        Self {
            samples: VecDeque::with_capacity(MAX_SAMPLES),
            last_rx_total: initial_rx,
            last_tx_total: initial_tx,
        }
    }

    /// Records the byte delta since the previous tick as one `SAMPLE_INTERVAL_MS` bucket.
    pub fn sample(&mut self, rx_total: u64, tx_total: u64) {
        let delta_rx = rx_total.saturating_sub(self.last_rx_total);
        let delta_tx = tx_total.saturating_sub(self.last_tx_total);
        self.last_rx_total = rx_total;
        self.last_tx_total = tx_total;

        let scale = 1000.0 / SAMPLE_INTERVAL_MS as f64;
        let sample = ThroughputSample {
            rx_bytes_per_sec: delta_rx as f64 * scale,
            tx_bytes_per_sec: delta_tx as f64 * scale,
        };
        if self.samples.len() >= MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Clears recorded curve history while preserving the baseline byte totals so the next
    /// tick only counts newly transferred bytes.
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Returns exactly `span.sample_count() + 1` samples ordered oldest-to-newest, left-padded
    /// with zeros when less history has been collected than the requested span. The extra
    /// leading sample allows sub-interval smooth scrolling without edge popping.
    pub fn window_samples(&self, span: ThroughputSpan) -> Vec<ThroughputSample> {
        let needed = span.sample_count() + 1;
        let available = self.samples.len();
        let mut out = Vec::with_capacity(needed);
        if available < needed {
            out.resize(needed - available, ThroughputSample::default());
            out.extend(self.samples.iter().copied());
        } else {
            out.extend(self.samples.iter().skip(available - needed).copied());
        }
        out
    }

    /// Computes current and peak RX/TX rates over `span`.
    pub fn summary(&self, span: ThroughputSpan) -> ThroughputSummary {
        let needed = span.sample_count();
        let skip = self.samples.len().saturating_sub(needed);
        let mut peak_rx = 0.0_f64;
        let mut peak_tx = 0.0_f64;
        for sample in self.samples.iter().skip(skip) {
            peak_rx = peak_rx.max(sample.rx_bytes_per_sec);
            peak_tx = peak_tx.max(sample.tx_bytes_per_sec);
        }
        let latest = self.samples.back().copied().unwrap_or_default();
        ThroughputSummary {
            current_rx: latest.rx_bytes_per_sec,
            current_tx: latest.tx_bytes_per_sec,
            peak_rx,
            peak_tx,
        }
    }
}

/// Theoretical maximum unidirectional throughput in bytes per second for the serial framing.
pub fn theoretical_max_bytes_per_sec(config: &SerialConfig) -> f64 {
    let data_bits = match config.data_bits {
        DataBits::Five => 5.0,
        DataBits::Six => 6.0,
        DataBits::Seven => 7.0,
        DataBits::Eight => 8.0,
    };
    let parity_bits = match config.parity {
        Parity::None => 0.0,
        Parity::Odd | Parity::Even => 1.0,
    };
    let stop_bits = match config.stop_bits {
        StopBits::One => 1.0,
        StopBits::OnePointFive => 1.5,
        StopBits::Two => 2.0,
    };
    let frame_bits = 1.0 + data_bits + parity_bits + stop_bits;
    if frame_bits <= 0.0 {
        return 0.0;
    }
    config.baud_rate as f64 / frame_bits
}

/// Formats a compact framing summary such as `8N1`.
pub fn format_framing_short(config: &SerialConfig) -> String {
    let p = match config.parity {
        Parity::None => 'N',
        Parity::Odd => 'O',
        Parity::Even => 'E',
    };
    format!(
        "{}{}{}",
        config.data_bits.label(),
        p,
        config.stop_bits.label()
    )
}

/// Formats a byte rate using human-readable units (`B/s`, `KB/s`, `MB/s`).
pub fn format_rate(bytes_per_sec: f64) -> String {
    let value = bytes_per_sec.max(0.0);
    if value < 1024.0 {
        format!("{value:.0} B/s")
    } else if value < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", value / 1024.0)
    } else {
        format!("{:.2} MB/s", value / (1024.0 * 1024.0))
    }
}

/// Formats a byte count using compact units (`B`, `KB`, `MB`).
pub fn format_bytes_compact(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Chooses a clean rounded Y-axis ceiling in the active display unit (`B/s`, `KB/s`, `MB/s`)
/// so quarter-step grid labels land on tidy numbers (e.g. `2.0 / 4.0 / 6.0 / 8.0 KB/s`).
pub fn nice_y_ceiling(peak: f64) -> f64 {
    if !peak.is_finite() || peak <= 0.0 {
        return 100.0;
    }
    let target = peak * 1.15;
    let unit = if target >= 1024.0 * 1024.0 {
        1024.0 * 1024.0
    } else if target >= 1024.0 {
        1024.0
    } else {
        1.0
    };
    let scaled = target / unit;
    let exponent = scaled.log10().floor();
    let magnitude = 10.0_f64.powf(exponent);
    let normalized = scaled / magnitude;
    let nice = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 4.0 {
        4.0
    } else if normalized <= 5.0 {
        5.0
    } else if normalized <= 8.0 {
        8.0
    } else {
        10.0
    };
    (nice * magnitude * unit).max(10.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FlowControl;

    #[test]
    fn sample_computes_rate_and_caps_ring_buffer() {
        let mut history = ThroughputHistory::new(100, 50);
        history.sample(150, 75);
        let summary = history.summary(ThroughputSpan::Seconds30);
        // 50 bytes in 250ms => 200 B/s; 25 bytes in 250ms => 100 B/s.
        assert_eq!(summary.current_rx, 200.0);
        assert_eq!(summary.current_tx, 100.0);
        assert_eq!(summary.peak_rx, 200.0);
        assert_eq!(summary.peak_tx, 100.0);

        for i in 1..=(MAX_SAMPLES + 50) {
            history.sample(150 + i as u64 * 10, 75);
        }
        assert_eq!(
            history.window_samples(ThroughputSpan::Minutes5).len(),
            MAX_SAMPLES
        );
    }

    #[test]
    fn window_samples_left_pads_zeros_and_clear_keeps_baseline() {
        let mut history = ThroughputHistory::new(0, 0);
        history.sample(25, 0);
        let samples = history.window_samples(ThroughputSpan::Seconds30);
        assert_eq!(samples.len(), 121);
        assert_eq!(samples[0].rx_bytes_per_sec, 0.0);
        assert_eq!(samples[120].rx_bytes_per_sec, 100.0);

        history.clear();
        history.sample(25, 0);
        assert_eq!(history.summary(ThroughputSpan::Seconds30).current_rx, 0.0);
    }

    #[test]
    fn theoretical_bandwidth_matches_standard_framing() {
        let cfg = SerialConfig {
            port: "COM3".into(),
            baud_rate: 115_200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
            ..SerialConfig::default()
        };
        assert!((theoretical_max_bytes_per_sec(&cfg) - 11_520.0).abs() < 1e-6);
        assert_eq!(format_framing_short(&cfg), "8N1");
    }

    #[test]
    fn formatters_and_nice_y_ceiling_behave_cleanly() {
        assert_eq!(format_rate(0.0), "0 B/s");
        assert_eq!(format_rate(512.0), "512 B/s");
        assert_eq!(format_rate(2048.0), "2.0 KB/s");
        assert_eq!(format_bytes_compact(900), "900 B");
        assert_eq!(format_bytes_compact(2048), "2.0 KB");

        assert_eq!(nice_y_ceiling(0.0), 100.0);
        assert_eq!(nice_y_ceiling(80.0), 100.0);
        // 5.5 KB/s * 1.15 = 6.325 KB/s => snaps to 8.0 KB/s (clean 2.0 KB/s steps).
        assert_eq!(nice_y_ceiling(5.5 * 1024.0), 8.0 * 1024.0);
        assert_eq!(nice_y_ceiling(1.2 * 1024.0), 2.0 * 1024.0);
    }
}
