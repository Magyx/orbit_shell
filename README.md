# Orbit Shell

A modular Wayland shell/daemon (`orbitd`) with a tiny CLI (`orbit`) and loadable modules (e.g., `wallpaper`, `bar`). Modules are built as shared libraries and discovered from your config directory.

## Requirements

- Linux on Wayland with **wlr-layer-shell** and **xdg-shell** available (Orbit binds both via Smithay Client Toolkit).
- **Vulkan** runtime/driver (the UI library is built with the `vulkan` feature).
- A **D-Bus** session bus (Orbit exports `io.github.orbitshell.Orbit1`).
- Rust + Cargo (for building).

## Install

### Option A — Cargo (dev-friendly)

The repo defines handy Cargo aliases:

```bash
# builds the workspace, copies modules into your config dir, and runs orbitd
cargo xrund
```

Aliases live in `.cargo/config.toml` (`xrun`, `xrund`, `xtask`). The `xtask` helper builds the whole workspace (default `--profile release`) and copies `lib{module}.so` into `~/.config/orbit/modules/` (or `$XDG_CONFIG_HOME/orbit/modules`).

### Option B — Arch Linux (PKGBUILD)

A `PKGBUILD` is included. From that folder:

```bash
makepkg -si
```

> This uses your system toolchain and package manager conventions to build and install the project.

## Configure

On first run, Orbit creates its config directory and a minimal config file:

- Config dir: `$XDG_CONFIG_HOME/orbit` (falls back to `~/.config/orbit`)
- Modules dir: `<config-dir>/modules`
- Config file: `<config-dir>/config.yaml` (initialized with `modules: {}`)

### 1) Enable modules

Modules are **disabled by default** until you explicitly turn them on in `config.yaml`:

```yaml
modules:
  wallpaper: true
  bar: true
```

Orbit only loads modules whose boolean is `true`; missing entries default to `false`.

### 2) (Optional) Per-module config

When a module is enabled, Orbit writes/merges default config for that module into `config.yaml`.

- **wallpaper** (defaults): pictures folder `Wallpapers` under your XDG Pictures, cycle `1h`, and no widgets. Example:

  ```yaml
  wallpaper:
    source: /home/you/Pictures/Wallpapers
    cycle: "1h"
    widgets:
      - type: clock
        x: 0.9
        y: 0.1
        font_size: 48
        time_format: "%H:%M"
  ```

- **bar** (defaults): `height: 32`, `time_format: "%H:%M:%S"`.

Config changes are auto-detected (a file watcher is running). If needed, you can also trigger a manual reload (see CLI).

## Run

Start the daemon:

```bash
# dev-friendly (builds, copies modules, runs orbitd)
cargo xrund
# PKGBUILD
orbitd
```

You can also run the already-built binary directly if you prefer.

## CLI

Use the `orbit` CLI to control a running daemon via D-Bus:

```bash
orbit modules        # list loaded modules
orbit toggle bar     # toggle a module on/off
orbit reload         # re-discover modules and re-apply config
orbit exit           # stop the daemon
```

If `orbit` prints “Orbit is not running.”, start `orbitd` first. (Service/interface/path are `io.github.orbitshell.Orbit1`.)

> From the workspace, you can also run via the alias:
>
> ```bash
> cargo xrun -- -- modules
> ```
>
> (the double `--` passes arguments through the `xtask` shim to `orbit`).

## Notes

- Modules are discovered as `*.so` files in `<config-dir>/modules`. The build helper copies them there for you.
- Enabling/disabling happens under the top-level `modules:` map in `config.yaml` (true/false per module).
