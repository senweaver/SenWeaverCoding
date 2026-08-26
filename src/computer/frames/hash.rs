// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub fn dhash64(image: &image::RgbaImage) -> u64 {
    let small = image::imageops::resize(image, 9, 8, image::imageops::FilterType::Triangle);
    let mut bits: u64 = 0;
    let mut bit = 0u32;
    for y in 0..8u32 {
        for x in 0..8u32 {
            let left = luma(small.get_pixel(x, y));
            let right = luma(small.get_pixel(x + 1, y));
            if left > right {
                bits |= 1u64 << bit;
            }
            bit += 1;
        }
    }
    bits
}

fn luma(pixel: &image::Rgba<u8>) -> u32 {
    let [r, g, b, _] = pixel.0;
    (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

pub fn phash_hex(hash: u64) -> String {
    format!("{hash:016x}")
}
