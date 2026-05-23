// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::transport::Transport;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRuntime {

    MicroPython,

    CircuitPython,

    Arduino,

    Nucleus,

    Linux,

    Aardvark,
}

impl DeviceRuntime {

    pub fn from_kind(kind: &DeviceKind) -> Self {
        match kind {
            DeviceKind::Pico | DeviceKind::Esp32 | DeviceKind::Generic => Self::MicroPython,
            DeviceKind::Arduino => Self::Arduino,
            DeviceKind::Nucleo => Self::Nucleus,
            DeviceKind::Aardvark => Self::Aardvark,
        }
    }
}

impl std::fmt::Display for DeviceRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MicroPython => write!(f, "MicroPython"),
            Self::CircuitPython => write!(f, "CircuitPython"),
            Self::Arduino => write!(f, "Arduino"),
            Self::Nucleus => write!(f, "Nucleus"),
            Self::Linux => write!(f, "Linux"),
            Self::Aardvark => write!(f, "Aardvark"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceKind {

    Pico,

    Arduino,

    Esp32,

    Nucleo,

    Generic,

    Aardvark,
}

impl DeviceKind {

    pub fn from_vid(vid: u16) -> Option<Self> {
        match vid {
            0x2e8a => Some(Self::Pico),
            0x2341 => Some(Self::Arduino),
            0x10c4 => Some(Self::Esp32),
            0x0483 => Some(Self::Nucleo),
            0x2b76 => Some(Self::Aardvark),
            _ => None,
        }
    }
}

impl std::fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pico => write!(f, "pico"),
            Self::Arduino => write!(f, "arduino"),
            Self::Esp32 => write!(f, "esp32"),
            Self::Nucleo => write!(f, "nucleo"),
            Self::Generic => write!(f, "generic"),
            Self::Aardvark => write!(f, "aardvark"),
        }
    }
}

#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct DeviceCapabilities {
    pub gpio: bool,
    pub i2c: bool,
    pub spi: bool,
    pub swd: bool,
    pub uart: bool,
    pub adc: bool,
    pub pwm: bool,
}

#[derive(Debug, Clone)]
pub struct Device {

    pub alias: String,

    pub board_name: String,

    pub kind: DeviceKind,

    pub runtime: DeviceRuntime,

    pub vid: Option<u16>,

    pub pid: Option<u16>,

    pub device_path: Option<String>,

    pub architecture: Option<String>,

    pub firmware: Option<String>,
}

impl Device {

    pub fn port(&self) -> Option<&str> {
        self.device_path.as_deref()
    }
}

pub struct DeviceContext {

    pub device: Arc<Device>,

    pub transport: Arc<dyn Transport>,

    pub capabilities: DeviceCapabilities,
}

struct RegisteredDevice {
    device: Arc<Device>,
    transport: Option<Arc<dyn Transport>>,
    capabilities: DeviceCapabilities,
}

pub const NO_HW_DEVICES_SUMMARY: &str = "No hardware devices connected.";

pub struct DeviceRegistry {
    devices: HashMap<String, RegisteredDevice>,
    alias_counters: HashMap<String, u32>,
}

impl DeviceRegistry {

