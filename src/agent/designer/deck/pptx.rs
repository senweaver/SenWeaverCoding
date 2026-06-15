// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::io::Write;
use std::path::Path;

use super::render::{
    RenderBackground, RenderBlock, RenderDeck, RenderImageBlock, RenderShapeBlock, RenderSlide,
    RenderTableBlock, RenderTextBlock,
};

const EMU_SLIDE_CY: u64 = 6_858_000;
const EMU_NOTES_CX: u64 = 6_858_000;
const EMU_NOTES_CY: u64 = 9_144_000;

const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const R_OFFICE_DOC: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const R_CORE_PROPS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const R_EXT_PROPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const R_SLIDE_MASTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
const R_NOTES_MASTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";
const R_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const R_SLIDE_LAYOUT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
const R_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const R_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const R_NOTES_SLIDE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
const R_PRES_PROPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps";
const R_VIEW_PROPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps";
const R_TABLE_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles";

const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' => {}
            _ => out.push(ch),
        }
    }
    out
}

fn hex6(raw: &str) -> String {
    let hex: String = raw
        .trim()
        .trim_start_matches('#')
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(6)
        .collect::<String>()
        .to_ascii_uppercase();
    if hex.len() == 6 {
        hex
    } else if hex.len() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect()
    } else {
        "000000".to_string()
    }
}

fn slide_emu(stage_w: u32, stage_h: u32) -> (u64, u64) {
    let w = stage_w.max(1) as u64;
    let h = stage_h.max(1) as u64;
    let cx = (EMU_SLIDE_CY.saturating_mul(w) / h / 100) * 100;
    (cx.max(914_400), EMU_SLIDE_CY)
}

fn rels_xml(entries: &[(String, &str, String)]) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"{REL_NS}\">"
    );
    for (id, rel_type, target) in entries {
        out.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{rel_type}\" Target=\"{target}\"/>"
        ));
    }
    out.push_str("</Relationships>");
    out
}

struct EmuMapper {
    sx: f64,
    sy: f64,
}

impl EmuMapper {
    fn new(cx: u64, cy: u64, stage_w: u32, stage_h: u32) -> Self {
        Self {
            sx: cx as f64 / stage_w.max(1) as f64,
            sy: cy as f64 / stage_h.max(1) as f64,
        }
    }
    fn rect(&self, x: f64, y: f64, w: f64, h: f64) -> (i64, i64, i64, i64) {
        (
            (x * self.sx).round() as i64,
            (y * self.sy).round() as i64,
            ((w.max(0.5) * self.sx).round() as i64).max(6_350),
            ((h.max(0.5) * self.sy).round() as i64).max(6_350),
        )
    }
}

fn font_size_attr(px: f64) -> u64 {
    ((px.max(8.0) * 50.0).round() as u64).clamp(400, 40_000)
}

fn align_code(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "center" | "ctr" => "ctr",
        "right" | "end" | "r" => "r",
        "justify" | "just" => "just",
        _ => "l",
    }
}

fn anchor_code(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "middle" | "center" | "ctr" => "ctr",
        "bottom" | "b" => "b",
        _ => "t",
    }
}

struct MediaStore {
    items: Vec<(String, Vec<u8>)>,
    seen: std::collections::HashMap<u64, String>,
}

fn content_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.len().hash(&mut hasher);
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8
}

impl MediaStore {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            seen: std::collections::HashMap::new(),
        }
    }

    fn add(&mut self, bytes: Vec<u8>) -> String {
        let key = content_hash(&bytes);
        if let Some(existing) = self.seen.get(&key) {
            return existing.clone();
        }
        let (ext, stored) = if is_jpeg(&bytes) {
            ("jpeg", bytes)
        } else if is_png(&bytes) {
            ("png", bytes)
        } else {
            let converted = image::load_from_memory(&bytes).ok().and_then(|img| {
                let mut buf = std::io::Cursor::new(Vec::new());
                img.write_to(&mut buf, image::ImageFormat::Png)
                    .ok()
                    .map(|_| buf.into_inner())
            });
            match converted {
                Some(png_bytes) => ("png", png_bytes),
                None => ("png", bytes),
            }
        };
        let name = format!("image{}.{ext}", self.items.len() + 1);
        self.items.push((name.clone(), stored));
        self.seen.insert(key, name.clone());
        name
    }
}

