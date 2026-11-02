# Planora beta.47 Windows and Ubuntu WSL stability evidence

This report separates the **installed beta.47 binaries** from later source-only test changes. The Windows Setup was built from `173186cd068cd42123adaf8034e5c6b7c0d78124`; the Ubuntu DEB was built from clean commit `76baabbc1531c966879cc4e47ab43994774c57a9`. The test scripts, the 10,000-process p95 assertion, and a Linux-only Clippy fix were committed later as `a443d0a0268623fea6d7eacccbe9c9972c1c3624`. That later commit has **not** been packaged as beta.47. All packages remain unsigned and internal.

## Environments and method

| Environment | Scope |
| --- | --- |
| Windows 11 Pro build 26300, Intel Core i7-1165G7 (4 cores, 8 logical processors) | Installed beta.47 UI, helper service, and sensor broker; live recording, cache benchmark, 15-second ETW disk profile, focused/minimized sampling, one-hour helper and process monitors |
| Ubuntu 24.04.5 LTS on WSL2 kernel `5.15.153.1-microsoft-standard-WSL2` | Isolated `Planora-Test-Ubuntu-24.04` distro; native DEB, systemd helper/profiler/PMU services, WSLg X11 UI, recording, one-hour helper/process/UI runs, package lifecycle |

