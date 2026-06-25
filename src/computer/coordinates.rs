// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, Copy)]
pub struct Box2d {
    pub ymin: f64,
    pub xmin: f64,
    pub ymax: f64,
    pub xmax: f64,
}

impl Box2d {
    pub fn from_slice(values: &[f64]) -> Option<Self> {
        if values.len() != 4 {
            return None;
        }
        Some(Self {
            ymin: values[0],
            xmin: values[1],
            ymax: values[2],
            xmax: values[3],
        })
    }

    pub fn center_normalized(&self) -> (f64, f64) {
        let x = f64::midpoint(self.xmin, self.xmax);
        let y = f64::midpoint(self.ymin, self.ymax);
        (x.clamp(0.0, 1000.0), y.clamp(0.0, 1000.0))
    }
}

pub fn coords_to_center_norm(values: &[f64]) -> Option<(f64, f64)> {
    let (x, y) = match values.len() {
        2 => (values[0], values[1]),
        4 => (
            f64::midpoint(values[1], values[3]),
            f64::midpoint(values[0], values[2]),
        ),
        _ => return None,
    };
    let max = values.iter().copied().fold(0.0_f64, f64::max);
    let (x, y) = if max <= 1.0 {
        (x * 1000.0, y * 1000.0)
    } else {
        (x, y)
    };
    Some((x.clamp(0.0, 1000.0), y.clamp(0.0, 1000.0)))
}

pub fn normalized_to_input(x_norm: f64, y_norm: f64, display_w: i32, display_h: i32) -> (i32, i32) {
    let w = display_w.max(1);
    let h = display_h.max(1);
    let x = (x_norm / 1000.0) * f64::from(w);
    let y = (y_norm / 1000.0) * f64::from(h);
    (
        (x.round() as i32).clamp(0, w - 1),
        (y.round() as i32).clamp(0, h - 1),
    )
}
