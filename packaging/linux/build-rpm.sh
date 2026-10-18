#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
mkdir -p "$root/dist" "$root/target/rpmbuild"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
rpmbuild -bb "$root/packaging/linux/planora.spec" \
  --define "_topdir $root/target/rpmbuild" \
  --define "planora_root $root"
find "$root/target/rpmbuild/RPMS" -type f -name '*.rpm' -exec cp -v {} "$root/dist/" \;
