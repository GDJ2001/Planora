# Planora brand guidelines

Version 1.0 · 18 October 2026 · English brand foundation

## Identity and positioning

**Planora** is a planned desktop resource monitor: a system task manager that helps people understand process-level CPU, memory, GPU, disk, and network use. Its audience includes everyday PC users and technical users who need readable measurements.

- **Working tagline:** Your PC, clearly.
- **Short description:** See how processes use your computer’s resources.
- **Voice:** direct, calm, specific. Name the resource and explain the state. Prefer “Memory usage” to “Memory pressure detected” unless pressure was actually measured.
- **Claims:** do not claim improved speed, reduced resource consumption, privacy guarantees, or platform functionality before implementation and verification.

The [showcase](../assets/brand/showcase.html) is a visual specification with fictional sample data. Its numbers, rows, and screen arrangement do not define final application requirements. Rust development, GUI framework selection, resource collection, feature behavior, and packaging integration belong to the later development brief.

## Logo family

The reference is [Planora Blue Orbit Logo.png](../assets/brand/Planora%20Blue%20Orbit%20Logo.png). It remains unchanged; SHA-256: `31e1e20534bc8715bb782f8f211db924980b94f09220d4e3c123c7b993f0ab49`.

The vector adaptation preserves the blue orbit, open route, cyan center, and navy name. The lettering is custom outlined geometry inspired by the reference, not a font-identical reconstruction. No installed font or external image is required to display the SVG wordmark. All paths remain editable.

| Family | Filename in `assets/brand/logos/` | Use |
| --- | --- | --- |
| Horizontal | `planora-horizontal-light.svg`, `planora-horizontal-dark.svg` | Header, documentation, brand presentations |
| Stacked | `planora-stacked-light.svg`, `planora-stacked-dark.svg` | Compact brand placement |
| Wordmark | `planora-wordmark-light.svg`, `planora-wordmark-dark.svg` | Name when a separate symbol is present |
| Full-color mark | `planora-mark.svg` | Large standalone symbol |
| Monochrome lockup | `planora-monochrome-navy.svg`, `planora-monochrome-white.svg` | Single-color reproduction |
| Monochrome symbol | `planora-mark-navy.svg`, `planora-mark-white.svg` | Single-color standalone mark |

Transparent PNG equivalents of all 11 variants are in [the PNG directory](../assets/brand/png/). Horizontal and monochrome lockups are 2400 pixels wide; other logo exports are 1200 pixels wide. Prefer SVG for scaling.

### Placement and fidelity

- Leave external clear space of at least 25% of the symbol height. For wordmark-only use, allow half the capital P height. Canvas padding does not replace external clear space.
- Minimum widths: horizontal logo 180 logical pixels; stacked logo 120; wordmark 100.
- Full-detail marks require at least 40 physical pixels. Use `masters/planora-small-mark.svg` at 16–32 pixels. Small exports have a thicker solid silhouette.
- Logo suffixes identify the destination theme: `*-light` on white/pale surfaces, `*-dark` on dark neutrals. Tray suffixes identify ink: `tray-light` is white; `tray-dark` is dark.
- Brand gradients are intentional. Use the supplied gradients or monochrome variants; do not add glow, shadows, rotations, stretched proportions, or new gradients.
- Never crop the orbit tail or fill the open route. Never use the wordmark as a 16-pixel icon. Keep the original reference separate from derived exports.

## Platform icons

Paths below are relative to `assets/brand/`.

| Platform | Deliverables |
| --- | --- |
| Windows | `platforms/windows/planora.ico`: 16, 20, 24, 32, 40, 48, 64, 128, 256-pixel RGBA frames; individual PNGs also supplied |
| Linux | `platforms/linux/planora.svg`; PNGs at 16, 24, 32, 48, 64, 128, 256, 512 pixels |
| macOS | `platforms/macos/Planora.icns`; `Planora.iconset` at 16, 32, 128, 256, 512 pixels in 1× and 2×, through 1024 pixels |
| Tray/menu bar | Light/dark SVG and PNG at 16, 20, 22, 24, 32, 44, 48, 64 pixels |

The editable full icon composition and macOS tile are in [masters](../assets/brand/masters/). macOS uses a dark rounded tile with transparent outer padding; Windows/Linux use transparent freestanding marks. These are conventional exports, not a layered Icon Composer project or store submission package.

Select the correct scale-specific export and use native template-image mode for monochrome menu bar icons where supported. Do not bake a background behind tray symbols. Linux install paths, Windows store/package names, macOS asset-catalog wiring, and executable identifiers will be defined during application packaging.