fn content_types_xml(slide_count: usize, with_notes: bool, media: &MediaStore) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>",
    );
    let has_png = media.items.iter().any(|(n, _)| n.ends_with(".png"));
    let has_jpeg = media.items.iter().any(|(n, _)| n.ends_with(".jpeg"));
    if has_png {
        out.push_str("<Default Extension=\"png\" ContentType=\"image/png\"/>");
    }
    if has_jpeg {
        out.push_str("<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>");
    }
    out.push_str(
        "<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\
         <Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>\
         <Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>\
         <Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\
         <Override PartName=\"/ppt/presProps.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presProps+xml\"/>\
         <Override PartName=\"/ppt/viewProps.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml\"/>\
         <Override PartName=\"/ppt/tableStyles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml\"/>\
         <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
         <Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>",
    );
    if with_notes {
        out.push_str(
            "<Override PartName=\"/ppt/notesMasters/notesMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml\"/>\
             <Override PartName=\"/ppt/theme/theme2.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>",
        );
    }
    for i in 1..=slide_count {
        out.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
        if with_notes {
            out.push_str(&format!(
                "<Override PartName=\"/ppt/notesSlides/notesSlide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml\"/>"
            ));
        }
    }
    out.push_str("</Types>");
    out
}

fn presentation_xml(slide_count: usize, with_notes: bool, cx: u64, cy: u64) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:presentation xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\">\
         <p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>"
    );
    if with_notes {
        let notes_rid = slide_count + 2;
        out.push_str(&format!(
            "<p:notesMasterIdLst><p:notesMasterId r:id=\"rId{notes_rid}\"/></p:notesMasterIdLst>"
        ));
    }
    out.push_str("<p:sldIdLst>");
    for i in 1..=slide_count {
        let sld_id = 255 + i;
        let rid = 1 + i;
        out.push_str(&format!("<p:sldId id=\"{sld_id}\" r:id=\"rId{rid}\"/>"));
    }
    out.push_str(&format!(
        "</p:sldIdLst><p:sldSz cx=\"{cx}\" cy=\"{cy}\"/><p:notesSz cx=\"{EMU_NOTES_CX}\" cy=\"{EMU_NOTES_CY}\"/></p:presentation>"
    ));
    out
}

fn presentation_rels(slide_count: usize, with_notes: bool) -> String {
    let mut entries: Vec<(String, &str, String)> = vec![(
        "rId1".to_string(),
        R_SLIDE_MASTER,
        "slideMasters/slideMaster1.xml".to_string(),
    )];
    for i in 1..=slide_count {
        entries.push((
            format!("rId{}", 1 + i),
            R_SLIDE,
            format!("slides/slide{i}.xml"),
        ));
    }
    let mut next = slide_count + 2;
    if with_notes {
        entries.push((
            format!("rId{next}"),
            R_NOTES_MASTER,
            "notesMasters/notesMaster1.xml".to_string(),
        ));
        next += 1;
    }
    entries.push((format!("rId{next}"), R_PRES_PROPS, "presProps.xml".to_string()));
    entries.push((
        format!("rId{}", next + 1),
        R_VIEW_PROPS,
        "viewProps.xml".to_string(),
    ));
    entries.push((
        format!("rId{}", next + 2),
        R_TABLE_STYLES,
        "tableStyles.xml".to_string(),
    ));
    rels_xml(&entries)
}

fn empty_sp_tree() -> &'static str {
    "<p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
     <p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>"
}

fn clr_map() -> &'static str {
    "<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>"
}

fn slide_master_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:sldMaster xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\">\
         <p:cSld>{tree}</p:spTree></p:cSld>{map}\
         <p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst>\
         <p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles>\
         </p:sldMaster>",
        tree = empty_sp_tree(),
        map = clr_map(),
    )
}

fn slide_master_rels() -> String {
    rels_xml(&[
        (
            "rId1".to_string(),
            R_SLIDE_LAYOUT,
            "../slideLayouts/slideLayout1.xml".to_string(),
        ),
        ("rId2".to_string(), R_THEME, "../theme/theme1.xml".to_string()),
    ])
}

fn slide_layout_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:sldLayout xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\" type=\"blank\">\
         <p:cSld name=\"Blank\">{tree}</p:spTree></p:cSld>\
         <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>\
         </p:sldLayout>",
        tree = empty_sp_tree(),
    )
}

fn slide_layout_rels() -> String {
    rels_xml(&[(
        "rId1".to_string(),
        R_SLIDE_MASTER,
        "../slideMasters/slideMaster1.xml".to_string(),
    )])
}

fn notes_master_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:notesMaster xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\">\
         <p:cSld>{tree}</p:spTree></p:cSld>{map}\
         </p:notesMaster>",
        tree = empty_sp_tree(),
        map = clr_map(),
    )
}

fn notes_master_rels() -> String {
    rels_xml(&[("rId1".to_string(), R_THEME, "../theme/theme2.xml".to_string())])
}

struct ThemePalette {
    text: String,
    background: String,
    muted: String,
    surface: String,
    accent: String,
    accent2: String,
    hairline: String,
}

