// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;

use super::types::RecordedStep;

const TEMPLATE_CROP_PX: u32 = 240;
const COARSE_TPL_DIM: usize = 48;
const COARSE_HAY_DIM: usize = 420;
const MIN_COARSE_TPL_DIM: usize = 8;
const ROI_RADIUS_FACTOR: f64 = 0.6;
const ROI_ACCEPT_SCORE: f32 = 0.80;
const FULL_ACCEPT_SCORE: f32 = 0.845;
const FINAL_ACCEPT_SCORE: f32 = 0.78;
const MIN_TEMPLATE_STD: f32 = 0.012;
const SETTLE_SAMPLE_DIM: u32 = 128;

#[derive(Debug, Clone)]
struct GrayImage32 {
    w: usize,
    h: usize,
    data: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct ReferenceTemplate {
    gray: GrayImage32,
    click_dx: f64,
    click_dy: f64,
    src_w: u32,
    src_h: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct LocalMatch {
    pub x_norm: f64,
    pub y_norm: f64,
    pub score: f32,
}

fn to_gray(img: &image::RgbaImage) -> GrayImage32 {
    let w = img.width() as usize;
    let h = img.height() as usize;
    let mut data = Vec::with_capacity(w * h);
    for px in img.pixels() {
        let [red, green, blue, _] = px.0;
        data.push(
            (0.299 * f32::from(red) + 0.587 * f32::from(green) + 0.114 * f32::from(blue))
                / 255.0,
        );
    }
    GrayImage32 { w, h, data }
}

fn downscale(src: &GrayImage32, factor: usize) -> GrayImage32 {
    if factor <= 1 {
        return src.clone();
    }
    let w = (src.w / factor).max(1);
    let h = (src.h / factor).max(1);
    let mut data = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0f32;
            let mut n = 0f32;
            for dy in 0..factor {
                let sy = y * factor + dy;
                if sy >= src.h {
                    break;
                }
                let row = sy * src.w;
                for dx in 0..factor {
                    let sx = x * factor + dx;
                    if sx >= src.w {
                        break;
                    }
                    acc += src.data[row + sx];
                    n += 1.0;
                }
            }
            data[y * w + x] = if n > 0.0 { acc / n } else { 0.0 };
        }
    }
    GrayImage32 { w, h, data }
}

fn crop_gray(src: &GrayImage32, x0: usize, y0: usize, w: usize, h: usize) -> GrayImage32 {
    let w = w.min(src.w.saturating_sub(x0));
    let h = h.min(src.h.saturating_sub(y0));
    let mut data = Vec::with_capacity(w * h);
    for y in 0..h {
        let row = (y0 + y) * src.w + x0;
        data.extend_from_slice(&src.data[row..row + w]);
    }
    GrayImage32 { w, h, data }
}

fn resize_bilinear(src: &GrayImage32, nw: usize, nh: usize) -> GrayImage32 {
    let nw = nw.max(1);
    let nh = nh.max(1);
    let mut data = vec![0f32; nw * nh];
    let sx = src.w as f64 / nw as f64;
    let sy = src.h as f64 / nh as f64;
    for y in 0..nh {
        let fy = ((y as f64 + 0.5) * sy - 0.5).max(0.0);
        let y0 = (fy.floor() as usize).min(src.h - 1);
        let y1 = (y0 + 1).min(src.h - 1);
        let wy = (fy - y0 as f64) as f32;
        for x in 0..nw {
            let fx = ((x as f64 + 0.5) * sx - 0.5).max(0.0);
            let x0 = (fx.floor() as usize).min(src.w - 1);
            let x1 = (x0 + 1).min(src.w - 1);
            let wx = (fx - x0 as f64) as f32;
            let p00 = src.data[y0 * src.w + x0];
            let p01 = src.data[y0 * src.w + x1];
            let p10 = src.data[y1 * src.w + x0];
            let p11 = src.data[y1 * src.w + x1];
            let top = p00 + (p01 - p00) * wx;
            let bottom = p10 + (p11 - p10) * wx;
            data[y * nw + x] = top + (bottom - top) * wy;
        }
    }
    GrayImage32 {
        w: nw,
        h: nh,
        data,
    }
}

