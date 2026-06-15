---
name: login-flow
description: Mobile login and authentication flow screens
od:
  mode: prototype
  platform: mobile
triggers:
  - login
  - sign in
  - 注册登录
  - 登录注册
  - 手机号登录
  - 验证码登录
  - 密码登录
---

# Login Flow Skill

A skill for generating mobile-first login and authentication screens. Use this when the user wants a sign-in experience for a mobile app, including phone + SMS verification, password-based login, and social SSO options.

## Workflow

1. **Read reference files first** (see below)
2. **Clarify auth method**: phone/SMS, password, or social SSO
3. **Checklist gate** — verify P0 items before emitting `<artifact>`
4. **Build the HTML prototype** with proper states (default, loading, error)
5. **Wrap in `<artifact>` tag** referencing the output file

## Side Files

- `references/checklist.md` — P0/P1 acceptance criteria

## Output

A single standalone HTML file implementing the login screen with:
- Labels above inputs (never placeholder-only)
- Password field with show/hide toggle
- Social SSO buttons with SVG icons
- Error states below fields
- Loading spinner in primary CTA
- Touch targets minimum 44px

## Mobile-First Constraints

- Viewport: 375px wide (iPhone standard)
- No horizontal scroll
- Safe area insets for notched devices
- Input keyboards: `tel` for phone, `password` for password fields

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