fn theme_xml(name: &str, palette: &ThemePalette, major: &str, major_ea: &str, minor: &str, minor_ea: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <a:theme xmlns:a=\"{NS_A}\" name=\"{name}\"><a:themeElements>\
         <a:clrScheme name=\"{name}\">\
         <a:dk1><a:srgbClr val=\"{text}\"/></a:dk1>\
         <a:lt1><a:srgbClr val=\"{background}\"/></a:lt1>\
         <a:dk2><a:srgbClr val=\"{muted}\"/></a:dk2>\
         <a:lt2><a:srgbClr val=\"{surface}\"/></a:lt2>\
         <a:accent1><a:srgbClr val=\"{accent}\"/></a:accent1>\
         <a:accent2><a:srgbClr val=\"{accent2}\"/></a:accent2>\
         <a:accent3><a:srgbClr val=\"{muted}\"/></a:accent3>\
         <a:accent4><a:srgbClr val=\"{hairline}\"/></a:accent4>\
         <a:accent5><a:srgbClr val=\"{accent}\"/></a:accent5>\
         <a:accent6><a:srgbClr val=\"{accent2}\"/></a:accent6>\
         <a:hlink><a:srgbClr val=\"{accent}\"/></a:hlink>\
         <a:folHlink><a:srgbClr val=\"{accent2}\"/></a:folHlink>\
         </a:clrScheme>\
         <a:fontScheme name=\"{name}\">\
         <a:majorFont><a:latin typeface=\"{major}\"/><a:ea typeface=\"{major_ea}\"/><a:cs typeface=\"\"/></a:majorFont>\
         <a:minorFont><a:latin typeface=\"{minor}\"/><a:ea typeface=\"{minor_ea}\"/><a:cs typeface=\"\"/></a:minorFont>\
         </a:fontScheme>\
         <a:fmtScheme name=\"{name}\">\
         <a:fillStyleLst>\
         <a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
         <a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
         <a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
         </a:fillStyleLst>\
         <a:lnStyleLst>\
         <a:ln w=\"6350\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>\
         <a:ln w=\"12700\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>\
         <a:ln w=\"19050\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>\
         </a:lnStyleLst>\
         <a:effectStyleLst>\
         <a:effectStyle><a:effectLst/></a:effectStyle>\
         <a:effectStyle><a:effectLst/></a:effectStyle>\
         <a:effectStyle><a:effectLst/></a:effectStyle>\
         </a:effectStyleLst>\
         <a:bgFillStyleLst>\
         <a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
         <a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
         <a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
         </a:bgFillStyleLst>\
         </a:fmtScheme>\
         </a:themeElements></a:theme>",
        text = palette.text,
        background = palette.background,
        muted = palette.muted,
        surface = palette.surface,
        accent = palette.accent,
        accent2 = palette.accent2,
        hairline = palette.hairline,
    )
}

fn run_fonts(deck: &RenderDeck, font: &str) -> (String, String) {
    if font.eq_ignore_ascii_case("heading") {
        (
            deck.fonts.heading_latin.clone(),
            deck.fonts.heading_ea.clone(),
        )
    } else {
        (deck.fonts.body_latin.clone(), deck.fonts.body_ea.clone())
    }
}

fn bullet_indent(level: u8) -> (i64, i64) {
    match level {
        0 => (342_900, -342_900),
        1 => (742_950, -342_900),
        _ => (1_143_000, -342_900),
    }
}

