// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DesignerSubMode {
    Prototype,
    LiveArtifact,
    Deck,
    Diagram,
    Image,
    Video,
    HyperFrames,
    Audio,
    FromFigma,
    FromTemplate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DesignerSurface {
    Html,
    Deck,
    Diagram,
    Image,
    Video,
    Audio,
}

impl DesignerSubMode {
    pub fn all() -> &'static [DesignerSubMode] {
        &[
            Self::Prototype,
            Self::LiveArtifact,
            Self::Deck,
            Self::Diagram,
            Self::Image,
            Self::Video,
            Self::HyperFrames,
            Self::Audio,
            Self::FromFigma,
            Self::FromTemplate,
        ]
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id.trim().to_ascii_lowercase().as_str() {
            "prototype" => Some(Self::Prototype),
            "live-artifact" | "live_artifact" | "liveartifact" | "dashboard" | "bi"
            | "bi-dashboard" => Some(Self::LiveArtifact),
            "deck" | "slides" | "slide-deck" => Some(Self::Deck),
            "diagram" | "diagrams" | "chart" | "charts" => Some(Self::Diagram),
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            "hyperframes" | "hyperframe" => Some(Self::HyperFrames),
            "audio" => Some(Self::Audio),
            "figma" | "from-figma" => Some(Self::FromFigma),
            "template" | "from-template" => Some(Self::FromTemplate),
            _ => None,
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::Prototype => "prototype",
            Self::LiveArtifact => "live-artifact",
            Self::Deck => "deck",
            Self::Diagram => "diagram",
            Self::Image => "image",
            Self::Video => "video",
            Self::HyperFrames => "hyperframes",
            Self::Audio => "audio",
            Self::FromFigma => "figma",
            Self::FromTemplate => "template",
        }
    }

    pub fn label_en(&self) -> &'static str {
        match self {
            Self::Prototype => "Prototype",
            Self::LiveArtifact => "BI dashboard",
            Self::Deck => "Slide deck",
            Self::Diagram => "Diagram",
            Self::Image => "Image",
            Self::Video => "Video",
            Self::HyperFrames => "HyperFrames",
            Self::Audio => "Audio",
            Self::FromFigma => "From Figma",
            Self::FromTemplate => "From template",
        }
    }

    pub fn label_zh(&self) -> &'static str {
        match self {
            Self::Prototype => "原型",
            Self::LiveArtifact => "BI 看板",
            Self::Deck => "幻灯片",
            Self::Diagram => "图表",
            Self::Image => "图片",
            Self::Video => "视频",
            Self::HyperFrames => "HyperFrames",
            Self::Audio => "音频",
            Self::FromFigma => "来自 Figma",
            Self::FromTemplate => "来自模板",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Prototype => "🧩",
            Self::LiveArtifact => "📊",
            Self::Deck => "🖥",
            Self::Diagram => "📈",
            Self::Image => "🖼",
            Self::Video => "🎬",
            Self::HyperFrames => "✨",
            Self::Audio => "🎧",
            Self::FromFigma => "🅵",
            Self::FromTemplate => "📁",
        }
    }

    pub fn surface(&self) -> DesignerSurface {
        match self {
            Self::Prototype | Self::LiveArtifact | Self::FromFigma | Self::FromTemplate => {
                DesignerSurface::Html
            }
            Self::Deck => DesignerSurface::Deck,
            Self::Diagram => DesignerSurface::Diagram,
            Self::Image => DesignerSurface::Image,
            Self::Video | Self::HyperFrames => DesignerSurface::Video,
            Self::Audio => DesignerSurface::Audio,
        }
    }

    pub fn media_surface(&self) -> Option<&'static str> {
        match self.surface() {
            DesignerSurface::Image => Some("image"),
            DesignerSurface::Video => Some("video"),
            DesignerSurface::Audio => Some("audio"),
            DesignerSurface::Html | DesignerSurface::Deck | DesignerSurface::Diagram => None,
        }
    }
}
