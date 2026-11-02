#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="${PLANORA_VERSION:-$(cargo pkgid --manifest-path "$root/Cargo.toml" -p planora-ui | sed 's/.*[#@]//')}"
rpm_version="${version%%-*}"
if [ "$rpm_version" = "$version" ]; then rpm_release=1; else rpm_release="0.${version#*-}"; fi
release_dir="${CARGO_TARGET_DIR:-$root/target}/release"
bash "$root/packaging/linux/build-profiler.sh"
mkdir -p "$root/dist" "$root/target/rpmbuild"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
# A Windows checkout may give the spec CRLF endings. rpm copies %install
# commands verbatim into /bin/sh, where a trailing CR breaks shell loops.
spec="$root/target/rpmbuild/SPECS/planora.spec"
sed 's/\r$//' "$root/packaging/linux/planora.spec" > "$spec"
rpmbuild -bb "$spec" \
  --define "_topdir $root/target/rpmbuild" \
  --define "planora_root $root" \
  --define "planora_version $rpm_version" \
  --define "planora_release $rpm_release" \
  --define "planora_release_dir $release_dir"
rpm_dist="$(rpm --eval '%{?dist}')"
package="$root/target/rpmbuild/RPMS/x86_64/planora-${rpm_version}-${rpm_release}${rpm_dist}.x86_64.rpm"
test -f "$package"
cp -v "$package" "$root/dist/"
