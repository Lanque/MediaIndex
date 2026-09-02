---
name: MediaIndex
description: A compact archive workbench for indexing and searching local footage on Windows.
colors:
  accession-vermilion: "#c7472f"
  accession-vermilion-deep: "#a93623"
  archive-canvas: "#e9e6de"
  proof-paper: "#f8f6f0"
  raised-paper: "#fffdf8"
  graphite-rail: "#272824"
  graphite-raised: "#30312c"
  ledger-ink: "#24231f"
  ledger-muted: "#74726a"
  rule-line: "#cfcbc0"
  rule-line-strong: "#a9a59a"
typography:
  headline:
    fontFamily: "Segoe UI Variable Text, Segoe UI, sans-serif"
    fontSize: "21px"
    fontWeight: 680
    lineHeight: 1.1
    letterSpacing: "-0.025em"
  body:
    fontFamily: "Segoe UI Variable Text, Segoe UI, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.42
  label:
    fontFamily: "Segoe UI Variable Text, Segoe UI, sans-serif"
    fontSize: "10px"
    fontWeight: 700
    lineHeight: 1.42
    letterSpacing: "0.1em"
  data:
    fontFamily: "Cascadia Mono, SFMono-Regular, Consolas, monospace"
    fontSize: "9px"
    fontWeight: 400
    lineHeight: 1.5
rounded:
  badge: "2px"
  control: "3px"
  dialog: "4px"
spacing:
  tight: "6px"
  control: "8px"
  group: "14px"
  panel: "22px"
components:
  button-primary:
    backgroundColor: "{colors.accession-vermilion}"
    textColor: "{colors.raised-paper}"
    rounded: "{rounded.control}"
    padding: "5px 11px"
    height: "31px"
  button-secondary:
    backgroundColor: "{colors.raised-paper}"
    textColor: "{colors.ledger-ink}"
    rounded: "{rounded.control}"
    padding: "5px 11px"
    height: "31px"
  field:
    backgroundColor: "{colors.raised-paper}"
    textColor: "{colors.ledger-ink}"
    rounded: "{rounded.badge}"
    padding: "5px 8px"
    height: "32px"
---

# Design System: MediaIndex

## Overview

**Creative North Star: "The Archive Accession Desk"**

MediaIndex should feel like a well-kept footage intake desk: practical graphite tools sit beside warm proof paper, every clip has a place, and timecode-sized details remain legible without becoming decoration. The interface is a Windows desktop workbench delivered through Tauri, not a marketing site or a browser dashboard.

The visual system favors documentary clarity over spectacle. Source folders establish the hierarchy, contact sheets provide visual scanning, and one restrained vermilion marks consequential actions. Cyan glows, decorative gradients, oversized metrics, and floating card stacks are outside this world.

**Key Characteristics:**

- Dense but calm desktop composition
- Warm paper grounds against a low-glare graphite rail
- Folder-first organization with numbered footage records
- Vermilion reserved for primary actions and active emphasis
- Monospaced numerals only for timecode, paths, sizes, and technical metadata

## Colors

The palette joins warm archival neutrals to a functional dark rail, with one red-orange accession stamp as the action color.

### Primary

- **Accession Vermilion:** The scarce action color for primary buttons, selection, and the MediaIndex mark.
- **Accession Vermilion Deep:** Hover and pressed emphasis where the primary action needs a darker state.

### Neutral

- **Archive Canvas:** The main workspace field behind folder groups.
- **Proof Paper:** The default surface for grouped footage.
- **Raised Paper:** Clip bodies, dialogs, and high-contrast fields.
- **Graphite Rail:** The persistent tools and settings column.
- **Ledger Ink:** Primary content text.
- **Ledger Muted:** Secondary descriptions and path context.
- **Rule Line / Rule Line Strong:** Hairline separation and structural boundaries.

### Named Rules

**The One Stamp Rule.** Vermilion identifies primary action or current emphasis; it never becomes a decorative wash.

**The Warm Ground Rule.** Main content stays on warm neutral paper. Blue-black and cyan belong neither to the ground nor to elevation.

## Typography

**Display Font:** None; this is an operating surface, not a display-led page.
**Body Font:** Segoe UI Variable Text (with Segoe UI and sans-serif fallbacks)
**Label/Mono Font:** Cascadia Mono (with SFMono-Regular and Consolas fallbacks)

**Character:** Windows-native UI text carries commands and descriptions. Compact mono text is reserved for measurements and file identity, where fixed-width numerals improve scanning.

### Hierarchy