struct Integral {
    w: usize,
    sum: Vec<f64>,
    sq: Vec<f64>,
}

fn build_integral(img: &GrayImage32) -> Integral {
    let w = img.w + 1;
    let h = img.h + 1;
    let mut sum = vec![0f64; w * h];
    let mut sq = vec![0f64; w * h];
    for y in 0..img.h {
        let mut row_sum = 0f64;
        let mut row_sq = 0f64;
        for x in 0..img.w {
            let v = f64::from(img.data[y * img.w + x]);
            row_sum += v;
            row_sq += v * v;
            sum[(y + 1) * w + (x + 1)] = sum[y * w + (x + 1)] + row_sum;
            sq[(y + 1) * w + (x + 1)] = sq[y * w + (x + 1)] + row_sq;
        }
    }
    Integral { w, sum, sq }
}

impl Integral {
    fn window(&self, x: usize, y: usize, tw: usize, th: usize) -> (f64, f64) {
        let top_left = y * self.w + x;
        let top_right = y * self.w + (x + tw);
        let bottom_left = (y + th) * self.w + x;
        let bottom_right = (y + th) * self.w + (x + tw);
        (
            self.sum[bottom_right] - self.sum[top_right] - self.sum[bottom_left]
                + self.sum[top_left],
            self.sq[bottom_right] - self.sq[top_right] - self.sq[bottom_left]
                + self.sq[top_left],
        )
    }
}

fn template_stats(tpl: &GrayImage32) -> Option<(Vec<f32>, f64)> {
    let n = (tpl.w * tpl.h) as f64;
    if n <= 1.0 {
        return None;
    }
    let sum: f64 = tpl.data.iter().map(|&v| f64::from(v)).sum();
    let sq: f64 = tpl.data.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
    let mean = sum / n;
    let var = (sq / n - mean * mean).max(0.0);
    let std = var.sqrt();
    if std < f64::from(MIN_TEMPLATE_STD) {
        return None;
    }
    let zero_mean: Vec<f32> = tpl.data.iter().map(|&v| v - mean as f32).collect();
    let norm = (n * var).sqrt();
    Some((zero_mean, norm))
}

fn zncc_search(
    hay: &GrayImage32,
    integ: &Integral,
    tpl: &GrayImage32,
    tpl_zero_mean: &[f32],
    tpl_norm: f64,
    x_range: (usize, usize),
    y_range: (usize, usize),
) -> Option<(usize, usize, f32)> {
    let tw = tpl.w;
    let th = tpl.h;
    if hay.w < tw || hay.h < th {
        return None;
    }
    let max_x = (hay.w - tw).min(x_range.1);
    let max_y = (hay.h - th).min(y_range.1);
    let min_x = x_range.0.min(max_x);
    let min_y = y_range.0.min(max_y);
    let n = (tw * th) as f64;
    let mut best: Option<(usize, usize, f32)> = None;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let (hsum, hsq) = integ.window(x, y, tw, th);
            let hmean = hsum / n;
            let hvar = (hsq / n - hmean * hmean).max(0.0);
            if hvar <= 1e-9 {
                continue;
            }
            let mut cross = 0f64;
            for ty in 0..th {
                let hrow = &hay.data[(y + ty) * hay.w + x..(y + ty) * hay.w + x + tw];
                let trow = &tpl_zero_mean[ty * tw..ty * tw + tw];
                let mut acc = 0f32;
                for (hv, tv) in hrow.iter().zip(trow.iter()) {
                    acc += hv * tv;
                }
                cross += f64::from(acc);
            }
            let denom = (n * hvar).sqrt() * tpl_norm;
            if denom <= 1e-9 {
                continue;
            }
            let score = (cross / denom) as f32;
            if best.map_or(true, |b| score > b.2) {
                best = Some((x, y, score));
            }
        }
    }
    best
}

