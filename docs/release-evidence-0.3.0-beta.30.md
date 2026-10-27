# Planora 0.3.0-beta.30 desktop shortcut

The previous MSI defined only Start menu shortcuts. Beta.30 adds a Planora desktop shortcut to the existing MSI-owned shortcut component, targeting the installed GUI executable with its Planora icon and installation working directory. Per-machine installation resolves DesktopFolder to the public desktop. Windows Installer owns its installation, repair and removal. No shell wrapper or elevated UI launch is added.

Setup copy now explicitly mentions the desktop shortcut. All beta.29 Overview fixes remain included. MSI version is `0.2.1030`; beta.29 artifacts are retained unchanged.

Built from clean source `8f2c617`. WiX MSI validation, installer theme checks, GUI subsystem, binary identities and PawnIO signature/hash checks passed. The built MSI Shortcut table contains DesktopShortcut targeting `[INSTALLFOLDER]planora.exe`, DesktopFolder, PlanoraIcon.ico and INSTALLFOLDER working directory. Setup/MSI copies match their manifest hashes. [Package manifest](screenshots/native/beta30/package-manifest.json), [built shortcut validation](screenshots/native/beta30/shortcut-msi-validation.json).

The user completed installation; Burn exited `0x0` with no restart. All installed UI/helper/broker hashes match the manifest. The helper is running, authorization remains present and the settings hash is unchanged.

`C:\Users\Public\Desktop\Planora.lnk` exists with the expected installed executable, no command arguments, Program Files working directory and cached Planora icon. [Installed verification](screenshots/native/beta30/installed-verification.json).

Shortcut activation initially encountered an old beta.28 development process (PID 13468, started 01:23) holding the single-instance lock without any top-level window. No window existed to receive a normal close message; that exact stale development process was terminated. A subsequent launch through the desktop shortcut opened the installed beta.30 window. Startup diagnostics record commit `8f2c617`, normal launch, restored maximized state, first painted frame, 125 percent system scale and helper connected. The app was left open. This is not evidence that all shutdown paths are fixed; the stale development-process lifecycle remains a separate issue. Full uninstall/repair lifecycle retesting is not implied by MSI validation.

Implementation follows the [WiX Shortcut schema](https://docs.firegiant.com/wix/schema/wxs/shortcut/).
