Name: planora
Version: %{planora_version}
Release: 1%{?dist}
Summary: Native hardware visualizer and resource monitor
License: MIT OR Apache-2.0
BuildArch: x86_64
Requires: systemd, polkit

%description
Planora shows process and system resources with an authorized local helper.

%install
install -Dm755 %{planora_release_dir}/planora %{buildroot}/usr/bin/planora
install -Dm755 %{planora_release_dir}/planora-helper %{buildroot}/usr/libexec/planora/planora-helper
install -Dm755 %{planora_root}/packaging/linux/authorize-user %{buildroot}/usr/libexec/planora/authorize-user
install -Dm644 %{planora_root}/packaging/linux/planora-helper.service %{buildroot}/usr/lib/systemd/system/planora-helper.service
install -Dm644 %{planora_root}/packaging/linux/planora.sysusers %{buildroot}/usr/lib/sysusers.d/planora.conf
install -Dm644 %{planora_root}/packaging/linux/org.planora.authorize.policy %{buildroot}/usr/share/polkit-1/actions/org.planora.authorize.policy
install -Dm644 %{planora_root}/packaging/linux/planora.desktop %{buildroot}/usr/share/applications/planora.desktop
for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 %{planora_root}/assets/brand/platforms/linux/planora-$size.png %{buildroot}/usr/share/icons/hicolor/${size}x${size}/apps/planora.png
done
install -Dm644 %{planora_root}/assets/brand/platforms/linux/planora.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/planora.svg

%post
systemd-sysusers /usr/lib/sysusers.d/planora.conf
systemctl daemon-reload || true
systemctl enable --now planora-helper.service || true

%preun
if [ "$1" -eq 0 ]; then systemctl disable --now planora-helper.service || true; fi

%postun
systemctl daemon-reload || true
if [ "$1" -eq 0 ]; then
  userdel planora 2>/dev/null || true
  groupdel planora-users 2>/dev/null || true
fi

%files
/usr/bin/planora
/usr/libexec/planora/planora-helper
/usr/libexec/planora/authorize-user
/usr/lib/systemd/system/planora-helper.service
/usr/lib/sysusers.d/planora.conf
/usr/share/polkit-1/actions/org.planora.authorize.policy
/usr/share/applications/planora.desktop
/usr/share/icons/hicolor/*/apps/planora.png
/usr/share/icons/hicolor/scalable/apps/planora.svg