- **Headline** (680, 21px, 1.1): Workspace title only.
- **Title** (680–720, 11–14px, 1.35): Folder, clip, model, and dialog titles.
- **Body** (400, 13px, 1.42): Operational copy; explanatory passages stay within 65–70 characters when space allows.
- **Label** (700, 10px, 0.1em, uppercase): Rail sections and compact state labels, never a decorative eyebrow above page headings.
- **Data** (400, 9–10px, tabular numerals): Paths, timestamps, file size, pricing, and media metadata.

### Named Rules

**The Measurement Earns Mono Rule.** Monospace appears only when alignment, time, file identity, or numeric comparison benefits from it.

## Layout

The application uses a fixed 52px command bar, a persistent 292px tool rail, and a flexible content workspace. At narrower desktop widths the rail contracts to 270px and then 244px; the application deliberately keeps an 820px minimum because phone layout is not a product target.

Folder groups are the primary content units. Each group has a compact accession header followed by a CSS-grid contact sheet whose cards stay at editing-workstation scale. Search tools sit above the sheet as two explicit operations: technical filtering and AI visual search. Spacing uses tight 6–8px control groups, 14–16px component groups, and 19–22px panel insets.

## Elevation & Depth

The system is flat by default and uses line hierarchy first. Ambient shadows with vertical offset appear only on top-level folder groups, dialogs, and hover-lifted footage records; colored halos are not part of the system.

### Shadow Vocabulary

- **Folder Rest** (`0 5px 14px rgba(70, 66, 55, 0.08), 0 1px 3px rgba(70, 66, 55, 0.08)`): Quiet separation from the archive canvas.
- **Record Hover** (`0 8px 18px rgba(41, 38, 32, 0.15), 0 2px 5px rgba(41, 38, 32, 0.11)`): Temporary focus while browsing a contact sheet.
- **Dialog Raised** (`0 10px 24px rgba(50, 47, 39, 0.14), 0 2px 5px rgba(50, 47, 39, 0.12)`): Protected modal tasks such as model selection and clip preview.

### Named Rules

**The Rule Before Shadow Rule.** Borders and tonal layers establish structure; a shadow is added only when an element truly rises or overlaps.

## Shapes

Controls and containers use tight 2–4px corners, matching desktop tools and archival labels. Round geometry is reserved for the status dot and play control. One-pixel rules divide folders, cards, and metadata without colored side stripes.

## Components

### Buttons

- **Shape:** Compact rectangular controls with 3px corners and a 31px minimum height.
- **Primary:** Vermilion with white text, a dark red border, and restrained ambient depth.
- **Hover / Focus:** Darker vermilion on hover; a 2px blue focus ring with 2px offset for keyboard visibility.
- **Secondary:** Raised paper with ledger ink and a strong neutral rule.

### Chips

- **Style:** Small rectangular state stamps with 2px corners, uppercase 9px labels, and tonal state colors.
- **State:** Red for top/current, green for indexed/ready, amber for higher-cost or related, graphite for neutral.

### Cards / Containers

- **Corner Style:** 3px at the folder boundary; clip records meet on one-pixel internal rules.
- **Background:** Proof paper for groups and raised paper for clip bodies.
- **Shadow Strategy:** Flat inside the sheet; ambient lift only at group level or hovered record.
- **Border:** One-pixel warm gray rules.
- **Internal Padding:** 7–11px for dense operational content.

### Inputs / Fields

- **Style:** Warm raised paper in the content area and near-black graphite in the rail, each with a 1px border and 2px corners.
- **Focus:** Higher-contrast border plus the shared keyboard focus ring.
- **Error / Disabled:** Disabled controls reduce opacity; errors use direct recovery copy instead of decorative warning chrome.

### Navigation

The command bar names the product, desktop platform, selected folder, and the two primary library actions. The graphite rail persists independently from the scrolling footage workspace.

### Folder Contact Sheet

Each source folder owns a header with its path, clip count, total size, and AI readiness. Video thumbnails and technical records remain inside that folder boundary. AI search instead groups timestamped moments beneath one video record, preventing adjacent frames from appearing as separate videos.

## Do's and Don'ts

### Do:

- **Do** organize footage by real source folder before presenting individual clips.
- **Do** keep primary actions scarce and vermilion.
- **Do** use authored SVG icons with a consistent 1.5–1.6px stroke.
- **Do** show loading, empty, disabled, warning, and keyboard-focus states in the same material system.

### Don't:

- **Don't** redesign MediaIndex as a web landing page, analytics dashboard, or mobile-first surface.
- **Don't** reintroduce cyan glows, gradient text, decorative glass, or nested rounded-card stacks.
- **Don't** use emoji or Unicode arrows as interface icons.
- **Don't** duplicate adjacent AI timestamps as separate top-level results for the same video.