pub fn load_reference_template(
    recording_dir: &Path,
    step: &RecordedStep,
) -> Option<ReferenceTemplate> {
    let file = step.screenshot_file.as_deref()?;
    let x_norm = step.x_norm?;
    let y_norm = step.y_norm?;
    let img = image::open(recording_dir.join(file)).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let cx = (x_norm / 1000.0 * f64::from(w)).round() as i64;
    let cy = (y_norm / 1000.0 * f64::from(h)).round() as i64;
    let half = i64::from(TEMPLATE_CROP_PX / 2);
    let x0 = (cx - half).clamp(0, i64::from(w).saturating_sub(1));
    let y0 = (cy - half).clamp(0, i64::from(h).saturating_sub(1));
    let x1 = (cx + half).clamp(x0 + 1, i64::from(w));
    let y1 = (cy + half).clamp(y0 + 1, i64::from(h));
    let crop = image::imageops::crop_imm(
        &img,
        x0 as u32,
        y0 as u32,
        (x1 - x0) as u32,
        (y1 - y0) as u32,
    )
    .to_image();
    let gray = to_gray(&crop);
    template_stats(&gray)?;
    Some(ReferenceTemplate {
        gray,
        click_dx: (cx - x0) as f64,
        click_dy: (cy - y0) as f64,
        src_w: w,
        src_h: h,
    })
}

