#![allow(dead_code)]

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::i18n::{
    self, Locale, serial_action_clone_failed, serial_action_flush_failed,
    serial_action_open_failed, serial_action_spawn_failed, serial_action_write_failed, tr,
};
use crate::model::{DataBits, FlowControl, ModemSignals, Parity, SerialConfig, StopBits};

const READ_TIMEOUT: Duration = Duration::from_millis(50);
const READ_BUFFER_SIZE: usize = 4096;
/// Minimum interval between modem input line samples.
const SIGNAL_POLL_INTERVAL: Duration = Duration::from_millis(150);

/// A serial port discovered on the host machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedPort {
    pub name: String,
    pub description: String,
}

/// Events emitted by the background serial reader thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SerialEvent {
    Rx(Vec<u8>),
    /// Modem input lines changed (emitted once at start and on every change).
    Signals(ModemSignals),
    Disconnected(String),
}

/// Owned write half of an open [`SerialConnection`].
pub struct SerialWriteHandle {
    port: Box<dyn serialport::SerialPort>,
    locale: Locale,
}

impl SerialWriteHandle {
    /// Writes `data` to the serial port and flushes the transmit buffer.
    pub fn write_bytes(&mut self, data: &[u8]) -> Result<(), String> {
        write_port_bytes(&mut *self.port, self.locale, data)
    }
}

impl crate::session_service::PortWriter for SerialWriteHandle {
    fn write_all_and_flush(&mut self, data: &[u8]) -> Result<(), String> {
        self.write_bytes(data)
    }

    fn set_dtr(&mut self, level: bool) -> Result<(), String> {
        self.port
            .write_data_terminal_ready(level)
            .map_err(|err| err.description)
    }

    fn set_rts(&mut self, level: bool) -> Result<(), String> {
        self.port
            .write_request_to_send(level)
            .map_err(|err| err.description)
    }
}

/// Samples the modem input lines; unsupported or failing lines become `None`.
fn sample_signals(port: &mut dyn serialport::SerialPort) -> ModemSignals {
    ModemSignals {
        cts: port.read_clear_to_send().ok(),
        dsr: port.read_data_set_ready().ok(),
        cd: port.read_carrier_detect().ok(),
        ri: port.read_ring_indicator().ok(),
    }
}

/// Active real hardware serial connection with a dedicated reader thread.
pub struct SerialConnection {
    port: Option<Box<dyn serialport::SerialPort>>,
    stop_flag: Arc<AtomicBool>,
    reader_handle: Option<JoinHandle<()>>,
    locale: Locale,
}

impl SerialConnection {
    /// Opens a real serial port using `config` and starts a background reader thread.
    pub fn open(config: &SerialConfig) -> Result<(Self, Receiver<SerialEvent>), String> {
        Self::open_for(config, Locale::ZhCn)
    }

    /// Opens a real serial port using `config` and `locale` for localized messages.
    pub fn open_for(
        config: &SerialConfig,
        locale: Locale,
    ) -> Result<(Self, Receiver<SerialEvent>), String> {
        let port_name = config.port.trim();
        if port_name.is_empty() {
            return Err(tr(locale, "err.port_empty").into_owned());
        }

        let data_bits = map_data_bits(config.data_bits);
        let parity = map_parity(config.parity);
        let stop_bits = map_stop_bits(config.stop_bits, locale)?;

        let mut port = serialport::new(port_name, config.baud_rate)
            .data_bits(data_bits)
            .parity(parity)
            .stop_bits(stop_bits)
            .flow_control(map_flow_control(config.flow_control))
            .timeout(READ_TIMEOUT)
            .open()
            .map_err(|err| {
                format_serial_error(locale, &serial_action_open_failed(locale), port_name, &err)
            })?;

        // Initial modem output levels are best-effort: some drivers reject them.
        let _ = port.write_data_terminal_ready(config.dtr);
        if config.flow_control != FlowControl::Hardware {
            let _ = port.write_request_to_send(config.rts);
        }

        let mut reader_port = port.try_clone().map_err(|err| {
            format_serial_error(locale, &serial_action_clone_failed(locale), port_name, &err)
        })?;

        let (tx, rx) = mpsc::channel();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);

