# Planora Google Play Publishing & Early Access Guide

**Target Release**: v1.0.0 (Early Access / Closed Testing Track)  
**Target Platform**: Android 14+ (API Level 34/35)  
**Track**: Closed Testing (Alpha) $\rightarrow$ Production Track  

---

## 1. Overview & Distribution Strategy

To ensure hardware monitoring and telemetry companion capabilities reach mobile users safely, Planora coordinates mobile companion distributions across two official release channels:
1. **Google Play Store (Early Release / Closed Testing Track)**: Managed through the Google Play Console for verified testers, automatic updates, and crash diagnostics.
2. **Direct APK Sideloading (GitHub Releases)**: Standalone, production-signed release APKs attached directly to GitHub Releases for users in regions without Google Play access or running de-Googled Android distributions (e.g., GrapheneOS, LineageOS).

---

## 2. Google Play Console Release Requirements

### 2.1 Release Artifact Formats
- **Format**: Android App Bundle (`.aab`) required for Google Play Console submission.
- **Signing**: Google Play App Signing with a dedicated upload keystore.
  - Keystore parameters: RSA 4096-bit, SHA-256 with RSA.
- **Direct Sideloading Format**: Universal APK (`Planora-v1.0.0-universal.apk`) or split per-ABI APKs (`arm64-v8a`, `armeabi-v7a`, `x86_64`) signed with the release certificate and published on GitHub Releases.

### 2.2 Store Listing & Policy Compliance
- **Content Rating**: Everyone (OARS 1.1).
- **Target API**: Target API 35 (Android 15) with minimum API 26 (Android 8.0 Oreo).
- **Data Safety Declarations**:
  - No personal identifiable information (PII) collected or sold.
  - Diagnostic logs and anonymous telemetry are opt-in and stored purely locally on-device.
  - No background location access requested.
- **Permissions**:
  - `FOREGROUND_SERVICE` (Micro-telemetry / connection monitoring)
  - `INTERNET` (Local network discovery of Planora desktop instances over authenticated mDNS/IPC)
  - `ACCESS_NETWORK_STATE` (WiFi/cellular interface throughput metrics)

---

## 3. Closed Testing Track Setup & Tester Management

Under Google Play policies, new developer accounts and apps must complete a mandatory closed testing cycle (minimum 12–20 testers opted in continuously for at least 14 consecutive days) before applying for open Production access.

### 3.1 Configuring the Closed Testing Track
1. Navigate to **Google Play Console** $\rightarrow$ select your app $\rightarrow$ **Testing** $\rightarrow$ **Closed testing**.
2. Click **Create track** (or select **Closed testing - Alpha**).
3. Under **Testers**:
   - Create an email list (e.g., `Planora Early Access Testers`).
   - Add tester Gmail or Google Workspace accounts.
   - Click **Save changes**.

### 3.2 Obtaining and Sharing the Tester Opt-in Links
Testers cannot find the app via public Google Play search while in Closed Testing. They must explicitly opt-in:
1. In the **Closed testing** dashboard, navigate to the **Testers** tab.
2. Under **How testers join your test**, copy both links:
   - **Join on the web**: `https://play.google.com/apps/testing/org.planora.android`
   - **Join on Android**: `https://play.google.com/store/apps/details?id=org.planora.android`
3. **Important Tester Onboarding Rule**:
   - Simply adding an email to the list **does not count** towards Google Play's required active tester quota.
   - Testers **must open the web link**, sign in with their authorized Google account, click **"Become a tester"**, and then download the app from Google Play onto their physical device.

---

## 4. Direct APK Sideloading Instructions (GitHub Releases)

For users who are not part of the closed testing group or choose not to use Google Play:

1. Download the verified production APK from GitHub Releases:
   ```
   https://github.com/GDJ2001/Planora/releases/tag/v1.0.0
   ```
2. Verify the SHA-256 digest against `SHA256SUMS.txt`:
   ```sh
   sha256sum -c SHA256SUMS.txt --ignore-missing
   ```
3. Enable "Install Unknown Apps" for your browser or file manager under Android Settings:
   - **Settings** $\rightarrow$ **Apps** $\rightarrow$ **Special app access** $\rightarrow$ **Install unknown apps** $\rightarrow$ Allow for your browser/file manager.
4. Tap the downloaded `.apk` file and confirm the installation.
5. On Android 13+, grant notification permissions if background monitoring alerts are desired.

---

## 5. Promotion to Google Play Production Track

Once the 14-day closed testing period is completed and engagement criteria are satisfied:
1. In Google Play Console, navigate to **Dashboard** $\rightarrow$ **Apply for production access**.
2. Complete the Production Access Questionnaire detailing testing feedback, bug fixes implemented during testing, and release criteria.
3. Upon Google Play approval:
   - Go to **Testing** $\rightarrow$ **Closed testing** $\rightarrow$ Click **Manage track**.
   - Click **Promote release** $\rightarrow$ Select **Production**.
   - Review release notes, countries/regions (178 countries/regions, 100% rollout), and click **Start rollout to Production**.