fn text_shape_xml(
    block: &RenderTextBlock,
    deck: &RenderDeck,
    shape_id: usize,
    mapper: &EmuMapper,
    accent_hex: &str,
) -> String {
    let (off_x, off_y, ext_x, ext_y) = mapper.rect(block.x, block.y, block.w, block.h);
    let anchor = anchor_code(&block.valign);
    let ln_spc_val = ((block.line_spacing.clamp(0.6, 3.0)) * 100_000.0).round() as u64;
    let algn = align_code(&block.align);

    let mut paragraphs_xml = String::new();
    for para in &block.paragraphs {
        let mut p_pr = format!("<a:pPr algn=\"{algn}\"");
        if para.bullet {
            let (mar_l, indent) = bullet_indent(para.level);
            p_pr.push_str(&format!(" lvl=\"{}\" marL=\"{mar_l}\" indent=\"{indent}\"", para.level.min(8)));
        }
        p_pr.push('>');
        p_pr.push_str(&format!(
            "<a:lnSpc><a:spcPct val=\"{ln_spc_val}\"/></a:lnSpc>"
        ));
        if para.space_before > 0.1 {
            let pts = (para.space_before * 50.0).round() as u64;
            p_pr.push_str(&format!("<a:spcBef><a:spcPts val=\"{pts}\"/></a:spcBef>"));
        }
        if para.bullet {
            let marker = para
                .bullet_char
                .as_deref()
                .filter(|m| !m.is_empty())
                .unwrap_or("•");
            p_pr.push_str(&format!(
                "<a:buClr><a:srgbClr val=\"{accent_hex}\"/></a:buClr><a:buChar char=\"{}\"/>",
                xml_escape(marker)
            ));
        } else {
            p_pr.push_str("<a:buNone/>");
        }
        p_pr.push_str("</a:pPr>");

        let mut runs_xml = String::new();
        for run in &para.runs {
            if run.text.is_empty() {
                continue;
            }
            let sz = font_size_attr(run.size);
            let bold = if run.bold { " b=\"1\"" } else { "" };
            let italic = if run.italic { " i=\"1\"" } else { "" };
            let color = hex6(&run.color);
            let (latin, ea) = run_fonts(deck, &run.font);
            runs_xml.push_str(&format!(
                "<a:r><a:rPr lang=\"en-US\" altLang=\"zh-CN\" sz=\"{sz}\"{bold}{italic} dirty=\"0\">\
                 <a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill>\
                 <a:latin typeface=\"{}\"/><a:ea typeface=\"{}\"/></a:rPr>\
                 <a:t>{}</a:t></a:r>",
                xml_escape(&latin),
                xml_escape(&ea),
                xml_escape(&run.text)
            ));
        }
        if runs_xml.is_empty() {
            continue;
        }
        paragraphs_xml.push_str(&format!("<a:p>{p_pr}{runs_xml}</a:p>"));
    }
    if paragraphs_xml.is_empty() {
        return String::new();
    }

    format!(
        "<p:sp>\
         <p:nvSpPr><p:cNvPr id=\"{shape_id}\" name=\"{}\"/>\
         <p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr>\
         <p:spPr><a:xfrm><a:off x=\"{off_x}\" y=\"{off_y}\"/><a:ext cx=\"{ext_x}\" cy=\"{ext_y}\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>\
         <p:txBody>\
         <a:bodyPr wrap=\"square\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"{anchor}\"><a:normAutofit/></a:bodyPr>\
         <a:lstStyle/>{paragraphs_xml}</p:txBody>\
         </p:sp>",
        xml_escape(&format!("Text {}", block.id)),
    )
}

fn shape_xml(block: &RenderShapeBlock, shape_id: usize, mapper: &EmuMapper) -> String {
    let (off_x, off_y, ext_x, ext_y) = mapper.rect(block.x, block.y, block.w, block.h);
    let geom = match block.shape.as_str() {
        "ellipse" => "<a:prstGeom prst=\"ellipse\"><a:avLst/></a:prstGeom>".to_string(),
        "line" => "<a:prstGeom prst=\"line\"><a:avLst/></a:prstGeom>".to_string(),
        "roundRect" => {
            let min_side = block.w.min(block.h).max(1.0);
            let adj = (((block.radius.max(0.0) / min_side) * 100_000.0).round() as u64).min(50_000);
            format!(
                "<a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val {adj}\"/></a:avLst></a:prstGeom>"
            )
        }
        _ => {
            if block.radius > 0.0 {
                let min_side = block.w.min(block.h).max(1.0);
                let adj =
                    (((block.radius / min_side) * 100_000.0).round() as u64).min(50_000);
                format!(
                    "<a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val {adj}\"/></a:avLst></a:prstGeom>"
                )
            } else {
                "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>".to_string()
            }
        }
    };
    let fill = match &block.fill {
        Some(paint) => {
            let color = hex6(&paint.color);
            let alpha = ((paint.alpha.clamp(0.0, 1.0)) * 100_000.0).round() as u64;
            if alpha >= 100_000 {
                format!("<a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill>")
            } else {
                format!(
                    "<a:solidFill><a:srgbClr val=\"{color}\"><a:alpha val=\"{alpha}\"/></a:srgbClr></a:solidFill>"
                )
            }
        }
        None => "<a:noFill/>".to_string(),
    };
    let line = match &block.stroke {
        Some(stroke) => {
            let color = hex6(&stroke.color);
            let w_emu = ((stroke.width.max(0.25)) * 9_525.0).round() as u64;
            let alpha = ((stroke.alpha.clamp(0.0, 1.0)) * 100_000.0).round() as u64;
            let clr = if alpha >= 100_000 {
                format!("<a:srgbClr val=\"{color}\"/>")
            } else {
                format!("<a:srgbClr val=\"{color}\"><a:alpha val=\"{alpha}\"/></a:srgbClr>")
            };
            format!("<a:ln w=\"{w_emu}\"><a:solidFill>{clr}</a:solidFill></a:ln>")
        }
        None => "<a:ln><a:noFill/></a:ln>".to_string(),
    };
    format!(
        "<p:sp>\
         <p:nvSpPr><p:cNvPr id=\"{shape_id}\" name=\"{}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>\
         <p:spPr><a:xfrm><a:off x=\"{off_x}\" y=\"{off_y}\"/><a:ext cx=\"{ext_x}\" cy=\"{ext_y}\"/></a:xfrm>\
         {geom}{fill}{line}</p:spPr>\
         <p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody>\
         </p:sp>",
        xml_escape(&format!("Shape {}", block.id)),
    )
}

