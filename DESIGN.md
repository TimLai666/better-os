---
version: alpha
name: Better Manager
description: A compact, light-first desktop manager for reviewing and safely applying Better OS component changes.
colors:
  primary: "#1f2937"
  secondary: "#667085"
  tertiary: "#4f6df5"
  neutral: "#f4f7fb"
typography:
  h1:
    fontFamily: system-ui
    fontSize: 24px
    fontWeight: 700
    lineHeight: 1.2
  h2:
    fontFamily: system-ui
    fontSize: 18px
    fontWeight: 600
    lineHeight: 1.3
  body-md:
    fontFamily: system-ui
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.5
rounded:
  sm: 4px
  md: 8px
  lg: 12px
  full: 9999px
spacing:
  xs: 4px
  sm: 8px
  md: 16px
  lg: 32px
components:
  button-primary:
    backgroundColor: "{colors.tertiary}"
    textColor: "#ffffff"
    rounded: "{rounded.md}"
  card:
    backgroundColor: "#ffffff"
    rounded: "{rounded.lg}"
    padding: 16px
---

## Overview

Better Manager is a native utility for making component lifecycle decisions visible before any privileged operation exists. The visual motif is a reviewed change set: calm surfaces, explicit status tags, and a clear path from plan to verification.

## Colors

- **Primary:** `#1f2937` for readable headings and action text.
- **Secondary:** `#667085` for metadata and explanations.
- **Tertiary:** `#4f6df5` for the single primary action and selected state.
- **Neutral:** `#f4f7fb` for the light application canvas.

The light theme is the default and the only committed theme in this slice. Dark theme support remains a later decision from Issue #8.

## Typography

Use the platform UI font supplied by GPUI. Hierarchy comes from size, weight, and muted metadata rather than oversized display text.

## Layout

Use a compact sidebar with a flexible content column. Cards and action rows use content-driven widths, `min_w_0`, wrapping, and stacked layouts at compact widths. Descriptions wrap; only non-critical identifiers may truncate.

## Elevation & Depth

Prefer tonal separation and a 1px border over shadows. Keep surfaces white against the soft gray canvas.

## Shapes

Use small-to-medium radii for utility surfaces and full pills only for semantic status tags.

## Components

Primary buttons use one clear verb such as `Update All` or `Install updates`. Review screens show the affected components, dependencies, touched files, and restore availability before approval. Loading, failure, restore, empty, and manual-recovery states are part of the same flow.

## Do's and Don'ts

- Do keep lifecycle wording user-facing: `Review changes`, `Install updates`, `Applying settings`, `Checking that everything works`, and `Restore previous version`.
- Do localize all UI copy at runtime for `zh-TW`, `en-US`, and `system` fallback.
- Don't use fixed card or button widths that can clip translated content.
- Don't introduce privileged commands, real package mutation, or a dark-theme inversion in this UI slice.
