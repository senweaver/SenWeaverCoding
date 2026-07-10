// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub struct FontSource {
    pub bytes: Vec<u8>,
    pub index: u32,
    pub path: PathBuf,
}

pub struct DiscoveredFonts {
    pub regular: FontSource,
    pub bold: Option<FontSource>,
}

const GOOD_COVERAGE: f32 = 0.98;
const MIN_COVERAGE: f32 = 0.5;

fn cached_pick() -> &'static Mutex<Option<(PathBuf, u32, Option<PathBuf>)>> {
    static CACHE: OnceLock<Mutex<Option<(PathBuf, u32, Option<PathBuf>)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn candidate_pairs() -> Vec<(PathBuf, Option<PathBuf>)> {
    let mut out: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        let fonts_dir = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("windir"))
            .map(|r| PathBuf::from(r).join("Fonts"))
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows\Fonts"));
        let pairs: [(&str, Option<&str>); 9] = [
            ("msyh.ttc", Some("msyhbd.ttc")),
            ("msyh.ttf", Some("msyhbd.ttf")),
            ("Deng.ttf", Some("Dengb.ttf")),
            ("simhei.ttf", None),
            ("simsun.ttc", None),
            ("msjh.ttc", Some("msjhbd.ttc")),
            ("YuGothM.ttc", Some("YuGothB.ttc")),
            ("malgun.ttf", Some("malgunbd.ttf")),
            ("arialuni.ttf", None),
        ];
        for (reg, bold) in pairs {
            out.push((fonts_dir.join(reg), bold.map(|b| fonts_dir.join(b))));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let pairs: [(&str, Option<&str>); 5] = [
            ("/System/Library/Fonts/PingFang.ttc", None),
            ("/System/Library/Fonts/Hiragino Sans GB.ttc", None),
            ("/System/Library/Fonts/STHeiti Medium.ttc", None),
            ("/System/Library/Fonts/STHeiti Light.ttc", None),
            ("/Library/Fonts/Arial Unicode.ttf", None),
        ];
        for (reg, bold) in pairs {
            out.push((PathBuf::from(reg), bold.map(PathBuf::from)));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let pairs: [(&str, Option<&str>); 8] = [
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                Some("/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc"),
            ),
            (
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                Some("/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc"),
            ),
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
                Some("/usr/share/fonts/opentype/noto/NotoSansCJKsc-Bold.otf"),
            ),
            ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", None),
            (
                "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
                None,
            ),
            ("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", None),
            (
                "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
                None,
            ),
            (
                "/usr/share/fonts/truetype/arphic/uming.ttc",
                None,
            ),
        ];
        for (reg, bold) in pairs {
            out.push((PathBuf::from(reg), bold.map(PathBuf::from)));
        }
    }
    out
}

fn load_source(path: &Path, index: u32) -> Option<FontSource> {
    let bytes = std::fs::read(path).ok()?;
    Some(FontSource {
        bytes,
        index,
        path: path.to_path_buf(),
    })
}

fn load_bold_companion(path: Option<&PathBuf>) -> Option<FontSource> {
    let path = path?;
    let bytes = std::fs::read(path).ok()?;
    ttf_parser::Face::parse(&bytes, 0).ok()?;
    Some(FontSource {
        bytes,
        index: 0,
        path: path.clone(),
    })
}

pub fn discover_cjk_fonts(required: &BTreeSet<char>) -> Option<DiscoveredFonts> {
    if let Ok(guard) = cached_pick().lock() {
        if let Some((path, index, bold_path)) = guard.clone() {
            if let Ok(bytes) = std::fs::read(&path) {
                if let Some(ratio) = super::pdf_font::coverage_ratio(&bytes, index, required) {
                    if ratio >= GOOD_COVERAGE {
                        return Some(DiscoveredFonts {
                            regular: FontSource {
                                bytes,
                                index,
                                path: path.clone(),
                            },
                            bold: load_bold_companion(bold_path.as_ref()),
                        });
                    }
                }
            }
        }
    }

    let mut fallback: Option<(PathBuf, Option<PathBuf>, u32, f32)> = None;
    for (reg_path, bold_path) in candidate_pairs() {
        let Ok(bytes) = std::fs::read(&reg_path) else {
            continue;
        };
        let Some((index, ratio)) =
            super::pdf_font::best_face_index(&bytes, required, GOOD_COVERAGE)
        else {
            continue;
        };
        if ratio >= GOOD_COVERAGE {
            if let Ok(mut guard) = cached_pick().lock() {
                *guard = Some((reg_path.clone(), index, bold_path.clone()));
            }
            return Some(DiscoveredFonts {
                regular: FontSource {
                    bytes,
                    index,
                    path: reg_path,
                },
                bold: load_bold_companion(bold_path.as_ref()),
            });
        }
        if ratio >= MIN_COVERAGE
            && fallback
                .as_ref()
                .map(|(_, _, _, best)| ratio > *best)
                .unwrap_or(true)
        {
            fallback = Some((reg_path, bold_path, index, ratio));
        }
    }

    let (reg_path, bold_path, index, _) = fallback?;
    let regular = load_source(&reg_path, index)?;
    if let Ok(mut guard) = cached_pick().lock() {
        *guard = Some((reg_path, index, bold_path.clone()));
    }
    Some(DiscoveredFonts {
        regular,
        bold: load_bold_companion(bold_path.as_ref()),
    })
}
