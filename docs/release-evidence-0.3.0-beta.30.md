# Planora 0.3.0-beta.30 desktop shortcut

The previous MSI defined only Start menu shortcuts. Beta.30 adds a Planora desktop shortcut to the existing MSI-owned shortcut component, targeting the installed GUI executable with its Planora icon and installation working directory. Per-machine installation resolves DesktopFolder to the public desktop. Windows Installer owns its installation, repair and removal. No shell wrapper or elevated UI launch is added.

Setup copy now explicitly mentions the desktop shortcut. All beta.29 Overview fixes remain included. MSI version is `0.2.1030`; beta.29 artifacts are retained unchanged.

Package validation and native shortcut verification are recorded after the clean-source build. Full uninstall/repair lifecycle retesting is not implied by MSI validation.

Implementation follows the [WiX Shortcut schema](https://docs.firegiant.com/wix/schema/wxs/shortcut/).
