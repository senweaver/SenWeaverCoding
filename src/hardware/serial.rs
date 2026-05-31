// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::{
    protocol::{ZcCommand, ZcResponse},
    transport::{Transport, TransportError, TransportKind},
};
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_serial::SerialPortBuilderExt;

const SEND_TIMEOUT_SECS: u64 = 5;

pub const DEFAULT_BAUD: u32 = 115_200;

const PING_TIMEOUT_MS: u64 = 300;

use crate::util::is_serial_path_allowed as is_path_allowed;

pub struct HardwareSerialTransport {
    port_path: String,
    baud_rate: u32,
}

impl HardwareSerialTransport {

    pub fn new(port_path: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            port_path: port_path.into(),
            baud_rate,
        }
    }

    pub fn with_default_baud(port_path: impl Into<String>) -> Self {
        Self::new(port_path, DEFAULT_BAUD)
    }

    pub fn port_path(&self) -> &str {
        &self.port_path
    }

    pub async fn ping_handshake(&self) -> bool {
        let ping = ZcCommand::simple("ping");
        let json = match serde_json::to_string(&ping) {
            Ok(j) => j,
            Err(_) => return false,
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(PING_TIMEOUT_MS),
            do_send(&self.port_path, self.baud_rate, &json),
        )
        .await;

        match result {
            Ok(Ok(resp)) => {

                resp.ok
                    && resp
                        .data
                        .get("firmware")
                        .and_then(|v| v.as_str())
                        .map(|s| s == "sen")
                        .unwrap_or(false)
            }
            _ => false,
        }
    }
}

#[async_trait]
impl Transport for HardwareSerialTransport {
    async fn send(&self, cmd: &ZcCommand) -> Result<ZcResponse, TransportError> {
        if !is_path_allowed(&self.port_path) {
            return Err(TransportError::Other(format!(
                "serial path not allowed: {}",
                self.port_path
            )));
        }

        let json = serde_json::to_string(cmd)
            .map_err(|e| TransportError::Protocol(format!("failed to serialize command: {e}")))?;

        tracing::info!(port = %self.port_path, cmd = %cmd.cmd, "serial send");

        tokio::time::timeout(
            std::time::Duration::from_secs(SEND_TIMEOUT_SECS),
            do_send(&self.port_path, self.baud_rate, &json),
        )
        .await
        .map_err(|_| TransportError::Timeout(SEND_TIMEOUT_SECS))?
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Serial
    }

    fn is_connected(&self) -> bool {

        std::path::Path::new(&self.port_path).exists()
    }
}

async fn do_send(path: &str, baud: u32, json: &str) -> Result<ZcResponse, TransportError> {

    let mut port = tokio_serial::new(path, baud)
        .open_native_async()
        .map_err(|e| {

            match e.kind {
                tokio_serial::ErrorKind::NoDevice => TransportError::Disconnected,
                tokio_serial::ErrorKind::Io(io_kind) if io_kind == std::io::ErrorKind::NotFound => {
                    TransportError::Disconnected
                }
                _ => TransportError::Other(format!("failed to open {path}: {e}")),
            }
        })?;

    port.write_all(format!("{json}\n").as_bytes())
        .await
        .map_err(TransportError::Io)?;
    port.flush().await.map_err(TransportError::Io)?;

    let mut reader = BufReader::new(port);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .await
        .map_err(|e: std::io::Error| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                TransportError::Disconnected
            } else {
                TransportError::Io(e)
            }
        })?;

    let trimmed = response_line.trim();
    if trimmed.is_empty() {
        return Err(TransportError::Protocol(
            "empty response from device".to_string(),
        ));
    }

    serde_json::from_str(trimmed).map_err(|e| {
        TransportError::Protocol(format!("invalid JSON response: {e}  -  got: {trimmed:?}"))
    })
}