struct PlacedImage {
    rid: usize,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
    crop: Option<(u64, u64, u64, u64)>,
    name: String,
}

fn place_image(
    block: &RenderImageBlock,
    workspace: &Path,
    media: &mut MediaStore,
    rid: usize,
) -> Option<PlacedImage> {
    let abs = workspace.join(block.src.replace('/', std::path::MAIN_SEPARATOR_STR));
    let bytes = std::fs::read(&abs).ok()?;
    let dims = image::load_from_memory(&bytes)
        .ok()
        .map(|img| (img.width(), img.height()));
    let name = media.add(bytes);

    let (mut x, mut y, mut w, mut h) = (block.x, block.y, block.w.max(1.0), block.h.max(1.0));
    let mut crop = None;
    if let Some((iw, ih)) = dims.filter(|(iw, ih)| *iw > 0 && *ih > 0) {
        let img_aspect = iw as f64 / ih as f64;
        let frame_aspect = w / h;
        if block.fit == "contain" {
            if img_aspect > frame_aspect {
                let new_h = w / img_aspect;
                y += (h - new_h) / 2.0;
                h = new_h;
            } else {
                let new_w = h * img_aspect;
                x += (w - new_w) / 2.0;
                w = new_w;
            }
        } else if (img_aspect - frame_aspect).abs() > 0.005 {
            if img_aspect > frame_aspect {
                let keep = frame_aspect / img_aspect;
                let cut = (((1.0 - keep) / 2.0) * 100_000.0).round() as u64;
                crop = Some((cut, 0, cut, 0));
            } else {
                let keep = img_aspect / frame_aspect;
                let cut = (((1.0 - keep) / 2.0) * 100_000.0).round() as u64;
                crop = Some((0, cut, 0, cut));
            }
        }
    }
    Some(PlacedImage {
        rid,
        x,
        y,
        w,
        h,
        radius: block.radius,
        crop,
        name,
    })
}

fn pic_xml(placed: &PlacedImage, block_id: &str, shape_id: usize, mapper: &EmuMapper) -> String {
    let (off_x, off_y, ext_x, ext_y) = mapper.rect(placed.x, placed.y, placed.w, placed.h);
    let src_rect = match placed.crop {
        Some((l, t, r, b)) => format!("<a:srcRect l=\"{l}\" t=\"{t}\" r=\"{r}\" b=\"{b}\"/>"),
        None => String::new(),
    };
    let geom = if placed.radius > 0.0 {
        let min_side = placed.w.min(placed.h).max(1.0);
        let adj = (((placed.radius / min_side) * 100_000.0).round() as u64).min(50_000);
        format!(
            "<a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val {adj}\"/></a:avLst></a:prstGeom>"
        )
    } else {
        "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>".to_string()
    };
    format!(
        "<p:pic>\
         <p:nvPicPr><p:cNvPr id=\"{shape_id}\" name=\"{}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>\
         <p:blipFill><a:blip r:embed=\"rId{}\"/>{src_rect}<a:stretch><a:fillRect/></a:stretch></p:blipFill>\
         <p:spPr><a:xfrm><a:off x=\"{off_x}\" y=\"{off_y}\"/><a:ext cx=\"{ext_x}\" cy=\"{ext_y}\"/></a:xfrm>\
         {geom}</p:spPr>\
         </p:pic>",
        xml_escape(&format!("Image {block_id}")),
        placed.rid,
    )
}

