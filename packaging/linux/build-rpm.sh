#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="${PLANORA_VERSION:-$(cargo pkgid --manifest-path "$root/Cargo.toml" -p planora-ui | sed 's/.*[#@]//')}"
release_dir="${CARGO_TARGET_DIR:-$root/target}/release"
mkdir -p "$root/dist" "$root/target/rpmbuild"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
# A Windows checkout may give the spec CRLF endings. rpm copies %install
# commands verbatim into /bin/sh, where a trailing CR breaks shell loops.
spec="$root/target/rpmbuild/SPECS/planora.spec"
sed 's/\r$//' "$root/packaging/linux/planora.spec" > "$spec"
rpmbuild -bb "$spec" \
  --define "_topdir $root/target/rpmbuild" \
  --define "planora_root $root" \
  --define "planora_version $version" \
  --define "planora_release_dir $release_dir"
find "$root/target/rpmbuild/RPMS" -type f -name '*.rpm' -exec cp -v {} "$root/dist/" \;
