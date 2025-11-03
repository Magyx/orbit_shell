pkgbase=orbitshell-git
pkgname=('orbitd-git' 'orbitctl-git')
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
depends_orbitctl_git=('dbus')
provides_orbitctl_git=('orbitctl' 'orbit')

source=("orbitshell::git+file://$startdir")
sha256sums=('SKIP')

build() {
  cd "orbitshell"
  if [[ -f Cargo.lock ]]; then
    cargo build --release --locked -p orbitd -p orbit
  else
    cargo build --release -p orbitd -p orbit
  fi
}

package_orbitd-git() {
  cd "orbitshell"

  install -Dm755 "target/release/orbitd" "$pkgdir/usr/bin/orbitd"

  # D-Bus session auto-activation
  install -Dm644 /dev/stdin \
    "$pkgdir/usr/share/dbus-1/services/io.github.orbitshell.Orbit1.service" <<'EOF'
[D-BUS Service]
Name=io.github.orbitshell.Orbit1
Exec=/usr/bin/orbitd
EOF

  # Optional systemd --user unit
  install -Dm644 /dev/stdin \
    "$pkgdir/usr/lib/systemd/user/orbitd.service" <<'EOF'
[Unit]
Description=Orbit shell daemon
After=graphical-session.target

[Service]
ExecStart=/usr/bin/orbitd
Restart=on-failure

[Install]
WantedBy=default.target
EOF

  if [[ -f LICENSE ]]; then
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  fi
}

package_orbitctl-git() {
  cd "orbitshell"

  install -Dm755 "target/release/orbit" "$pkgdir/usr/bin/orbitctl"
  ln -s orbitctl "$pkgdir/usr/bin/orbit"

  if [[ -f LICENSE ]]; then
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  fi
}
