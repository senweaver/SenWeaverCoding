// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone)]
pub struct BoardInfo {
    pub vid: u16,
    pub pid: u16,
    pub name: &'static str,
    pub architecture: Option<&'static str>,
}

const KNOWN_BOARDS: &[BoardInfo] = &[
    BoardInfo {
        vid: 0x0483,
        pid: 0x374b,
        name: "nucleo-f401re",
        architecture: Some("ARM Cortex-M4"),
    },
    BoardInfo {
        vid: 0x0483,
        pid: 0x3748,
        name: "nucleo-f411re",
        architecture: Some("ARM Cortex-M4"),
    },
    BoardInfo {
        vid: 0x2341,
        pid: 0x0043,
        name: "arduino-uno",
        architecture: Some("AVR ATmega328P"),
    },
    BoardInfo {
        vid: 0x2341,
        pid: 0x0078,
        name: "arduino-uno",
        architecture: Some("Arduino Uno Q / ATmega328P"),
    },
    BoardInfo {
        vid: 0x2341,
        pid: 0x0042,
        name: "arduino-mega",
        architecture: Some("AVR ATmega2560"),
    },
    BoardInfo {
        vid: 0x10c4,
        pid: 0xea60,
        name: "cp2102",
        architecture: Some("USB-UART bridge"),
    },
    BoardInfo {
        vid: 0x10c4,
        pid: 0xea70,
        name: "cp2102n",
        architecture: Some("USB-UART bridge"),
    },

    BoardInfo {
        vid: 0x1a86,
        pid: 0x7523,
        name: "esp32",
        architecture: Some("ESP32 (CH340)"),
    },
    BoardInfo {
        vid: 0x1a86,
        pid: 0x55d4,
        name: "esp32",
        architecture: Some("ESP32 (CH340)"),
    },
];

pub fn lookup_board(vid: u16, pid: u16) -> Option<&'static BoardInfo> {
    KNOWN_BOARDS.iter().find(|b| b.vid == vid && b.pid == pid)
}

pub fn known_boards() -> &'static [BoardInfo] {
    KNOWN_BOARDS
}
