#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="${PLANORA_VERSION:-0.1.2}"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
install -Dm755 "$root/target/release/planora" "$stage/usr/bin/planora"
install -Dm755 "$root/target/release/planora-helper" "$stage/usr/libexec/planora/planora-helper"
install -Dm755 "$root/packaging/linux/authorize-user" "$stage/usr/libexec/planora/authorize-user"
install -Dm644 "$root/packaging/linux/planora-helper.service" "$stage/usr/lib/systemd/system/planora-helper.service"
install -Dm644 "$root/packaging/linux/planora.sysusers" "$stage/usr/lib/sysusers.d/planora.conf"
install -Dm644 "$root/packaging/linux/org.planora.authorize.policy" "$stage/usr/share/polkit-1/actions/org.planora.authorize.policy"
install -Dm644 "$root/packaging/linux/planora.desktop" "$stage/usr/share/applications/planora.desktop"
for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 "$root/assets/brand/platforms/linux/planora-$size.png" "$stage/usr/share/icons/hicolor/${size}x${size}/apps/planora.png"
done
install -Dm644 "$root/assets/brand/platforms/linux/planora.svg" "$stage/usr/share/icons/hicolor/scalable/apps/planora.svg"
mkdir -p "$stage/DEBIAN" "$root/dist"
cat > "$stage/DEBIAN/control" <<EOF
Package: planora
Version: $version
Architecture: amd64
Maintainer: Planora <maintainers@invalid.example>
Depends: libc6, libgcc-s1, libx11-6, libwayland-client0, libpolkit-gobject-1-0, pkexec, systemd
Section: utils
Priority: optional
Description: Native hardware visualizer and resource monitor
 Planora shows process and system resources with an authorized local helper.
EOF
cat > "$stage/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
systemd-sysusers /usr/lib/sysusers.d/planora.conf
systemctl daemon-reload
systemctl enable --now planora-helper.service
EOF
cat > "$stage/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = remove ]; then systemctl disable --now planora-helper.service || true; fi
EOF
cat > "$stage/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
systemctl daemon-reload || true
if [ "$1" = purge ]; then
  userdel planora 2>/dev/null || true
  groupdel planora-users 2>/dev/null || true
fi
EOF
chmod 755 "$stage/DEBIAN/"{postinst,prerm,postrm}
dpkg-deb --build --root-owner-group "$stage" "$root/dist/planora_${version}_amd64.deb"