fn table_xml(
    block: &RenderTableBlock,
    shape_id: usize,
    mapper: &EmuMapper,
) -> String {
    let (off_x, off_y, ext_x, ext_y) = mapper.rect(block.x, block.y, block.w, block.h);
    let row_count = block.rows.len().max(1);
    let row_h = (ext_y as f64 / row_count as f64).round() as i64;

    let mut grid = String::from("<a:tblGrid>");
    for frac in &block.col_fracs {
        let w = ((ext_x as f64) * frac).round() as i64;
        grid.push_str(&format!("<a:gridCol w=\"{}\"/>", w.max(6_350)));
    }
    grid.push_str("</a:tblGrid>");

    let text_hex = hex6(&block.text_color);
    let header_fill = hex6(&block.header_fill);
    let header_text = hex6(&block.header_text);
    let row_fill = hex6(&block.row_fill);
    let hairline = hex6(&block.hairline);
    let sz = font_size_attr(block.size);
    let header_sz = font_size_attr(block.size * 0.95);
    let latin = xml_escape(&block.font_latin);
    let ea = xml_escape(&block.font_ea);

    let mut rows_xml = String::new();
    for (ri, row) in block.rows.iter().enumerate() {
        let is_header = block.header_row && ri == 0;
        rows_xml.push_str(&format!("<a:tr h=\"{row_h}\">"));
        for cell in row {
            let (color, size_attr, bold) = if is_header {
                (header_text.as_str(), header_sz, " b=\"1\"")
            } else {
                (text_hex.as_str(), sz, "")
            };
            let fill = if is_header {
                format!("<a:solidFill><a:srgbClr val=\"{header_fill}\"/></a:solidFill>")
            } else {
                format!("<a:solidFill><a:srgbClr val=\"{row_fill}\"><a:alpha val=\"60000\"/></a:srgbClr></a:solidFill>")
            };
            let border = format!(
                "<a:lnB w=\"9525\"><a:solidFill><a:srgbClr val=\"{hairline}\"><a:alpha val=\"70000\"/></a:srgbClr></a:solidFill></a:lnB>"
            );
            rows_xml.push_str(&format!(
                "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr algn=\"l\"><a:buNone/></a:pPr>\
                 <a:r><a:rPr lang=\"en-US\" altLang=\"zh-CN\" sz=\"{size_attr}\"{bold} dirty=\"0\">\
                 <a:solidFill><a:srgbClr val=\"{color}\"/></a:solidFill>\
                 <a:latin typeface=\"{latin}\"/><a:ea typeface=\"{ea}\"/></a:rPr>\
                 <a:t>{}</a:t></a:r></a:p></a:txBody>\
                 <a:tcPr marL=\"91440\" marR=\"91440\" marT=\"45720\" marB=\"45720\" anchor=\"ctr\">{border}{fill}</a:tcPr></a:tc>",
                xml_escape(cell)
            ));
        }
        rows_xml.push_str("</a:tr>");
    }

    format!(
        "<p:graphicFrame>\
         <p:nvGraphicFramePr><p:cNvPr id=\"{shape_id}\" name=\"{}\"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>\
         <p:xfrm><a:off x=\"{off_x}\" y=\"{off_y}\"/><a:ext cx=\"{ext_x}\" cy=\"{ext_y}\"/></p:xfrm>\
         <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\">\
         <a:tbl><a:tblPr firstRow=\"{}\" bandRow=\"0\"/>{grid}{rows_xml}</a:tbl>\
         </a:graphicData></a:graphic>\
         </p:graphicFrame>",
        xml_escape(&format!("Table {}", block.id)),
        if block.header_row { 1 } else { 0 },
    )
}

fn gradient_fill_xml(from: &str, to: &str, angle: f64) -> String {
    let ooxml_deg = ((angle - 90.0).rem_euclid(360.0) * 60_000.0).round() as u64;
    format!(
        "<a:gradFill><a:gsLst>\
         <a:gs pos=\"0\"><a:srgbClr val=\"{}\"/></a:gs>\
         <a:gs pos=\"100000\"><a:srgbClr val=\"{}\"/></a:gs>\
         </a:gsLst><a:lin ang=\"{ooxml_deg}\" scaled=\"1\"/></a:gradFill>",
        hex6(from),
        hex6(to),
    )
}

fn transition_xml(transition: &str) -> &'static str {
    match transition {
        "none" => "",
        "cinematic" => "<p:transition spd=\"slow\"><p:fade thruBlk=\"1\"/></p:transition>",
        _ => "<p:transition spd=\"med\"><p:fade/></p:transition>",
    }
}

struct BuiltSlide {
    xml: String,
    rels: Vec<(String, &'static str, String)>,
}

fn build_slide(
    index: usize,
    slide: &RenderSlide,
    deck: &RenderDeck,
    cx: u64,
    cy: u64,
    accent_hex: &str,
    workspace: &Path,
    media: &mut MediaStore,
    with_notes: bool,
) -> BuiltSlide {
    let mapper = EmuMapper::new(cx, cy, deck.stage_w, deck.stage_h);
    let mut rels: Vec<(String, &'static str, String)> = vec![(
        "rId1".to_string(),
        R_SLIDE_LAYOUT,
        "../slideLayouts/slideLayout1.xml".to_string(),
    )];
    let mut next_rid = 2usize;
    let mut body = String::new();
    let mut next_id = 2usize;

    let bg = match &slide.background {
        RenderBackground::Color { color } => format!(
            "<p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>",
            hex6(color)
        ),
        RenderBackground::Gradient { from, to, angle } => format!(
            "<p:bg><p:bgPr>{}<a:effectLst/></p:bgPr></p:bg>",
            gradient_fill_xml(from, to, *angle)
        ),
        RenderBackground::Image { src } => {
            let block = RenderImageBlock {
                id: "_bg".to_string(),
                x: 0.0,
                y: 0.0,
                w: deck.stage_w as f64,
                h: deck.stage_h as f64,
                src: src.clone(),
                fit: "cover".to_string(),
                radius: 0.0,
            };
            if let Some(placed) = place_image(&block, workspace, media, next_rid) {
                rels.push((
                    format!("rId{next_rid}"),
                    R_IMAGE,
                    format!("../media/{}", placed.name),
                ));
                next_rid += 1;
                body.push_str(&pic_xml(&placed, "_bg", next_id, &mapper));
                next_id += 1;
            }
            String::new()
        }
    };

