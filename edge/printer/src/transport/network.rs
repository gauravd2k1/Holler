//! `ESCPOS_NETWORK` transport — a raw TCP socket to `host:port`
//! (`printer.address`, e.g. `"192.168.1.50:9100"`, ESC/POS's conventional
//! raw-print port). Pure `std::net`, no dependency: a restaurant LAN printer
//! only needs a connect-and-write, and adding a crate for that would be
//! exactly the dependency ADR-013 asks us to avoid.

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use crate::error::{PrinterError, PrinterResult};
use crate::transport::PrinterTransport;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

pub struct NetworkTransport {
    address: String,
}

impl NetworkTransport {
    pub fn new(address: String) -> Self {
        Self { address }
    }
}

impl PrinterTransport for NetworkTransport {
    fn send(&mut self, bytes: &[u8]) -> PrinterResult<()> {
        let addr = self
            .address
            .parse::<std::net::SocketAddr>()
            .or_else(|_| resolve(&self.address))
            .map_err(|e| PrinterError::Transport {
                printer_id: String::new(),
                address: self.address.clone(),
                message: format!("invalid address: {e}"),
            })?;

        let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| {
            PrinterError::Transport {
                printer_id: String::new(),
                address: self.address.clone(),
                message: e.to_string(),
            }
        })?;
        stream
            .set_write_timeout(Some(WRITE_TIMEOUT))
            .map_err(|e| PrinterError::Transport {
                printer_id: String::new(),
                address: self.address.clone(),
                message: e.to_string(),
            })?;
        stream
            .write_all(bytes)
            .map_err(|e| PrinterError::Transport {
                printer_id: String::new(),
                address: self.address.clone(),
                message: e.to_string(),
            })?;
        stream.flush().map_err(|e| PrinterError::Transport {
            printer_id: String::new(),
            address: self.address.clone(),
            message: e.to_string(),
        })
    }
}

fn resolve(address: &str) -> std::io::Result<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    address
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no address resolved"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn sends_bytes_over_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");

        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut buf = Vec::new();
            socket.read_to_end(&mut buf).expect("read");
            buf
        });

        let mut transport = NetworkTransport::new(addr.to_string());
        transport.send(b"hello printer").expect("send");
        drop(transport);

        let received = handle.join().expect("join");
        assert_eq!(received, b"hello printer");
    }

    #[test]
    fn connect_failure_is_a_typed_transport_error() {
        // Port 1 is reserved and nothing listens there in CI/dev.
        let mut transport = NetworkTransport::new("127.0.0.1:1".to_string());
        let err = transport.send(b"x").unwrap_err();
        assert!(matches!(err, PrinterError::Transport { .. }));
    }
}
