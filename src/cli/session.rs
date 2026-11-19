//! A real serial connection wired into a `SessionService`, without any UI.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::thread::{self, JoinHandle};

use crate::i18n::Locale;
use crate::model::SerialConfig;
use crate::serial::{SerialConnection, SerialEvent};
use crate::session_service::SessionService;

use super::exit::CliError;

/// Forwards serial reader events into the service until the reader side closes.
pub(crate) fn spawn_pump(
    service: Arc<SessionService>,
    generation: u64,
    events: Receiver<SerialEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(event) = events.recv() {
            match event {
                SerialEvent::Rx(bytes) => service.publish_rx(generation, &bytes),
                SerialEvent::Disconnected(reason) => service.disconnect(Some(reason)),
                SerialEvent::Signals(_) => {}
            }
        }
    })
}

/// An open port plus the service that owns its writer and fans out received bytes.
pub struct LiveSession {
    pub service: Arc<SessionService>,
    pub generation: u64,
    connection: SerialConnection,
    pump: Option<JoinHandle<()>>,
}

impl LiveSession {
    /// Opens the port. Any failure (including a port that is busy) is `CliExit::OpenFailed`.
    pub fn open(config: &SerialConfig) -> Result<LiveSession, CliError> {
        let (mut connection, events) = SerialConnection::open_for(config, Locale::EnUs)
            .map_err(|msg| CliError::open_failed(format!("cannot open {}: {msg}", config.port)))?;
        let writer = connection.take_writer().ok_or_else(|| {
            CliError::open_failed(format!("cannot open {}: no write handle", config.port))
        })?;
        let service = Arc::new(SessionService::new());
        let generation = service.attach_writer(writer);
        let pump = spawn_pump(Arc::clone(&service), generation, events);
        Ok(LiveSession {
            service,
            generation,
            connection,
            pump: Some(pump),
        })
    }

    /// Releases the port: stops the writer, closes the connection and joins the pump.
    pub fn close(mut self) {
        self.service.disconnect(None);
        self.connection.close();
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_service::{PortWriter, RxRecvError};
    use std::sync::mpsc;
    use std::time::Duration;

    struct NullWriter;

    impl PortWriter for NullWriter {
        fn write_all_and_flush(&mut self, _data: &[u8]) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn pump_forwards_rx_bytes_to_subscribers() {
        let service = Arc::new(SessionService::new());
        let generation = service.attach_writer(NullWriter);
        let sub = service.subscribe_rx(generation).unwrap();
        let (tx, rx) = mpsc::channel();
        let pump = spawn_pump(Arc::clone(&service), generation, rx);

        tx.send(SerialEvent::Rx(b"hello".to_vec())).unwrap();
        assert_eq!(
            sub.recv_timeout(Duration::from_secs(2)).unwrap(),
            b"hello".to_vec()
        );
        drop(tx);
        pump.join().unwrap();
    }

    #[test]
    fn pump_turns_a_serial_disconnect_into_a_service_disconnect() {
        let service = Arc::new(SessionService::new());
        let generation = service.attach_writer(NullWriter);
        let sub = service.subscribe_rx(generation).unwrap();
        let (tx, rx) = mpsc::channel();
        let pump = spawn_pump(Arc::clone(&service), generation, rx);

        tx.send(SerialEvent::Disconnected("unplugged".into()))
            .unwrap();
        let err = sub.recv_timeout(Duration::from_secs(2)).unwrap_err();
        assert_eq!(err, RxRecvError::Disconnected(Some("unplugged".into())));
        drop(tx);
        pump.join().unwrap();
    }
}
