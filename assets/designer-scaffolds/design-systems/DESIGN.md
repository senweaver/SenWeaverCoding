---
version: alpha
name: Project Design System
description: Neutral starter system for a SenWeaverCoding design workspace
colors:
  background: "#F7F3EC"
  surface: "#FFFAF2"
  surfaceRaised: "#FFFFFF"
  text: "#10172B"
  muted: "#5D6680"
  border: "#DED6C8"
  accent: "#E0522D"
typography:
  display:
    fontFamily: Georgia
    fontSize: 56px
    fontWeight: 700
    lineHeight: 1.05
    letterSpacing: -0.02em
  body:
    fontFamily: DM Sans
    fontSize: 16px
    fontWeight: 400
    lineHeight: 1.55
rounded:
  sm: 4px
  md: 8px
  lg: 16px
spacing:
  xs: 4px
  sm: 8px
  md: 16px
  lg: 32px
  xl: 64px
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "#FFFFFF"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: 12px
  card:
    backgroundColor: "{colors.surfaceRaised}"
    textColor: "{colors.text}"
    rounded: "{rounded.lg}"
    padding: 24px
---

## Overview

A warm editorial system with crisp product UI structure. Use it as a starting
baton, then replace tokens with project-specific choices once the visual
direction is resolved.

## Colors

Use background and surface for page depth, text and muted for hierarchy, border
for quiet separation, and accent for primary actions or data highlights.

## Typography

Display type carries high-emphasis headings. Body type should stay practical and
readable for navigation, tables, controls, and explanatory copy.

## Layout

Use a 12-column desktop rhythm, a two-column tablet rhythm, and a single-column
mobile rhythm. Keep content max widths explicit and avoid accidental horizontal
scroll.

## Elevation & Depth

Prefer tonal separation and fine borders before large shadows. Raised surfaces
should feel attached to the system, not floating above it.

## Shapes

Use small radii for controls and medium-to-large radii for panels. Nested
surfaces should never have a larger radius than their parent.

## Components

Primary buttons are direct and high contrast. Cards hold one clear job: status,
record detail, comparison, form group, or content module.

## Do's and Don'ts

Do promote repeated visual choices back into this file. Don't paste transient
task notes, long source files, or unverified brand values here.
