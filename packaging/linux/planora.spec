Name: planora
Version: %{planora_version}
Release: %{planora_release}%{?dist}
Summary: Native hardware visualizer and resource monitor
License: MIT OR Apache-2.0
BuildArch: x86_64
Requires: systemd, polkit, libxkbcommon-x11

%description
Planora shows process and system resources with an authorized local helper.

%install
install -Dm755 %{planora_release_dir}/planora %{buildroot}/usr/bin/planora
install -Dm755 %{planora_release_dir}/planora-helper %{buildroot}/usr/libexec/planora/planora-helper
install -Dm755 %{planora_root}/packaging/linux/authorize-user %{buildroot}/usr/libexec/planora/authorize-user
install -Dm755 %{planora_root}/packaging/linux/revoke-user %{buildroot}/usr/libexec/planora/revoke-user
install -Dm644 %{planora_root}/packaging/linux/planora-helper.service %{buildroot}/usr/lib/systemd/system/planora-helper.service
install -Dm644 %{planora_root}/packaging/linux/planora.sysusers %{buildroot}/usr/lib/sysusers.d/planora.conf
install -Dm644 %{planora_root}/packaging/linux/org.planora.authorize.policy %{buildroot}/usr/share/polkit-1/actions/org.planora.authorize.policy
install -Dm644 %{planora_root}/packaging/linux/org.planora.revoke.policy %{buildroot}/usr/share/polkit-1/actions/org.planora.revoke.policy
install -Dm644 %{planora_root}/packaging/linux/planora.desktop %{buildroot}/usr/share/applications/planora.desktop
install -Dm644 %{planora_root}/packaging/linux/org.planora.Planora.metainfo.xml %{buildroot}/usr/share/metainfo/org.planora.planora.metainfo.xml
for license_file in LICENSE-MIT LICENSE-APACHE NOTICE; do
  install -Dm644 %{planora_root}/$license_file %{buildroot}/usr/share/licenses/planora/$license_file
done
for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 %{planora_root}/assets/brand/platforms/linux/planora-$size.png %{buildroot}/usr/share/icons/hicolor/${size}x${size}/apps/planora.png
done
install -Dm644 %{planora_root}/assets/brand/platforms/linux/planora.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/planora.svg

%post
systemd-sysusers /usr/lib/sysusers.d/planora.conf
systemctl daemon-reload || true
if [ "$1" -gt 1 ]; then systemctl try-restart planora-helper.service || true; fi
systemctl enable --now planora-helper.service || true

%preun
if [ "$1" -eq 0 ]; then systemctl disable --now planora-helper.service || true; fi

%postun
systemctl daemon-reload || true
if [ "$1" -eq 0 ]; then
  rm -rf /var/lib/planora/authorized-uids
  userdel planora 2>/dev/null || true
  groupdel planora-users 2>/dev/null || true
fi

%files
/usr/bin/planora
/usr/libexec/planora/planora-helper
/usr/libexec/planora/authorize-user
/usr/libexec/planora/revoke-user
/usr/lib/systemd/system/planora-helper.service
/usr/lib/sysusers.d/planora.conf
/usr/share/polkit-1/actions/org.planora.authorize.policy
/usr/share/polkit-1/actions/org.planora.revoke.policy
/usr/share/applications/planora.desktop
/usr/share/metainfo/org.planora.planora.metainfo.xml
/usr/share/icons/hicolor/*/apps/planora.png
/usr/share/icons/hicolor/scalable/apps/planora.svg
%license /usr/share/licenses/planora/LICENSE-MIT
%license /usr/share/licenses/planora/LICENSE-APACHE
%license /usr/share/licenses/planora/NOTICE