    pub fn new() -> Self {
        Self {
            devices: HashMap::new(),
            alias_counters: HashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        board_name: &str,
        vid: Option<u16>,
        pid: Option<u16>,
        device_path: Option<String>,
        architecture: Option<String>,
    ) -> String {
        let prefix = alias_prefix(board_name);
        let counter = self.alias_counters.entry(prefix.clone()).or_insert(0);
        let alias = format!("{}{}", prefix, counter);
        *counter += 1;

        let kind = vid
            .and_then(DeviceKind::from_vid)
            .unwrap_or(DeviceKind::Generic);
        let runtime = DeviceRuntime::from_kind(&kind);

        let device = Arc::new(Device {
            alias: alias.clone(),
            board_name: board_name.to_string(),
            kind,
            runtime,
            vid,
            pid,
            device_path,
            architecture,
            firmware: None,
        });

        self.devices.insert(
            alias.clone(),
            RegisteredDevice {
                device,
                transport: None,
                capabilities: DeviceCapabilities::default(),
            },
        );

        alias
    }

    pub fn attach_transport(
        &mut self,
        alias: &str,
        transport: Arc<dyn Transport>,
        capabilities: DeviceCapabilities,
    ) -> anyhow::Result<()> {
        if let Some(entry) = self.devices.get_mut(alias) {
            entry.transport = Some(transport);
            entry.capabilities = capabilities;
            Ok(())
        } else {
            Err(anyhow::anyhow!("unknown device alias: {}", alias))
        }
    }

    pub fn get_device(&self, alias: &str) -> Option<Arc<Device>> {
        self.devices.get(alias).map(|e| e.device.clone())
    }

    pub fn context(&self, alias: &str) -> Option<DeviceContext> {
        self.devices.get(alias).and_then(|e| {
            e.transport.as_ref().map(|t| DeviceContext {
                device: e.device.clone(),
                transport: t.clone(),
                capabilities: e.capabilities.clone(),
            })
        })
    }

    pub fn aliases(&self) -> Vec<&str> {
        self.devices.keys().map(|s| s.as_str()).collect()
    }

    pub fn prompt_summary(&self) -> String {
        if self.devices.is_empty() {
            return NO_HW_DEVICES_SUMMARY.to_string();
        }

        let mut lines = vec!["Connected devices:".to_string()];
        let mut sorted_aliases: Vec<&String> = self.devices.keys().collect();
        sorted_aliases.sort();
        for alias in sorted_aliases {
            let entry = &self.devices[alias];
            let status = entry
                .transport
                .as_ref()
                .map(|t| {
                    if t.is_connected() {
                        "connected"
                    } else {
                        "disconnected"
                    }
                })
                .unwrap_or("no transport");
            let arch = entry
                .device
                .architecture
                .as_deref()
                .unwrap_or("unknown arch");
            lines.push(format!(
                "  {} — {} ({}) [{}]",
                alias, entry.device.board_name, arch, status
            ));
        }
        lines.join("\n")
    }

    pub fn resolve_gpio_device(
        &self,
        args: &serde_json::Value,
    ) -> Result<(String, DeviceContext), String> {
        let device_alias: String = match args.get("device").and_then(|v| v.as_str()) {
            Some(a) => a.to_string(),
            None => {
                let gpio_aliases: Vec<String> = self
                    .aliases()
                    .into_iter()
                    .filter(|a| {
                        self.context(a)
                            .map(|c| c.capabilities.gpio)
                            .unwrap_or(false)
                    })
                    .map(|a| a.to_string())
                    .collect();
                match gpio_aliases.as_slice() {
                    [single] => single.clone(),
                    [] => {
                        return Err("no GPIO-capable device found; specify \"device\" parameter"
                            .to_string());
                    }
                    _ => {
                        return Err(format!(
                            "multiple devices available ({}); specify \"device\" parameter",
                            gpio_aliases.join(", ")
                        ));
                    }
                }
            }
        };

        let ctx = self.context(&device_alias).ok_or_else(|| {
            format!(
                "device '{}' not found or has no transport attached",
                device_alias
            )
        })?;

        if !ctx.capabilities.gpio {
            return Err(format!(
                "device '{}' does not support GPIO; specify a GPIO-capable device",
                device_alias
            ));
        }

        Ok((device_alias, ctx))
    }

    pub fn has_aardvark(&self) -> bool {
        self.devices
            .values()
            .any(|e| e.device.kind == DeviceKind::Aardvark)
    }

    pub fn resolve_aardvark_device(
        &self,
        args: &serde_json::Value,
    ) -> Result<(String, DeviceContext), String> {
        let device_alias: String = match args.get("device").and_then(|v| v.as_str()) {
            Some(a) => a.to_string(),
            None => {
                let aardvark_aliases: Vec<String> = self
                    .aliases()
                    .into_iter()
                    .filter(|a| {
                        self.devices
                            .get(*a)
                            .map(|e| e.device.kind == DeviceKind::Aardvark)
                            .unwrap_or(false)
                    })
                    .map(|a| a.to_string())
                    .collect();
                match aardvark_aliases.as_slice() {
                    [single] => single.clone(),
                    [] => {
                        return Err("no Aardvark adapter found; is it plugged in?".to_string());
                    }
                    _ => {
                        return Err(format!(
                            "multiple Aardvark adapters available ({}); \
                             specify \"device\" parameter",
                            aardvark_aliases.join(", ")
                        ));
                    }
                }
            }
        };

        let ctx = self.context(&device_alias).ok_or_else(|| {
            format!("device '{device_alias}' not found or has no transport attached")
        })?;

        Ok((device_alias, ctx))
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    pub fn get(&self, alias: &str) -> Option<Arc<Device>> {
        self.get_device(alias)
    }

    pub fn all(&self) -> Vec<Arc<Device>> {
        self.devices.values().map(|e| e.device.clone()).collect()
    }

    pub fn summary(&self) -> String {
        if self.devices.is_empty() {
            return String::new();
        }
        let mut lines: Vec<String> = self
            .devices
            .values()
            .map(|e| {
                let path = e.device.port().unwrap_or("(native)");
                format!("{}: {} {}", e.device.alias, e.device.board_name, path)
            })
            .collect();
        lines.sort();
        lines.join("\n")
    }

    #[cfg(feature = "hardware")]
    pub async fn discover() -> Self {
        use super::{
            discover::scan_serial_devices,
            serial::{DEFAULT_BAUD, HardwareSerialTransport},
        };

        let mut registry = Self::new();

        for info in scan_serial_devices() {
            let is_known_vid = info.vid != 0;

            let probe_transport = if !is_known_vid {
                let probe = HardwareSerialTransport::new(&info.port_path, DEFAULT_BAUD);
                if !probe.ping_handshake().await {
                    tracing::debug!(
                        port = %info.port_path,
                        "skipping unknown device: no SenWeaverCoding firmware response"
                    );
                    continue;
                }
                Some(probe)
            } else {
                None
            };

            let board_name = info.board_name.as_deref().unwrap_or("unknown").to_string();

            let alias = registry.register(
                &board_name,
                if info.vid != 0 { Some(info.vid) } else { None },
                if info.pid != 0 { Some(info.pid) } else { None },
                Some(info.port_path.clone()),
                info.architecture,
            );

            let transport: Arc<dyn super::transport::Transport> =
                if let Some(probe) = probe_transport {
                    Arc::new(probe)
                } else {
                    Arc::new(HardwareSerialTransport::new(&info.port_path, DEFAULT_BAUD))
                };
            let caps = DeviceCapabilities {
                gpio: true,
                ..DeviceCapabilities::default()
            };
            registry.attach_transport(&alias, transport, caps)
                .unwrap_or_else(|e| tracing::warn!(alias = %alias, err = %e, "attach_transport: unexpected unknown alias"));

            tracing::info!(
                alias = %alias,
                port  = %info.port_path,
                vid   = %info.vid,
                "device registered"
            );
        }

        registry
    }
}

impl DeviceRegistry {

    #[cfg(feature = "hardware")]
    pub async fn reconnect(&mut self, alias: &str, new_port: Option<&str>) -> anyhow::Result<()> {
        use super::serial::{DEFAULT_BAUD, HardwareSerialTransport};

        let entry = self
            .devices
            .get_mut(alias)
            .ok_or_else(|| anyhow::anyhow!("unknown device alias: {alias}"))?;

        let port_path = match new_port {
            Some(p) => {

                let mut updated = (*entry.device).clone();
                updated.device_path = Some(p.to_string());
                entry.device = Arc::new(updated);
                p.to_string()
            }
            None => entry
                .device
                .device_path
                .clone()
                .ok_or_else(|| anyhow::anyhow!("device {alias} has no port path"))?,
        };

        entry.transport = None;

        let transport = HardwareSerialTransport::new(&port_path, DEFAULT_BAUD);
        if !transport.ping_handshake().await {
            anyhow::bail!(
                "ping handshake failed after reconnect on {port_path} — \
                 firmware may not be running"
            );
        }

        entry.transport = Some(Arc::new(transport) as Arc<dyn super::transport::Transport>);
        entry.capabilities.gpio = true;

        tracing::info!(alias = %alias, port = %port_path, "device reconnected");
        Ok(())
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn alias_prefix(board_name: &str) -> String {
    match board_name {
        s if s.starts_with("raspberry-pi-pico") || s.starts_with("pico") => "pico".to_string(),
        s if s.starts_with("arduino") => "arduino".to_string(),
        s if s.starts_with("esp32") || s.starts_with("esp") => "esp".to_string(),
        s if s.starts_with("nucleo") || s.starts_with("stm32") => "nucleo".to_string(),
        s if s.starts_with("rpi") || s == "raspberry-pi" => "rpi".to_string(),
        _ => "device".to_string(),
    }
}