    for block in &slide.blocks {
        match block {
            RenderBlock::Text(text) => {
                let xml = text_shape_xml(text, deck, next_id, &mapper, accent_hex);
                if !xml.is_empty() {
                    body.push_str(&xml);
                    next_id += 1;
                }
            }
            RenderBlock::Shape(shape) => {
                body.push_str(&shape_xml(shape, next_id, &mapper));
                next_id += 1;
            }
            RenderBlock::Image(img) => {
                if let Some(placed) = place_image(img, workspace, media, next_rid) {
                    rels.push((
                        format!("rId{next_rid}"),
                        R_IMAGE,
                        format!("../media/{}", placed.name),
                    ));
                    next_rid += 1;
                    body.push_str(&pic_xml(&placed, &img.id, next_id, &mapper));
                    next_id += 1;
                }
            }
            RenderBlock::Table(table) => {
                body.push_str(&table_xml(table, next_id, &mapper));
                next_id += 1;
            }
        }
    }

    if with_notes {
        rels.push((
            format!("rId{next_rid}"),
            R_NOTES_SLIDE,
            format!("../notesSlides/notesSlide{}.xml", index + 1),
        ));
    }

    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:sld xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\">\
         <p:cSld>{bg}{tree}\
         {body}\
         </p:spTree></p:cSld>\
         <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>\
         {transition}\
         </p:sld>",
        tree = empty_sp_tree(),
        transition = transition_xml(&deck.transition),
    );
    BuiltSlide { xml, rels }
}

fn notes_slide_xml(notes: &str) -> String {
    let mut paragraphs = String::new();
    let mut any = false;
    for line in notes.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            paragraphs.push_str("<a:p/>");
        } else {
            paragraphs.push_str(&format!(
                "<a:p><a:r><a:t>{}</a:t></a:r></a:p>",
                xml_escape(trimmed)
            ));
            any = true;
        }
    }
    if !any {
        paragraphs = "<a:p/>".to_string();
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:notes xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\">\
         <p:cSld>{tree}\
         <p:sp>\
         <p:nvSpPr><p:cNvPr id=\"2\" name=\"Notes Placeholder\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>\
         <p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr>\
         <p:spPr/>\
         <p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody>\
         </p:sp>\
         </p:spTree></p:cSld>\
         <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>\
         </p:notes>",
        tree = empty_sp_tree(),
    )
}

fn notes_slide_rels(index: usize) -> String {
    rels_xml(&[
        (
            "rId1".to_string(),
            R_NOTES_MASTER,
            "../notesMasters/notesMaster1.xml".to_string(),
        ),
        (
            "rId2".to_string(),
            R_SLIDE,
            format!("../slides/slide{index}.xml"),
        ),
    ])
}

fn core_props_xml(title: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
         <dc:title>{}</dc:title><dc:creator>SenWeaverCoding</dc:creator>\
         </cp:coreProperties>",
        xml_escape(title)
    )
}

fn app_props_xml(slide_count: usize) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" \
         xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">\
         <Application>SenWeaverCoding Designer</Application><Slides>{slide_count}</Slides>\
         </Properties>"
    )
}

fn pres_props_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:presentationPr xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\"/>"
    )
}

fn view_props_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:viewPr xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\"/>"
    )
}

fn table_styles_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <a:tblStyleLst xmlns:a=\"{NS_A}\" def=\"{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}\"/>"
    )
}

fn deck_palette(deck: &RenderDeck) -> ThemePalette {
    let theme = super::theme::theme_for(&deck.theme);
    ThemePalette {
        text: hex6(theme.colors.text),
        background: hex6(theme.colors.background),
        muted: hex6(theme.colors.muted),
        surface: hex6(theme.colors.surface),
        accent: hex6(theme.colors.accent),
        accent2: hex6(theme.colors.accent2),
        hairline: hex6(theme.colors.hairline),
    }
}