References: [Microsoft app icons](https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icons), [Apple app icons](https://developer.apple.com/design/human-interface-guidelines/app-icons/).

## Colors

### Brand palette

| Name | Hex | Purpose |
| --- | --- | --- |
| Midnight navy | `#05255D` | Light-surface wordmark and single-color logo |
| Deep blue | `#061947` | Logo depth |
| Electric blue | `#087EFF` | Recognizable brand accent |
| Sky cyan | `#0A9EF6` | Supporting gradient |
| Teal | `#08E5C6` | Center and tail highlights |

The primary UI blue is darker than the logo blue so white button labels remain readable. Do not use cyan or teal for white-text buttons. Gradients belong to branding; controls and chart traces use solids.

### Application roles

| Role | Dark | Light |
| --- | --- | --- |
| Window | `#0B1220` | `#F4F7FB` |
| Panel | `#111C2E` | `#FFFFFF` |
| Raised panel | `#19263B` | `#FFFFFF` |
| Primary text | `#E6EDF7` | `#14213D` |
| Secondary text | `#A7B4C8` | `#52627A` |
| Primary action | `#2563EB` | `#2563EB` |
| Action label | `#FFFFFF` | `#FFFFFF` |
| Links/focus | `#60A5FA` | `#2563EB` |
| Decorative divider | `#23324B` | `#CBD5E1` |
| Essential control border | `#64748B` | `#64748B` |
| Success | `#34D399` | `#047857` |
| Warning | `#FBBF24` | `#B45309` |
| Error | `#F87171` | `#B91C1C` |

Subtle dividers are decorative grouping aids. Essential control boundaries use the stronger control-border token.

### Resource identity

| Resource | Dark | Light |
| --- | --- | --- |
| CPU | `#60A5FA` | `#2563EB` |
| Memory | `#22D3EE` | `#0E7490` |
| GPU | `#A78BFA` | `#7C3AED` |
| Disk | `#FBBF24` | `#B45309` |
| Network | `#F472B6` | `#BE185D` |

Use the same mapping in cards, charts, table summaries, and legends. Resource colors are categorical: a yellow disk trace does not indicate a warning. Status indicators require explicit labels and symbols. Do not invent alert thresholds in the branding layer.

Label charts with resource, unit, scale, and time window. Use solid/dashed lines for overlapping series and textual summaries for nonvisual access. Gridlines may be subtle; essential traces must contrast with the plot. Formatting must keep units explicit; final measurement semantics belong to product requirements.

## Typography, layout, and components

Use native system UI typography: `system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`. Typical native choices are Segoe UI on Windows, San Francisco on macOS, and the configured UI font on Linux. Do not bundle proprietary system fonts. In Rust, resolve the system font through the chosen toolkit; do not treat the CSS fallback string as one font name.

| Role | Logical size | Weight | Line height |
| --- | --- | --- | --- |
| Secondary/caption | 12 | 400 | 1.5 |
| Body | 14 | 400 | 1.5 |
| Control label | 14 | 500–600 | 1.5 |
| Section heading | 20 | 600 | 1.2 |
| Page title | 32 | 600 | 1.2 |

Enable tabular numerals for measurements and align numeric columns. Use native monospace for PIDs, paths, addresses, and technical identifiers. Font substitutions may change text width; support scaling and avoid clipped labels.

- Spacing grid: 4 px; common gaps 8, 12, 16, 24, 32 px.
- Controls: 36 px high, 6 px radius. Process rows: 40 px baseline minimum.
- Panels: 8 px radius, 24 px default padding. The outer preview frame uses 12 px.
- UI icons: 24 × 24 viewBox, 1.75 px stroke, round caps/joins, `currentColor`.
- Symbols: overview, processes, CPU, memory, GPU, disk, network, users, settings, search, refresh, pause, three chevrons, check, warning, error.
- Label unfamiliar icons and give icon-only controls accessible names. Hide decorative icons from assistive technology.

### Interaction states

| State | Treatment |
| --- | --- |
| Primary default | `#2563EB`, white label |
| Primary hover | `#1D4ED8`, white label |
| Primary pressed | `#1E40AF`, white label |
| Neutral hover | Dark `#19263B`; light `#E9EFF7` |
| Neutral pressed | Dark `#23324B`; light `#CBD5E1` |
| Selected | Dark `#23324B` / `#93C5FD` text; light `#E9EFF7` / `#1E40AF` text; add edge indicator or checkmark |
| Disabled | Dark `#23324B` / `#A7B4C8` text; light `#E9EFF7` / `#52627A` text; disabled semantics and no action |
| Focus | 2 px theme focus outline, 3 px offset outside control |
| Feedback | Label plus check, warning, or error icon in semantic color |

The focus offset separates the ring from blue button fill. Avoid clipping rings. Selection, availability, and errors must not rely on color alone.

### Theme and motion

The future app defaults to **System**, with **Light** and **Dark** overrides. Follow OS changes only when System is selected, preserving resource identity and user state. The showcase starts in Dark to present the dark-led direction, offers all three choices, and does not persist preferences.

Use 120 ms color transitions, at most 180 ms for ordinary motion. Under reduced-motion preference, disable nonessential animation/transitions. Avoid animating measurement text or pulsing charts. Monitoring refresh rates are not prescribed here.

## Tokens and Rust handoff

[design-tokens.json](../assets/design-tokens.json) is the source of truth. The [schema](../assets/design-tokens.schema.json) describes its top-level shape; the validator additionally checks reference resolution, theme parity, and contrast. This is a project-specific format, not a claim of DTCG compatibility.

1. **Primitive:** raw colors, spacing, fonts, radii, and durations.
2. **Semantic:** purpose aliases in complete `dark` and `light` maps.
3. **Component:** button, input, panel, table, chart, focus, icon, and motion defaults.

References are entire strings such as `{primitive.color.blue.600}` or `{semantic.surface}`. Resolve semantics against the selected theme. Reject unknown references and cycles. No interpolation or arithmetic is supported. Both themes expose identical keys.

Colors are opaque gamma-encoded sRGB `#RRGGBB`. Parse to 8-bit channels, normalizing by 255 only if the toolkit requires it. Convert to linear color only when required by the renderer. Avoid double gamma conversion and double scaling. Dimensions are device-independent logical pixels; durations are milliseconds; line height is a multiplier; font weights are numeric. Platform/toolkit scaling is separate. Choose SVG or appropriately scaled raster assets.

[design-tokens.css](../assets/design-tokens.css) is generated convenience output with `--p-*`, `--s-*`, and `--c-*` variables. Semantic/component values resolve under `[data-theme="dark"]` and `[data-theme="light"]`. This does not require a web-based Rust UI. The showcase embeds the CSS and all graphics for offline use.

No Rust types, application APIs, GUI framework, or monitoring runtime are introduced. The token document is the portable handoff interface.

## Rebuilding and validation

Use Node.js 20+ and Python 3.10+. Install asset-only dependencies:

```sh
npm install
python -m pip install -r scripts/requirements.txt
npm run brand:build
npm run brand:validate
```

Source ownership:

- Edit `masters/planora-mark.svg` and `masters/planora-wordmark.svg` for base vector artwork.
- Edit `scripts/build-brand.cjs` for compositions, small-mark geometry, UI icons, and export sizing. Other master SVGs are generated editable outputs; mirror edits in the generator to preserve them across builds.
- Edit `assets/design-tokens.json` for theme values.
- Edit `scripts/showcase.template.html` and `scripts/build-showcase.cjs` for the showcase, not generated HTML/CSS.
- `scripts/export-icons.py` packages PNGs into ICO/ICNS without resampling; Pillow validates decoding and alpha.

Optional browser checks:

```sh
npm install --no-save --package-lock=false playwright
npx playwright install chromium
node scripts/check-showcase.cjs
```

Alternatively set `PLANORA_BROWSER` to an installed Chromium-family executable. Checks run headlessly and cover offline images, themes/OS changes, sample filtering/empty state, copy feedback, focus, reduced motion, and a 390-pixel viewport. Screenshots and contrast evidence are written to ignored `state/`.

Asset checks cover the original hash, SVG self-containment, PNG dimensions/transparency, ICO sizes, ICNS decoding, token resolution, theme key parity, CSS freshness, and documentation links. Browser checks additionally inspect rendering and interaction.

Accessibility targets follow [WCAG 2.2](https://www.w3.org/TR/WCAG22/): at least 4.5:1 for ordinary text and 3:1 for essential graphics against adjacent backgrounds. The primary button pairing is 5.17:1. Logo artwork is not body text; decorative dividers are not essential control boundaries. Token checks do not certify a future app or replace assistive-technology testing.

### Verification boundary

File and browser checks run on the current Windows machine. ICO/ICNS decoding does not establish correct Windows shell, Linux desktop, or macOS Dock/menu bar integration. Native installation, scaling, high-contrast behavior, screen-reader navigation, and real resource readings remain application-stage verification.

The pre-existing deletion of `assets/brand/README.md` is retained. This guide supersedes its obsolete references and task-planning positioning.
