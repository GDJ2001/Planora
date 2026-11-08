#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="${PLANORA_VERSION:-$(cargo pkgid --manifest-path "$root/Cargo.toml" -p planora-ui | sed 's/.*[#@]//')}"
case "$version" in
  *-beta.*) deb_version="${version/-beta./~beta.}-1" ;;
  *) deb_version="$version-1" ;;
esac
release_dir="${CARGO_TARGET_DIR:-$root/target}/release"
bash "$root/packaging/linux/build-profiler.sh"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
install -Dm755 "$release_dir/planora" "$stage/usr/bin/planora"
install -Dm755 "$release_dir/planora-helper" "$stage/usr/libexec/planora/planora-helper"
install -Dm755 "$release_dir/planora-disk-profile" "$stage/usr/libexec/planora/planora-disk-profile"
install -Dm644 "$release_dir/planora-disk-profile.bpf.o" "$stage/usr/libexec/planora/planora-disk-profile.bpf.o"
install -Dm755 "$root/packaging/linux/authorize-user" "$stage/usr/libexec/planora/authorize-user"
install -Dm755 "$root/packaging/linux/revoke-user" "$stage/usr/libexec/planora/revoke-user"
install -Dm644 "$root/packaging/linux/planora-helper.service" "$stage/usr/lib/systemd/system/planora-helper.service"
install -Dm644 "$root/packaging/linux/planora-profiler.service" "$stage/usr/lib/systemd/system/planora-profiler.service"
install -Dm644 "$root/packaging/linux/planora-pmu.service" "$stage/usr/lib/systemd/system/planora-pmu.service"
install -Dm644 "$root/packaging/linux/planora.sysusers" "$stage/usr/lib/sysusers.d/planora.conf"
install -Dm644 "$root/packaging/linux/planora-rapl.conf" "$stage/usr/lib/tmpfiles.d/planora-rapl.conf"
install -Dm644 "$root/packaging/linux/60-planora-rapl.rules" "$stage/usr/lib/udev/rules.d/60-planora-rapl.rules"
install -Dm644 "$root/packaging/linux/org.planora.authorize.policy" "$stage/usr/share/polkit-1/actions/org.planora.authorize.policy"
install -Dm644 "$root/packaging/linux/org.planora.revoke.policy" "$stage/usr/share/polkit-1/actions/org.planora.revoke.policy"
install -Dm644 "$root/packaging/linux/planora.desktop" "$stage/usr/share/applications/planora.desktop"
install -Dm644 "$root/packaging/linux/org.planora.Planora.metainfo.xml" "$stage/usr/share/metainfo/org.planora.planora.metainfo.xml"
for license_file in LICENSE LICENSE-MIT LICENSE-APACHE LICENSE-GPL-2.0 NOTICE; do
  install -Dm644 "$root/$license_file" "$stage/usr/share/doc/planora/$license_file"
done
install -Dm644 "$root/packaging/linux/planora-disk-profile.bpf.c" "$stage/usr/share/doc/planora/source/planora-disk-profile.bpf.c"
install -Dm644 "$root/packaging/linux/planora-disk-profile.c" "$stage/usr/share/doc/planora/source/planora-disk-profile.c"
for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 "$root/assets/brand/platforms/linux/planora-$size.png" "$stage/usr/share/icons/hicolor/${size}x${size}/apps/planora.png"
done
install -Dm644 "$root/assets/brand/platforms/linux/planora.svg" "$stage/usr/share/icons/hicolor/scalable/apps/planora.svg"
mkdir -p "$stage/DEBIAN" "$root/dist"
cat > "$stage/DEBIAN/control" <<EOF
Package: planora
Version: $deb_version
Architecture: amd64
Maintainer: Planora <maintainers@invalid.example>
Depends: libc6, libgcc-s1, libx11-6, libxkbcommon-x11-0, libwayland-client0, libpolkit-gobject-1-0, pkexec, systemd, libbpf1, libelf1t64 | libelf1, zlib1g
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
if [ "${2:-}" ]; then
  systemctl try-restart planora-helper.service || true
  systemctl try-restart planora-profiler.service || true
  systemctl try-restart planora-pmu.service || true
fi
systemctl enable --now planora-profiler.service
systemctl enable --now planora-pmu.service
systemctl enable --now planora-helper.service
EOF
cat > "$stage/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = remove ]; then
  systemctl disable --now planora-profiler.service || true
  systemctl disable --now planora-pmu.service || true
  systemctl disable --now planora-helper.service || true
fi
EOF
cat > "$stage/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
systemctl daemon-reload || true
if [ "$1" = remove ] || [ "$1" = purge ]; then
  rm -rf /var/lib/planora/authorized-uids
fi
if [ "$1" = purge ]; then
  userdel planora 2>/dev/null || true
  groupdel planora-users 2>/dev/null || true
fi
EOF
chmod 755 "$stage/DEBIAN/"{postinst,prerm,postrm}
dpkg-deb --build --root-owner-group "$stage" "$root/dist/planora_${deb_version}_amd64.deb"