        let reader_handle = thread::Builder::new()
            .name(format!("sasha-serial-rx-{port_name}"))
            .spawn(move || {
                let mut buf = [0u8; READ_BUFFER_SIZE];
                let mut last_signals: Option<ModemSignals> = None;
                let mut last_poll: Option<Instant> = None;
                while !stop_clone.load(Ordering::Acquire) {
                    if last_poll.is_none_or(|at| at.elapsed() >= SIGNAL_POLL_INTERVAL) {
                        last_poll = Some(Instant::now());
                        let signals = sample_signals(&mut *reader_port);
                        if last_signals != Some(signals) {
                            last_signals = Some(signals);
                            if tx.send(SerialEvent::Signals(signals)).is_err() {
                                break;
                            }
                        }
                    }
                    match reader_port.read(&mut buf) {
                        Ok(0) => continue,
                        Ok(n) => {
                            if tx.send(SerialEvent::Rx(buf[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                        Err(ref err)
                            if err.kind() == std::io::ErrorKind::TimedOut
                                || err.kind() == std::io::ErrorKind::Interrupted =>
                        {
                            continue;
                        }
                        Err(err) => {
                            if !stop_clone.load(Ordering::Acquire) {
                                let _ = tx.send(SerialEvent::Disconnected(format_io_error(
                                    locale,
                                    &tr(locale, "serial.disconnected"),
                                    &err,
                                )));
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(|err| format_io_error(locale, &serial_action_spawn_failed(locale), &err))?;

        Ok((
            Self {
                port: Some(port),
                stop_flag,
                reader_handle: Some(reader_handle),
                locale,
            },
            rx,
        ))
    }

    /// Takes ownership of the underlying write port handle for use by `SessionService`.
    pub fn take_writer(&mut self) -> Option<SerialWriteHandle> {
        self.port.take().map(|port| SerialWriteHandle {
            port,
            locale: self.locale,
        })
    }

    /// Writes `data` to the serial port and flushes the transmit buffer.
    pub fn write_bytes(&mut self, data: &[u8]) -> Result<(), String> {
        let Some(port) = self.port.as_mut() else {
            return Err(tr(self.locale, "err.send_disconnected").into_owned());
        };
        write_port_bytes(&mut **port, self.locale, data)
    }

    /// Signals the background reader thread to exit and waits for it to finish.
    pub fn close(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        self.port.take();
        if let Some(handle) = self.reader_handle.take() {
            let _ = handle.join();
        }
    }
}

fn write_port_bytes(
    port: &mut dyn serialport::SerialPort,
    locale: Locale,
    data: &[u8],
) -> Result<(), String> {
    port.write_all(data)
        .map_err(|err| format_io_error(locale, &serial_action_write_failed(locale), &err))?;
    port.flush()
        .map_err(|err| format_io_error(locale, &serial_action_flush_failed(locale), &err))?;
    Ok(())
}

impl Drop for SerialConnection {
    fn drop(&mut self) {
        self.close();
    }
}

/// Enumerates available host serial ports, sorted stably by port name.
/// Returns an empty `Vec` if enumeration fails.
pub fn enumerate_serial_ports() -> Vec<DetectedPort> {
    enumerate_serial_ports_for(Locale::ZhCn)
}

pub fn enumerate_serial_ports_for(locale: Locale) -> Vec<DetectedPort> {
    let Ok(ports) = serialport::available_ports() else {
        return Vec::new();
    };

    let mut detected: Vec<DetectedPort> = ports
        .into_iter()
        .map(|info| DetectedPort {
            name: info.port_name,
            description: describe_port_type_for(&info.port_type, locale),
        })
        .collect();

    detected.sort_by(|a, b| a.name.cmp(&b.name));
    detected
}

/// Formats a `serialport::SerialPortType` into a human-readable description.
pub fn describe_port_type(port_type: &serialport::SerialPortType) -> String {
    describe_port_type_for(port_type, Locale::ZhCn)
}

pub fn describe_port_type_for(port_type: &serialport::SerialPortType, locale: Locale) -> String {
    match port_type {
        serialport::SerialPortType::UsbPort(usb) => format_usb_description(
            locale,
            usb.product.as_deref(),
            usb.manufacturer.as_deref(),
            usb.vid,
            usb.pid,
        ),
        serialport::SerialPortType::PciPort => tr(locale, "serial.port_pci").into_owned(),
        serialport::SerialPortType::BluetoothPort => tr(locale, "serial.port_bt").into_owned(),
        serialport::SerialPortType::Unknown => tr(locale, "serial.port_unknown").into_owned(),
    }
}

fn format_usb_description(
    locale: Locale,
    product: Option<&str>,
    manufacturer: Option<&str>,
    vid: u16,
    pid: u16,
) -> String {
    let product = product.map(str::trim).filter(|s| !s.is_empty());
    let manufacturer = manufacturer.map(str::trim).filter(|s| !s.is_empty());

    match (product, manufacturer) {
        (Some(p), Some(m)) => {
            if p.eq_ignore_ascii_case(m) || p.contains(m) {
                p.to_string()
            } else {
                format!("{p} ({m})")
            }
        }
        (Some(p), None) => p.to_string(),
        (None, Some(m)) => i18n::serial_usb_mfg(locale, m),
        (None, None) => i18n::serial_usb_vid_pid(locale, vid, pid),
    }
}

fn map_data_bits(bits: DataBits) -> serialport::DataBits {
    match bits {
        DataBits::Five => serialport::DataBits::Five,
        DataBits::Six => serialport::DataBits::Six,
        DataBits::Seven => serialport::DataBits::Seven,
        DataBits::Eight => serialport::DataBits::Eight,
    }
}

fn map_parity(parity: Parity) -> serialport::Parity {
    match parity {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    }
}

fn map_flow_control(flow: FlowControl) -> serialport::FlowControl {
    match flow {
        FlowControl::None => serialport::FlowControl::None,
        FlowControl::Hardware => serialport::FlowControl::Hardware,
        FlowControl::Software => serialport::FlowControl::Software,
    }
}

fn map_stop_bits(stop_bits: StopBits, locale: Locale) -> Result<serialport::StopBits, String> {
    match stop_bits {
        StopBits::One => Ok(serialport::StopBits::One),
        StopBits::Two => Ok(serialport::StopBits::Two),
        StopBits::OnePointFive => Err(tr(locale, "serial.unsupported_stop_bits").into_owned()),
    }
}

fn format_serial_error(
    locale: Locale,
    action: &str,
    port_name: &str,
    err: &serialport::Error,
) -> String {
    let detail = match err.kind() {
        serialport::ErrorKind::NoDevice => tr(locale, "serial.err_no_device").into_owned(),
        serialport::ErrorKind::InvalidInput => {
            i18n::serial_err_invalid_input(locale, &err.description)
        }
        serialport::ErrorKind::Io(kind) => describe_io_error_kind(locale, kind, &err.description),
        serialport::ErrorKind::Unknown => err.description.clone(),
    };
    i18n::serial_format_error(locale, action, port_name, &detail)
}

fn format_io_error(locale: Locale, action: &str, err: &std::io::Error) -> String {
    let detail = describe_io_error_kind(locale, err.kind(), &err.to_string());
    match locale {
        Locale::ZhCn => format!("{action}：{detail}"),
        Locale::EnUs => format!("{action}: {detail}"),
    }
}

fn describe_io_error_kind(locale: Locale, kind: std::io::ErrorKind, fallback: &str) -> String {
    match kind {
        std::io::ErrorKind::PermissionDenied => {
            tr(locale, "serial.err_permission_denied").into_owned()
        }
        std::io::ErrorKind::NotFound => tr(locale, "serial.err_no_device").into_owned(),
        std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted => {
            tr(locale, "serial.err_disconnected_handle").into_owned()
        }
        std::io::ErrorKind::TimedOut => tr(locale, "serial.err_timeout").into_owned(),
        _ => fallback.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_description_prefers_product_and_manufacturer() {
        let usb = serialport::SerialPortType::UsbPort(serialport::UsbPortInfo {
            vid: 0x1A86,
            pid: 0x7523,
            serial_number: None,
            manufacturer: Some("wch.cn".to_string()),
            product: Some("USB-SERIAL CH340".to_string()),
        });
        assert_eq!(describe_port_type(&usb), "USB-SERIAL CH340 (wch.cn)");

        let only_product = serialport::SerialPortType::UsbPort(serialport::UsbPortInfo {
            vid: 0x10C4,
            pid: 0xEA60,
            serial_number: None,
            manufacturer: Some("  ".to_string()),
            product: Some("CP2102 USB to UART".to_string()),
        });
        assert_eq!(describe_port_type(&only_product), "CP2102 USB to UART");

        let only_mfg = serialport::SerialPortType::UsbPort(serialport::UsbPortInfo {
            vid: 0x0403,
            pid: 0x6001,
            serial_number: None,
            manufacturer: Some("FTDI".to_string()),
            product: None,
        });
        assert_eq!(describe_port_type(&only_mfg), "FTDI USB 串口");

        let fallback_usb = serialport::SerialPortType::UsbPort(serialport::UsbPortInfo {
            vid: 0x1234,
            pid: 0xABCD,
            serial_number: None,
            manufacturer: None,
            product: None,
        });
        assert_eq!(describe_port_type(&fallback_usb), "USB 串口 (1234:ABCD)");
    }

    #[test]
    fn non_usb_port_types_return_readable_descriptions() {
        assert_eq!(
            describe_port_type(&serialport::SerialPortType::PciPort),
            "PCI 串口设备"
        );
        assert_eq!(
            describe_port_type(&serialport::SerialPortType::BluetoothPort),
            "蓝牙串口设备"
        );
        assert_eq!(
            describe_port_type(&serialport::SerialPortType::Unknown),
            "串口设备"
        );
    }

    #[test]
    fn open_rejects_empty_port_and_unsupported_stop_bits() {
        let mut empty_cfg = SerialConfig::default();
        empty_cfg.port = "   ".to_string();
        assert!(SerialConnection::open(&empty_cfg).is_err());

        let mut stop_cfg = SerialConfig::default();
        stop_cfg.stop_bits = StopBits::OnePointFive;
        let err = SerialConnection::open(&stop_cfg)
            .err()
            .expect("1.5 stop bits should be rejected");
        assert!(err.contains("1.5"));
    }

    #[test]
    fn flow_control_maps_to_serialport() {
        assert_eq!(
            map_flow_control(FlowControl::None),
            serialport::FlowControl::None
        );
        assert_eq!(
            map_flow_control(FlowControl::Hardware),
            serialport::FlowControl::Hardware
        );
        assert_eq!(
            map_flow_control(FlowControl::Software),
            serialport::FlowControl::Software
        );
    }

    #[test]
    fn enumerate_serial_ports_is_sorted_and_safe() {
        let ports = enumerate_serial_ports();
        for pair in ports.windows(2) {
            assert!(pair[0].name <= pair[1].name);
        }
    }
}
