pkgbase=orbitshell-git
pkgname=('orbitd-git' 'orbitctl-git' 'orbit-module-wallpaper-git' 'orbit-module-bar-git')
pkgver=0.1.0
pkgrel=0.1
pkgdesc='Orbit shell daemon and CLI'
arch=('x86_64' 'aarch64')
url='https://github.com/Magyx/orbit_shell'
license=('GPL3')

makedepends=('git' 'cargo' 'rust' 'pkgconf' 'wayland' 'libxkbcommon')

pkgdesc_orbitd_git='Orbit shell daemon'
depends_orbitd_git=('wayland' 'libxkbcommon' 'vulkan-icd-loader' 'dbus')
provides_orbitd_git=('orbitd')

pkgdesc_orbitctl_git='Orbit shell command-line client'
depends_orbitctl_git=('orbitd-git' 'dbus')
provides_orbitctl_git=('orbit')

pkgdesc_orbit_module_wallpaper_git='Orbit module: wallpaper'
depends_orbit_module_wallpaper_git=('orbitd-git')
provides_orbit_module_wallpaper_git=('orbit-module-wallpaper')

pkgdesc_orbit_module_bar_git='Orbit module: bar'
depends_orbit_module_bar_git=('orbitd-git')
provides_orbit_module_bar_git=('orbit-module-bar')

prepare() {
  cd "$srcdir"
  if [[ ! -d orbitshell ]]; then
    git clone "$url" orbitshell
  fi
  cd orbitshell
  git submodule update --init --recursive
}

build() {
  cd "$srcdir/orbitshell"
  cargo build --workspace --release
}

package_orbitd-git() {
  cd "$srcdir/orbitshell"

  install -Dm755 "target/release/orbitd" "$pkgdir/usr/bin/orbitd"

  if [[ -f LICENSE ]]; then
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  fi
}

package_orbitctl-git() {
  cd "$srcdir/orbitshell"

  install -Dm755 "target/release/orbit" "$pkgdir/usr/bin/orbit"

  if [[ -f LICENSE ]]; then
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  fi
}

package_orbit-module-wallpaper-git() {
  cd "$srcdir/orbitshell"
  install -d -m 755 "$pkgdir/usr/lib/orbit/modules"
  install -Dm644 "target/release/libwallpaper.so" \
    "$pkgdir/usr/lib/orbit/modules/wallpaper.so"
}

package_orbit-module-bar-git() {
  cd "$srcdir/orbitshell"
  install -d -m 755 "$pkgdir/usr/lib/orbit/modules"
  install -Dm644 "target/release/libbar.so" \
    "$pkgdir/usr/lib/orbit/modules/bar.so"
}
