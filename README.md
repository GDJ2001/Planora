<div align="center">

# Planora

### Your PC, clearly.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/logos/planora-horizontal-dark.svg" />
  <img alt="Planora" src="assets/brand/logos/planora-horizontal-light.svg" width="550" />
</picture>

See how processes use your computer’s resources.

</div>

Planora is a planned desktop resource monitor for **Windows, Linux, and macOS**. The future application will be written in **Rust**. This repository currently provides the brand foundation; monitoring features and the application itself are not implemented.

## Explore the brand kit

- [Interactive brand showcase](assets/brand/showcase.html) — download or open locally in a browser; self-contained, with dark/light/system themes and illustrative data.
- [Brand guidelines and Rust handoff](docs/brand-guidelines.md) — identity, colors, typography, states, usage, and rebuild instructions.
- [Design tokens](assets/design-tokens.json) and [generated CSS](assets/design-tokens.css) — primitive, semantic, and component layers.
- [Vector logos](assets/brand/logos/) and [transparent PNGs](assets/brand/png/).
- [Windows ICO](assets/brand/platforms/windows/planora.ico), [Linux icons](assets/brand/platforms/linux/), [macOS ICNS](assets/brand/platforms/macos/Planora.icns), and [tray icons](assets/brand/platforms/tray/).
- [Interface icons](assets/brand/ui/) — 18 vector symbols on a 24-pixel grid.
- [Original reference image](assets/brand/Planora%20Blue%20Orbit%20Logo.png) — preserved unchanged.

## Asset tooling

The Node/Python tooling only builds and validates brand assets; it does not select a Rust GUI framework or introduce an application runtime.

```sh
npm install
python -m pip install -r scripts/requirements.txt
npm run brand:build
npm run brand:validate
```

See the [handoff guide](docs/brand-guidelines.md#rebuilding-and-validation) for optional browser checks and the distinction between asset validation and native platform verification.
