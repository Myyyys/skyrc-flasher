# SkyRC Flasher (v1.0.0)

A native Linux firmware updater and flasher for **SkyRC Smart Chargers** (including B6neo, B6ACneo, T1000, D200neo, Q200neo, and more).

Reverse-engineered from the official Windows *SkyRC Charger Master* software (`charger master V1.55.exe`). Communicates natively over standard Linux USB HID (`/dev/hidraw*`) with **zero external C-library dependencies** and no Wine required.

---

## Features

- **One-Click Auto-Upgrade (`skyrc-flasher upgrade`)**:
  - Automatically detects the plugged-in charger over USB.
  - Reads the hardware model, hardware version, and installed firmware version.
  - Queries SkyRC's update server for the latest firmware and changelogs.
  - Compares versions, downloads the update into memory, verifies the 32-bit checksum, flashes the charger with a live progress bar, verifies on-device, and reboots.
- **Interactive Check & Download**:
  - Check available versions, release dates, file sizes, and release notes for any SkyRC charger model.
  - Interactive selection menu if no device is connected or if multiple firmware versions exist.
- **Local File Flashing (`skyrc-flasher flash`)**:
  - Flash any downloaded or custom firmware `.bin` file directly.
- **Pure Rust & Zero C-Dependencies**:
  - Direct communication with kernel `/dev/hidraw*` via non-blocking I/O and `poll`.

---

## Supported Devices

| Core ID | Model Name | Tested / Status |
|---|---|---|
| **`100198`** | **SkyRC B6neo** / X1 NANO USB | **Verified & Working (v2.17)** |
| **`100200`** | **SkyRC B6ACneo** / X1 NANO CUBE | Supported |
| **`100211`** | **SkyRC B6neo+** / X1 NANO USB+ | Supported |
| **`100225`** | **SkyRC B6neo 2** | Supported |
| **`100182`** | **SkyRC T1000 Maestro** / V1000 | Supported |
| **`100196`** | **SkyRC D200neo** / X2 800 | Supported |
| **`100197`** | **SkyRC Q200neo** / X4 AC PLUS 400 | Supported |
| **`100199`** | **SkyRC D100neo** / X2 AC Plus 200 | Supported |
| **`100202`** | **SkyRC S100neo** / X1 AC PLUS IV | Supported |
| **`100203`** | **SkyRC D200neo** / GMA6800SP | Supported |
| **`100223`** | **SkyRC Q100neo** / Pocket Base | Supported |
| **`100213`** | **SkyRC GMA620MP** / RC-S100neo | Supported |
| **`100220`** | **SkyRC PC1080neo** / 1080 UAV | Supported |
| **`100084`** | **SkyRC MC3000** | Supported |
| *Other* | Any device supported by SkyRC Charger Master | Enter Core ID manually |

---

## Installation

### 1. Configure USB Permissions (udev Rule)
By default, Linux limits raw access to `/dev/hidraw*` devices to root. To use the tool without `sudo`, add the following udev rule:

```bash
echo 'SUBSYSTEM=="hidraw", ATTRS{idVendor}=="0000", ATTRS{idProduct}=="0001", MODE="0666"' | sudo tee /etc/udev/rules.d/99-skyrc.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
```

### 2. Build and Install from Source
Ensure Rust is installed (`rustc` and `cargo` >= 1.80):

```bash
cargo install --path skyrc-flasher
```

This installs `skyrc-flasher` directly to `~/.cargo/bin/skyrc-flasher`.

---

## Usage Guide

### 1. One-Click Auto-Upgrade
Connect your charger using a USB-C data cable and run:

```bash
skyrc-flasher upgrade
```
*(Or simply `skyrc-flasher` without arguments, which defaults to `upgrade`)*

Example output:
```text
=== SkyRC Auto-Upgrade ===
Detecting connected charger...
Device detected at: /dev/hidraw11
Connected Device: B6neo / X1 NANO USB [Core: 100198, HW: 1]
Installed Version: v2.14

Checking for updates on SkyRC server...
Latest Available:  v2.17
Build Date:        2026-07-22 09:57 UTC
Firmware Size:     339.3 KB (347404 bytes)
Release Notes:     Tweaked firmware.

==> An upgrade is available! (v2.14 -> v2.17)
Do you want to proceed with downloading and flashing? [y/N]: y

Downloading firmware v2.17...
Downloaded 339.3 KB (347404 bytes).
Checksum verified: 31772827

Starting firmware flash...
Switching charger to update mode (CMD 96)...
Sending update handshake (CMD 1)...
Handshake acknowledged! Flashing blocks...
[========================================] 100.0% (347404/347404 bytes)
Verifying written firmware (CMD 4)...
Firmware verified successfully by device!
Rebooting device (CMD 5)...

=== Upgrade Completed Successfully! ===
Your charger has been updated to v2.17!
```

- Pass `-y` or `--yes` to skip the confirmation prompt.
- Pass `-f` or `--force` to reflash the firmware even if already on the latest version.

---

### 2. Query Connected Device Info
```bash
skyrc-flasher info
```

Example output:
```text
=== SkyRC Charger Information ===
  Model:            B6neo / X1 NANO USB
  Core ID:          100198
  Firmware Version: 2.17
  Hardware Version: 1
  Customer ID:      0
  Language ID:      0
  Encryption:       No
  Upgrade Type:     1
```

---

### 3. Check Available Firmware Releases
```bash
skyrc-flasher check
```
- If a device is plugged in, it checks updates for that model.
- If no device is plugged in, it presents an interactive menu to pick any supported model.
- You can also specify the Core ID directly on the command line:
  ```bash
  skyrc-flasher check --core 100198
  ```

---

### 4. Download Firmware to Disk
```bash
skyrc-flasher download
```
Interactively prompts for device model and firmware version, downloads the `.bin` file, and verifies its 32-bit checksum.

Or specify options directly:
```bash
skyrc-flasher download --core 100198 --version 2.17 -o b6neo_v2.17.bin
```

---

### 5. Flash a Local File
```bash
skyrc-flasher flash b6neo_v2.17.bin
```

---

## Technical Details

SkyRC smart chargers communicate via USB HID:
- **Vendor ID (VID)**: `0x0000`
- **Product ID (PID)**: `0x0001`
- **Packet Structure**: 65 bytes (Report ID `0x00` + 64 data bytes).

### Update Protocol Sequence:
1. **`CMD 87 (0x57)`**: Queries hardware identification, current firmware version, and hardware version.
2. **`CMD 96 (0x60)`**: Commands the MCU to switch from application mode into the internal bootloader (~4s reboot).
3. **`CMD 1`**: Handshake announcing the firmware binary size and 32-bit byte-sum checksum. MCU prepares internal flash sectors (~1.7s).
4. **`CMD 2` & `CMD 3`**:
   - `CMD 2` announces the 1024-byte block index and length; charger responds with acknowledgement byte.
   - `CMD 3` streams 34 sub-slices of up to 31 bytes each with 1.5ms pacing to match MCU buffer consumption.
5. **`CMD 4`**: Final verification command; device validates the complete image against the header checksum.
6. **`CMD 5`**: Reboot command; device boots into the newly flashed firmware.

---

## License

This project is licensed under the [MIT License](LICENSE).
