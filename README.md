# Orbit Shell

A modular Wayland shell/daemon (`orbitd`) with a tiny CLI (`orbit`) and loadable modules (`wallpaper`, `bar`, `launcher`, `lockscreen`). Modules are compiled as shared libraries and hot-loaded at runtime.

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

Aliases live in `.cargo/config.toml` (`xrun`, `xrund`, `xtask`). The `xtask` helper builds the entire workspace (default `--profile release`) and copies each `lib{module}.so` into `$XDG_CONFIG_HOME/orbit/modules/` (falling back to `~/.config/orbit/modules/`).

### Option B — Arch Linux (PKGBUILD)

A `PKGBUILD` is included. From that folder:

```bash
makepkg -si
```

This uses your system toolchain and installs via `pacman`.

## Configure

On first run, Orbit creates its config directory and a minimal config file:

- Config dir: `$XDG_CONFIG_HOME/orbit` (falls back to `~/.config/orbit`)
- Modules dir: `<config-dir>/modules`
- Config file: `<config-dir>/config.yaml`

### Enabling modules

Modules are **disabled by default**. Turn them on in `config.yaml` under the top-level `modules:` map:

```yaml
modules:
  wallpaper: true
  bar: true
  launcher: true
  lockscreen: true
```

Any module whose value is `false` or absent is not loaded.

### Per-module config

When a module is enabled, Orbit merges your config on top of the module's built-in defaults. Unknown keys are ignored; omitting a key keeps its default value.

#### `wallpaper`

Displays a wallpaper on all outputs, cycling through images on a timer.

```yaml
wallpaper:
  source: /home/you/Pictures/Wallpapers  # directory or single file (jpg/png)
  cycle: "1h"                            # humantime duration, e.g. "30m", "2h"
  widgets:
    - type: clock
      x: 0.9        # fractional position (0.0 = left, 1.0 = right)
      y: 0.1        # fractional position (0.0 = top, 1.0 = bottom)
      font_size: 48
      time_format: "%H:%M"
      # font_family: Monospace  # optional: Monospace, SansSerif, Serif, or a font name
```

Module commands: `orbit command wallpaper next` — skip to the next wallpaper immediately.

#### `bar`

A slim status bar anchored to the top of all outputs.

```yaml
bar:
  height: 32           # bar height in pixels (must be ≥ 1)
  time_format: "%H:%M:%S"  # strftime format; tick interval adapts automatically
```

#### `launcher`

A keyboard-driven application launcher. Reads `.desktop` files from your XDG data directories. Hidden by default — toggle it on with `orbit toggle launcher`.

```yaml
launcher:
  width: 600           # panel width in pixels (≥ 200)
  height: 420          # panel height in pixels (≥ 100)
  max_results: 8       # maximum number of search results shown
  icon_size: 32        # icon size in pixels (8–256)
  position: center     # "top", "center", or "bottom"
  launch_options: ""   # extra arguments prepended to every launch command
```

Module commands: `orbit command launcher refresh` — re-scan desktop files.

#### `lockscreen`

A full-screen lock screen using PAM authentication. Hidden by default — activate it with `orbit toggle lockscreen`.

```yaml
lockscreen:
  message: "Welcome {username}!"  # {username} is replaced at runtime
```

### Hot-reload

Config changes are detected automatically via a file watcher. You can also force a manual reload:

```bash
orbit reload
```

## Module discovery

On startup (and on reload), Orbit scans for `.so` files in two locations, with user modules taking priority over system ones:

1. `/usr/lib/orbit/modules/` — system-installed modules
2. `$XDG_CONFIG_HOME/orbit/modules/` — user modules (installed by `xtask`)

Only modules listed under `modules:` as `true` in `config.yaml` are loaded and started.

## Run

Start the daemon:

```bash
# dev-friendly (builds, copies modules, runs orbitd)
cargo xrund

# if installed via PKGBUILD
orbitd
```

## CLI

Use the `orbit` CLI to control a running daemon via D-Bus:

```bash
orbit modules                      # list loaded modules
orbit toggle <module>              # show/hide a module
orbit commands                     # list all module commands
orbit commands <module>            # list commands for a specific module
orbit command <module> <command>   # send a command to a module
orbit reload                       # re-discover modules and re-apply config
orbit exit                         # stop the daemon
```

If `orbit` prints `"Orbit is not running."`, start `orbitd` first.

> From the workspace you can also run via the cargo alias:
>
> ```bash
> cargo xrun -- -- modules
> ```
>
> The double `--` passes arguments through the `xtask` shim to `orbit`.

## Notes

- Modules are discovered as `*.so` files. The `xtask` build helper copies them to your user modules directory automatically.
- A module's `show_on_startup` flag (set in its manifest) controls whether it appears immediately when enabled, or stays hidden until toggled.
- Config values are deep-merged with module defaults — you only need to specify the keys you want to override.
- The D-Bus service/interface/path are all `io.github.orbitshell.Orbit1`.
