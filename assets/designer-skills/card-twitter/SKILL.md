---
name: card-twitter
zh_name: "Twitter 分享卡"
en_name: "Twitter Share Card"
emoji: "🐦"
description: "Twitter quote or data card designed to pair with a post."
zh_description: "推特金句 / 数据卡, 适合配推文"
en_description: "Twitter quote or data card designed to pair with a post."
category: card
scenario: marketing
aspect_hint: "1600×900 (16:9)"
tags: ["twitter", "x", "quote", "金句"]
example_id: sample-twitter-quote
example_name: "推特卡 · 金句"
example_format: text
example_tagline: "16:9 暗色金句卡, 截图直接配推文"
example_desc: "高对比金句模板, 含 grid 网格 + 渐变光晕背景"
od:
  mode: prototype
  surface: web
  platform: desktop
  scenario: marketing
  preview:
    type: html
    entry: index.html
    reload: debounce-100
  design_system:
    requires: false
  example_prompt: "Use the Twitter Share Card template to turn my content into a Twitter quote or data card designed to pair with a post. Preserve the template's visual signature, use real content and data, and avoid lorem ipsum or placeholder images."
  example_prompt_i18n:
    zh-CN: "用「Twitter 分享卡」模板把我的内容做成一份「推特金句 / 数据卡, 适合配推文」。保持模板的视觉签名，使用真实内容和数据，避免 lorem ipsum 和占位图片。"
---

【模板: Twitter 分享卡】
- 容器 `w-[1600px] h-[900px]`, 暗色 / 亮色二选一根据内容情绪。
- 中央一句 hero 金句 (text-6xl, font-semibold, 限 2-3 行)。
- 下方作者署名 + 头像占位 + handle。
- 左上角小标签 (类型: "Insight" / "Data" / "Quote")。
- 右下角品牌水印。
- 整张卡片有微妙的纹理 (grid 网格 / noise / dot pattern)。
- 截图后可直接配推文发出, 视觉简洁有力。

## SenWeaverCoding strengthening

Hard requirements for every artifact this skill produces inside Designer
mode, regardless of the workflow above:

- **Bind to the active design system.** Pull palette, type, spacing, and
  radius from the selected `DESIGN.md` / `tokens.css`; never hardcode an
  off-token palette or invent a second accent.
- **Apply the craft layer.** Honour the injected craft references
  (typography, color, anti-ai-slop, state-coverage, accessibility) — they
  are P0 self-checks at the critique stage, not suggestions.
- **Write into the session directory** so the artifact appears in the
  per-session canvas; keep files self-contained (inline assets, no external
  CDNs that break in the sandboxed iframe).
- **Use real content, never filler.** No Lorem Ipsum, no invented metrics,
  no placeholder copy — compose to fill empty space.
- **Ship every interactive state**, not just the populated one, and make
  the design responsive and keyboard-operable.

<!-- swc-strengthened -->