pub fn locate_in_frame(
    tpl: &ReferenceTemplate,
    frame: &image::RgbaImage,
    prior_norm: Option<(f64, f64)>,
) -> Option<LocalMatch> {
    let hay = to_gray(frame);
    if hay.w < 8 || hay.h < 8 {
        return None;
    }
    let sx = hay.w as f64 / f64::from(tpl.src_w.max(1));
    let sy = hay.h as f64 / f64::from(tpl.src_h.max(1));
    let scale = f64::midpoint(sx, sy).clamp(0.2, 5.0);
    let (tgray, click_dx, click_dy) = if (scale - 1.0).abs() > 0.02 {
        let nw = ((tpl.gray.w as f64 * scale).round() as usize).max(8);
        let nh = ((tpl.gray.h as f64 * scale).round() as usize).max(8);
        (
            resize_bilinear(&tpl.gray, nw, nh),
            tpl.click_dx * scale,
            tpl.click_dy * scale,
        )
    } else {
        (tpl.gray.clone(), tpl.click_dx, tpl.click_dy)
    };
    if tgray.w >= hay.w || tgray.h >= hay.h {
        return None;
    }

    let f_tpl = tgray.w.max(tgray.h) / COARSE_TPL_DIM;
    let f_hay = hay.w.max(hay.h) / COARSE_HAY_DIM;
    let max_f = (tgray.w.min(tgray.h) / MIN_COARSE_TPL_DIM).max(1);
    let coarse_f = f_tpl.max(f_hay).max(1).min(max_f);
    let hay_c = downscale(&hay, coarse_f);
    let tpl_c = downscale(&tgray, coarse_f);
    if tpl_c.w >= hay_c.w || tpl_c.h >= hay_c.h {
        return None;
    }
    let (tzm_c, tnorm_c) = template_stats(&tpl_c)?;
    let integ_c = build_integral(&hay_c);

    let mut candidate: Option<(usize, usize, f32)> = None;
    if let Some((pxn, pyn)) = prior_norm {
        let px = (pxn / 1000.0 * hay_c.w as f64 - click_dx / coarse_f as f64).round();
        let py = (pyn / 1000.0 * hay_c.h as f64 - click_dy / coarse_f as f64).round();
        let radius = (tpl_c.w.max(tpl_c.h) as f64 * ROI_RADIUS_FACTOR).round();
        let min_x = (px - radius).max(0.0) as usize;
        let min_y = (py - radius).max(0.0) as usize;
        let max_x = ((px + radius).max(0.0) as usize).min(hay_c.w.saturating_sub(tpl_c.w));
        let max_y = ((py + radius).max(0.0) as usize).min(hay_c.h.saturating_sub(tpl_c.h));
        if let Some(hit) = zncc_search(
            &hay_c,
            &integ_c,
            &tpl_c,
            &tzm_c,
            tnorm_c,
            (min_x, max_x),
            (min_y, max_y),
        ) {
            if hit.2 >= ROI_ACCEPT_SCORE {
                candidate = Some(hit);
            }
        }
    }
    if candidate.is_none() {
        if let Some(hit) = zncc_search(
            &hay_c,
            &integ_c,
            &tpl_c,
            &tzm_c,
            tnorm_c,
            (0, hay_c.w.saturating_sub(tpl_c.w)),
            (0, hay_c.h.saturating_sub(tpl_c.h)),
        ) {
            if hit.2 >= FULL_ACCEPT_SCORE {
                candidate = Some(hit);
            }
        }
    }
    let (cx, cy, coarse_score) = candidate?;

    let best = if coarse_f > 1 {
        let pad = coarse_f * 2 + 2;
        let gx = (cx * coarse_f).min(hay.w.saturating_sub(tgray.w));
        let gy = (cy * coarse_f).min(hay.h.saturating_sub(tgray.h));
        let (tzm, tnorm) = template_stats(&tgray)?;
        let x0 = gx.saturating_sub(pad);
        let y0 = gy.saturating_sub(pad);
        let sub_w = (gx + pad + tgray.w).min(hay.w) - x0;
        let sub_h = (gy + pad + tgray.h).min(hay.h) - y0;
        let sub = crop_gray(&hay, x0, y0, sub_w, sub_h);
        if sub.w < tgray.w || sub.h < tgray.h {
            return None;
        }
        let integ = build_integral(&sub);
        let hit = zncc_search(
            &sub,
            &integ,
            &tgray,
            &tzm,
            tnorm,
            (0, sub.w - tgray.w),
            (0, sub.h - tgray.h),
        )?;
        if hit.2 < FINAL_ACCEPT_SCORE {
            return None;
        }
        (x0 + hit.0, y0 + hit.1, hit.2)
    } else {
        (cx, cy, coarse_score)
    };

    let click_x = best.0 as f64 + click_dx;
    let click_y = best.1 as f64 + click_dy;
    Some(LocalMatch {
        x_norm: (click_x / hay.w as f64 * 1000.0).clamp(0.0, 1000.0),
        y_norm: (click_y / hay.h as f64 * 1000.0).clamp(0.0, 1000.0),
        score: best.2,
    })
}

pub fn frame_diff_ratio(first: &image::RgbaImage, second: &image::RgbaImage) -> f64 {
    if first.dimensions() != second.dimensions() {
        return 1.0;
    }
    let (w, h) = first.dimensions();
    if w == 0 || h == 0 {
        return 0.0;
    }
    let stride = (w.max(h) / SETTLE_SAMPLE_DIM).max(1);
    let mut total = 0f64;
    let mut count = 0f64;
    let mut y = 0;
    while y < h {
        let mut x = 0;
        while x < w {
            let pa = first.get_pixel(x, y).0;
            let pb = second.get_pixel(x, y).0;
            let ga = 0.299 * f64::from(pa[0]) + 0.587 * f64::from(pa[1]) + 0.114 * f64::from(pa[2]);
            let gb = 0.299 * f64::from(pb[0]) + 0.587 * f64::from(pb[1]) + 0.114 * f64::from(pb[2]);
            total += (ga - gb).abs() / 255.0;
            count += 1.0;
            x += stride;
        }
        y += stride;
    }
    if count > 0.0 {
        total / count
    } else {
        0.0
    }
}
