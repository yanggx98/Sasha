#![allow(dead_code)]

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::model::{DataBits, Parity, SerialConfig, StopBits};

const READ_TIMEOUT: Duration = Duration::from_millis(50);
const READ_BUFFER_SIZE: usize = 4096;

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
    Disconnected(String),
}

/// Active real hardware serial connection with a dedicated reader thread.
pub struct SerialConnection {
    port: Box<dyn serialport::SerialPort>,
    stop_flag: Arc<AtomicBool>,
    reader_handle: Option<JoinHandle<()>>,
}

impl SerialConnection {
    /// Opens a real serial port using `config` and starts a background reader thread.
    pub fn open(config: &SerialConfig) -> Result<(Self, Receiver<SerialEvent>), String> {
        let port_name = config.port.trim();
        if port_name.is_empty() {
            return Err("端口名称不能为空".to_string());
        }

        let data_bits = map_data_bits(config.data_bits);
        let parity = map_parity(config.parity);
        let stop_bits = map_stop_bits(config.stop_bits)?;

        let port = serialport::new(port_name, config.baud_rate)
            .data_bits(data_bits)
            .parity(parity)
            .stop_bits(stop_bits)
            .timeout(READ_TIMEOUT)
            .open()
            .map_err(|err| format_serial_error("打开串口失败", port_name, &err))?;

        let mut reader_port = port
            .try_clone()
            .map_err(|err| format_serial_error("初始化串口读取通道失败", port_name, &err))?;

        let (tx, rx) = mpsc::channel();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);

        let reader_handle = thread::Builder::new()
            .name(format!("sasha-serial-rx-{port_name}"))
            .spawn(move || {
                let mut buf = [0u8; READ_BUFFER_SIZE];
                while !stop_clone.load(Ordering::Acquire) {
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
                                    "串口连接已断开",
                                    &err,
                                )));
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(|err| format_io_error("启动串口读线程失败", &err))?;

        Ok((
            Self {
                port,
                stop_flag,
                reader_handle: Some(reader_handle),
            },
            rx,
        ))
    }

    /// Writes `data` to the serial port and flushes the transmit buffer.
    pub fn write_bytes(&mut self, data: &[u8]) -> Result<(), String> {
        self.port
            .write_all(data)
            .map_err(|err| format_io_error("串口写入失败", &err))?;
        self.port
            .flush()
            .map_err(|err| format_io_error("串口刷新发送缓冲区失败", &err))?;
        Ok(())
    }

    /// Signals the background reader thread to exit and waits for it to finish.
    pub fn close(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(handle) = self.reader_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SerialConnection {
    fn drop(&mut self) {
        self.close();
    }
}

/// Enumerates available host serial ports, sorted stably by port name.
/// Returns an empty `Vec` if enumeration fails.
pub fn enumerate_serial_ports() -> Vec<DetectedPort> {
    let Ok(ports) = serialport::available_ports() else {
        return Vec::new();
    };

    let mut detected: Vec<DetectedPort> = ports
        .into_iter()
        .map(|info| DetectedPort {
            name: info.port_name,
            description: describe_port_type(&info.port_type),
        })
        .collect();

    detected.sort_by(|a, b| a.name.cmp(&b.name));
    detected
}

/// Formats a `serialport::SerialPortType` into a human-readable description.
pub fn describe_port_type(port_type: &serialport::SerialPortType) -> String {
    match port_type {
        serialport::SerialPortType::UsbPort(usb) => format_usb_description(
            usb.product.as_deref(),
            usb.manufacturer.as_deref(),
            usb.vid,
            usb.pid,
        ),
        serialport::SerialPortType::PciPort => "PCI 串口设备".to_string(),
        serialport::SerialPortType::BluetoothPort => "蓝牙串口设备".to_string(),
        serialport::SerialPortType::Unknown => "串口设备".to_string(),
    }
}

fn format_usb_description(
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
        (None, Some(m)) => format!("{m} USB 串口"),
        (None, None) => format!("USB 串口 ({vid:04X}:{pid:04X})"),
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

fn map_stop_bits(stop_bits: StopBits) -> Result<serialport::StopBits, String> {
    match stop_bits {
        StopBits::One => Ok(serialport::StopBits::One),
        StopBits::Two => Ok(serialport::StopBits::Two),
        StopBits::OnePointFive => Err("当前串口驱动不支持 1.5 停止位，请选择 1 或 2".to_string()),
    }
}

fn format_serial_error(action: &str, port_name: &str, err: &serialport::Error) -> String {
    let detail = match err.kind() {
        serialport::ErrorKind::NoDevice => "未找到指定串口设备".to_string(),
        serialport::ErrorKind::InvalidInput => format!("串口参数无效（{}）", err.description),
        serialport::ErrorKind::Io(kind) => describe_io_error_kind(kind, &err.description),
        serialport::ErrorKind::Unknown => err.description.clone(),
    };
    format!("{action}（{port_name}）：{detail}")
}

fn format_io_error(action: &str, err: &std::io::Error) -> String {
    let detail = describe_io_error_kind(err.kind(), &err.to_string());
    format!("{action}：{detail}")
}

fn describe_io_error_kind(kind: std::io::ErrorKind, fallback: &str) -> String {
    match kind {
        std::io::ErrorKind::PermissionDenied => "串口已被占用或无访问权限".to_string(),
        std::io::ErrorKind::NotFound => "未找到指定串口设备".to_string(),
        std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted => "设备已断开或句柄失效".to_string(),
        std::io::ErrorKind::TimedOut => "操作超时".to_string(),
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
    fn enumerate_serial_ports_is_sorted_and_safe() {
        let ports = enumerate_serial_ports();
        for pair in ports.windows(2) {
            assert!(pair[0].name <= pair[1].name);
        }
    }
}
