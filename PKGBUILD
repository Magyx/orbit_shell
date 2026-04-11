pkgbase=orbitshell-git
pkgname=(
  'orbit-git'
  'orbit-module-wallpaper-git'
  'orbit-module-bar-git'
  'orbit-module-cliphistory-git'
  'orbit-module-launcher-git'
)
pkgver=git
pkgrel=1
pkgdesc='Orbit shell daemon and CLI'
arch=('x86_64' 'aarch64')
url='https://github.com/Magyx/orbit_shell'
license=('GPL-3.0-or-later')
makedepends=(
  'cargo'
  'git'
  'libxkbcommon'
  'pkgconf'
  'rust'
  'wayland'
)
source=('orbitshell::git+https://github.com/Magyx/orbit_shell.git')
b2sums=('SKIP')

pkgver() {
  cd "$srcdir/orbitshell"

  local _ver _rev _hash
  _ver=$(grep -oEm1 'version = "[^"]+"' src/orbitd/Cargo.toml | cut -d'"' -f2)
  _rev=$(git rev-list --count HEAD)
  _hash=$(git rev-parse --short HEAD)

  printf '%s.r%s.g%s\n' "${_ver:-0.1.0}" "$_rev" "$_hash"
}

build() {
  cd "$srcdir/orbitshell"

  cargo build --release --locked \
    -p orbitd \
    -p orbit \
    -p wallpaper \
    -p bar \
    -p cliphistory \
    -p launcher
}

package_orbit-git() {
  pkgdesc='Orbit shell daemon and CLI'
  depends=(
    'dbus'
    'libxkbcommon'
    'vulkan-icd-loader'
    'wayland'
  )
  optdepends=(
    'vulkan-intel: Intel Vulkan driver'
    'vulkan-radeon: AMD Radeon Vulkan driver'
    'nvidia-utils: NVIDIA Vulkan driver'
  )
  provides=(
    "orbit=${pkgver}"
    "orbitd=${pkgver}"
  )
  conflicts=('orbit' 'orbitd')

  cd "$srcdir/orbitshell"

  install -Dm755 target/release/orbitd "$pkgdir/usr/bin/orbitd"
  install -Dm755 target/release/orbit "$pkgdir/usr/bin/orbit"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}

package_orbit-module-wallpaper-git() {
  pkgdesc='Orbit module: wallpaper'
  depends=('orbit-git')
  provides=("orbit-module-wallpaper=${pkgver}")
  conflicts=('orbit-module-wallpaper')

  cd "$srcdir/orbitshell"

  install -Dm644 target/release/libwallpaper.so \
    "$pkgdir/usr/lib/orbit/modules/wallpaper.so"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}

package_orbit-module-bar-git() {
  pkgdesc='Orbit module: bar'
  depends=('orbit-git')
  provides=("orbit-module-bar=${pkgver}")
  conflicts=('orbit-module-bar')

  cd "$srcdir/orbitshell"

  install -Dm644 target/release/libbar.so \
    "$pkgdir/usr/lib/orbit/modules/bar.so"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}

package_orbit-module-cliphistory-git() {
  pkgdesc='Orbit module: cliphistory'
  depends=('orbit-git')
  provides=("orbit-module-cliphistory=${pkgver}")
  conflicts=('orbit-module-cliphistory')

  cd "$srcdir/orbitshell"

  install -Dm644 target/release/libcliphistory.so \
    "$pkgdir/usr/lib/orbit/modules/cliphistory.so"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}

package_orbit-module-launcher-git() {
  pkgdesc='Orbit module: launcher'
  depends=('orbit-git')
  provides=("orbit-module-launcher=${pkgver}")
  conflicts=('orbit-module-launcher')

  cd "$srcdir/orbitshell"

  install -Dm644 target/release/liblauncher.so \
    "$pkgdir/usr/lib/orbit/modules/launcher.so"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