pub fn write_render_pptx(
    out_path: &Path,
    deck: &RenderDeck,
    workspace: &Path,
) -> std::io::Result<()> {
    if deck.slides.is_empty() {
        return Err(std::io::Error::other("no slides to export"));
    }
    let with_notes = deck.slides.iter().any(|s| {
        s.notes
            .as_deref()
            .map(|n| !n.trim().is_empty())
            .unwrap_or(false)
    });
    let (cx, cy) = slide_emu(deck.stage_w, deck.stage_h);
    let count = deck.slides.len();
    let palette = deck_palette(deck);
    let accent_hex = hex6(&deck.accent);

    let mut media = MediaStore::new();
    let mut built: Vec<BuiltSlide> = Vec::with_capacity(count);
    for (idx, slide) in deck.slides.iter().enumerate() {
        built.push(build_slide(
            idx, slide, deck, cx, cy, &accent_hex, workspace, &mut media, with_notes,
        ));
    }

    let file = std::fs::File::create(out_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let deflate: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let stored: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);

    let put = |zip: &mut zip::ZipWriter<std::fs::File>,
                   name: &str,
                   bytes: &[u8],
                   binary: bool|
     -> std::io::Result<()> {
        zip.start_file(name, if binary { stored } else { deflate })
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        zip.write_all(bytes)
    };

    put(
        &mut zip,
        "[Content_Types].xml",
        content_types_xml(count, with_notes, &media).as_bytes(),
        false,
    )?;
    put(
        &mut zip,
        "_rels/.rels",
        rels_xml(&[
            (
                "rId1".to_string(),
                R_OFFICE_DOC,
                "ppt/presentation.xml".to_string(),
            ),
            (
                "rId2".to_string(),
                R_CORE_PROPS,
                "docProps/core.xml".to_string(),
            ),
            (
                "rId3".to_string(),
                R_EXT_PROPS,
                "docProps/app.xml".to_string(),
            ),
        ])
        .as_bytes(),
        false,
    )?;
    put(
        &mut zip,
        "docProps/core.xml",
        core_props_xml(&deck.title).as_bytes(),
        false,
    )?;
    put(
        &mut zip,
        "docProps/app.xml",
        app_props_xml(count).as_bytes(),
        false,
    )?;
    put(
        &mut zip,
        "ppt/presentation.xml",
        presentation_xml(count, with_notes, cx, cy).as_bytes(),
        false,
    )?;
    put(
        &mut zip,
        "ppt/_rels/presentation.xml.rels",
        presentation_rels(count, with_notes).as_bytes(),
        false,
    )?;
    put(&mut zip, "ppt/presProps.xml", pres_props_xml().as_bytes(), false)?;
    put(&mut zip, "ppt/viewProps.xml", view_props_xml().as_bytes(), false)?;
    put(
        &mut zip,
        "ppt/tableStyles.xml",
        table_styles_xml().as_bytes(),
        false,
    )?;
    put(
        &mut zip,
        "ppt/slideMasters/slideMaster1.xml",
        slide_master_xml().as_bytes(),
        false,
    )?;
    put(
        &mut zip,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        slide_master_rels().as_bytes(),
        false,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/slideLayout1.xml",
        slide_layout_xml().as_bytes(),
        false,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        slide_layout_rels().as_bytes(),
        false,
    )?;
    put(
        &mut zip,
        "ppt/theme/theme1.xml",
        theme_xml(
            "SenDeck",
            &palette,
            &deck.fonts.heading_latin,
            &deck.fonts.heading_ea,
            &deck.fonts.body_latin,
            &deck.fonts.body_ea,
        )
        .as_bytes(),
        false,
    )?;
    if with_notes {
        put(
            &mut zip,
            "ppt/theme/theme2.xml",
            theme_xml(
                "SenDeckNotes",
                &palette,
                &deck.fonts.heading_latin,
                &deck.fonts.heading_ea,
                &deck.fonts.body_latin,
                &deck.fonts.body_ea,
            )
            .as_bytes(),
            false,
        )?;
        put(
            &mut zip,
            "ppt/notesMasters/notesMaster1.xml",
            notes_master_xml().as_bytes(),
            false,
        )?;
        put(
            &mut zip,
            "ppt/notesMasters/_rels/notesMaster1.xml.rels",
            notes_master_rels().as_bytes(),
            false,
        )?;
    }

    for (idx, slide) in built.iter().enumerate() {
        let i = idx + 1;
        put(
            &mut zip,
            &format!("ppt/slides/slide{i}.xml"),
            slide.xml.as_bytes(),
            false,
        )?;
        put(
            &mut zip,
            &format!("ppt/slides/_rels/slide{i}.xml.rels"),
            rels_xml(&slide.rels).as_bytes(),
            false,
        )?;
        if with_notes {
            let notes = deck.slides[idx].notes.as_deref().unwrap_or("");
            put(
                &mut zip,
                &format!("ppt/notesSlides/notesSlide{i}.xml"),
                notes_slide_xml(notes).as_bytes(),
                false,
            )?;
            put(
                &mut zip,
                &format!("ppt/notesSlides/_rels/notesSlide{i}.xml.rels"),
                notes_slide_rels(i).as_bytes(),
                false,
            )?;
        }
    }

    for (name, bytes) in &media.items {
        put(&mut zip, &format!("ppt/media/{name}"), bytes, true)?;
    }

    zip.finish()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}