The Ubuntu image came from the [official Ubuntu 24.04.5 WSL release](https://releases.ubuntu.com/noble/) and its SHA-256 matched the published `SHA256SUMS`. WSL2 and WSLg are **supplemental VM evidence**, not physical Ubuntu, Fedora, GNOME/KDE, or native GPU acceptance. WSLg used a Mesa llvmpipe Vulkan adapter; its frame times are not a physical Linux performance claim.

The sustained helper test holds an authenticated IPC connection, reads snapshots for 3,605 seconds, and requires every CPU observation to be less than 2,500 ms old. Process growth was sampled every ten seconds. The redacted source reports and CSVs are in [beta47-stability evidence](evidence/beta47-stability/), with [SHA-256 hashes](evidence/beta47-stability/SHA256SUMS.txt). Process CSVs contain Planora process names, ephemeral PIDs, and resource counters; they contain no usernames, process command lines, or authorization values.

## Results

| Check | Windows installed build | Ubuntu WSL installed build |
| --- | --- | --- |
| One-hour helper freshness | **Pass:** 3,512 consecutive fresh snapshots over 60.1 minutes | **Pass:** 3,578 consecutive fresh snapshots over 60.1 minutes |
| UI and collection latency | 39,846 UI frames, **3.656 ms p95**; collection **15.672 ms p95** in the late [UI report](evidence/beta47-stability/beta47-windows-ui-perf-late.json) | Ten-minute follow-up: **1.091 ms UI p95**, **13.678 ms collection p95** in the [WSLg report](evidence/beta47-stability/beta47-wsl-ui-stability.json); the earlier one-hour UI exited normally, but its `/tmp` report was lost when WSL shut down |
| Installed snapshot probes | 100 distinct of 100 under concurrent WSL load; collection **6.763 ms p95** in the [load probe](evidence/beta47-stability/beta47-windows-load-ipc.json) | 99 distinct of 100 before UI load, **4.755 ms p95**; 100 distinct of 100 with UI active, **14.302 ms p95** in the [baseline](evidence/beta47-stability/beta47-wsl-live-ipc.json) and [load](evidence/beta47-stability/beta47-wsl-load-ipc.json) probes |
| 10,000-process UI fixture | Frame p95 **6.47 ms** in an initial run and **14.70 ms** under WSL UI load, both below the 33.3 ms stress target | Same source regression passes workspace tests; no physical Linux 10,000-process native timing |
| Recording | Clean 256 MiB stop, **2,907 samples**, zero malformed records, zero reported drops, and an `end` record saying `recording reached 256 MiB limit`; **continuity failed** as described below | **2,459 samples** over 3,022.6 seconds, zero malformed records or reported drops, maximum inter-sample gap 1.518 seconds, and a complete `end` record |
| Advanced work | Installed cache probe completed; 15-second ETW disk profile returned **574 events, zero lost, 100% attributed bytes**. Windows PMU remains explicitly unimplemented | No PMU or disk-profile native acceptance in this run; WSL kernel/hardware cannot establish those physical-Linux paths |

The Windows process [growth CSV](evidence/beta47-stability/beta47-windows-process-growth.csv) shows the UI private bytes rising during early recording/history warm-up and then remaining near 380 MiB for the middle 40 minutes, falling to about 372 MiB in the final ten minutes. UI handles averaged about 516 across the middle intervals and 512 in the final interval. Helper private bytes stayed around 22 MiB; broker private bytes stayed around 42 MiB and its handles stayed at 283. No unexplained sustained Windows memory, handle, or thread growth was observed in this hour. Resident working set varied with Windows trimming, so it is not treated as an allocation trend.

At the saved 100 ms setting, a comparable 90-second focused sample averaged **8.16% UI + 11.32% helper CPU**. After the installed UI was actually minimized and the effective request reached 1,000 ms, a 90-second sample averaged **1.115% UI + 1.269% helper CPU**: a **87.8% combined reduction**. The [focused](evidence/beta47-stability/beta47-windows-focused-cpu-repeat.csv) and [minimized](evidence/beta47-stability/beta47-windows-minimized-cpu.csv) CSVs retain the raw counters. A first attempted minimize did not minimize the window and was excluded. Restoring focus returned the effective interval to 100 ms. This is a machine- and workload-specific comparison, not a universal CPU-use claim.

The initial one-hour Ubuntu process summary recorded the UI resident set increasing by about **75.4 MiB** and the helper's file-descriptor count increasing by **49**. Detailed CSV and the one-hour UI report had been written to WSL `/tmp` and were lost when the distro automatically shut down. A follow-up ten-minute run wrote persistent [growth data](evidence/beta47-stability/beta47-wsl-growth-repro.csv): helper descriptors changed by **+1** over 590 seconds, and its RSS changed by **+64 KiB**. Most open helper descriptors observed during that run were cached `/proc/*/stat` handles associated with process enumeration. The UI RSS increased by about **80.9 MiB** during the first ten minutes of history warm-up. These observations do not prove a sustained leak, but the lost one-hour series prevents closing that question. Repeat with persistent output on a physical Linux machine.

## Recording continuity defect

The Windows `.planora.jsonl` file ended cleanly at 268,396,078 bytes, beneath the 256 MiB cap, with no malformed lines and `dropped_before = 0` across its samples. Its provider timestamps contain a **190.935-second gap** from 13:55:24 to 13:58:35 local time while the helper sequence advanced by 199. This coincides with the minimized-window test. The UI currently calls `Recorder::record` from the UI update callback, so a minimized window can stop recording observations even though helper sampling continues. The format did not mark the missing period as dropped or as a gap. Treat this as a failed stability check: move recording ingestion to an owned worker fed by telemetry, then test minimizing, hiding, reconnecting, and shutdown before claiming continuous recording.

## Ubuntu package lifecycle

The internal `planora_0.3.0~beta.47-1_amd64.deb` SHA-256 is `3eff4eff8c4d786c4eeca4efa9ae5982d10d77df21c94aac2d1ee50c853094ac`. The installed UI and helper hashes were respectively `6bc49b8d32083bdcbdeccddb2714deb2597eb232046e5b387c35caf8587efb15` and `dd6259718b9b4b833204430771f2214485d9866d4eee93a529349e66f73327fe`. Both report beta.47 and protocol v6.

Reinstalling the same DEB as a repair kept all three systemd services active, preserved the authorization record and personal recording, and allowed authenticated IPC. In the isolated test distro, the root test harness invoked the installed authorize/revoke scripts with `PKEXEC_UID=1000`: revocation removed the record and the next client connection was reset; authorization restored it without logging out. This verifies the script and helper behavior, **not** a graphical polkit prompt. `dpkg -r` stopped all three services, removed the UI binary and authorization record, and preserved the user's recording. Reinstalling and reauthorizing restored the services and live IPC. `dpkg -r` leaves Debian's `deinstall ok config-files` state, as expected for removal rather than purge.

## Source checks and remaining gates

Windows formatting, workspace tests, and strict Clippy passed. Ubuntu WSL formatting and workspace tests passed; strict Clippy initially found a Windows-only sensor cache field compiled on Linux. Commit `a443d0a` gates that field and its initializers with `#[cfg(windows)]`; post-fix Windows and WSL strict Clippy and workspace tests passed. These checks apply to the later source commit, while the installed packages remain the clean beta.47 builds identified above. A changed package must use a new beta and package version.

The Windows recording gap is an open correctness defect. Physical Ubuntu/Fedora profiling, GPU/sensor, desktop/accessibility, and one-hour persistent growth checks remain open. Windows 10/11 breadth, signing, and distribution approval remain separate public-release gates. The current evidence supports internal testing only.
