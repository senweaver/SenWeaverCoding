---
name: card-xiaohongshu
zh_name: "小红书图文卡片"
en_name: "Xiaohongshu Card"
emoji: "📱"
description: "Xiaohongshu-style knowledge cards, arranged as a swipeable multi-card carousel."
zh_description: "小红书风格知识卡片, 多张联排可滑动浏览"
en_description: "Xiaohongshu-style knowledge cards, arranged as a swipeable multi-card carousel."
category: card
scenario: marketing
aspect_hint: "1080×1440 (3:4)"
featured: 24
tags: ["xhs", "小红书", "carousel", "图文"]
example_id: sample-xhs-ai-habits
example_name: "小红书图文卡 · AI 工具习惯"
example_format: markdown
example_tagline: "7 张连排, 莫兰迪渐变"
example_desc: "干货卡片合集, 适合截图发小红书 / 朋友圈"
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
  example_prompt: "Use the Xiaohongshu Card template to turn my content into a Xiaohongshu-style swipeable knowledge-card carousel. Preserve the template's visual signature, use real content and data, and avoid lorem ipsum or placeholder images."
  example_prompt_i18n:
    zh-CN: "用「小红书图文卡片」模板把我的内容做成一份「小红书风格知识卡片, 多张联排可滑动浏览」。保持模板的视觉签名，使用真实内容和数据，避免 lorem ipsum 和占位图片。"
---

【模板: 小红书图文卡片】
- 输出 N 张连续卡片, 每张 `w-[1080px] h-[1440px]`, 用 flex 纵向排列方便整体截图也方便单张截图。N 由【用户内容】信息量决定: 短内容 3-6 张起步, 长内容应更多 (小红书平台单帖最多 18 图, 通常 9 张以内最佳); 一张卡只承载一个核心观点。
- 第一张是封面: 巨大的标题 + 1 行副标题 + 一个吸引人的标签 (类似 "干货预警" / "建议收藏")。
- 中间几张展开正文, 每张一个核心观点, 配 emoji + 短句 + 1-2 个例子。
- 最后一张是总结 + 行动号召 (关注 / 收藏 / 评论)。
- 配色: 选择柔和的莫兰迪色或粉色系; 元素圆润, 大量留白。
- 字号大、行距宽、对比强（小红书在手机上看, 小字根本看不清）。
- 每张卡片右下角小水印 (作者名 / 日期)。

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
