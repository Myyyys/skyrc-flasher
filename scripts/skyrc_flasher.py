#!/usr/bin/env python3
"""
SkyRC Smart Charger Firmware Tool for Linux
Supports B6neo (core 100198) and other SkyRC chargers.

Reverse engineered from SkyRC Charger Master (V1.55 / Charger Master_E).
Vendor ID: 0x0000, Product ID: 0x0001
"""

import os
import sys
import glob
import time
import json
import datetime
import argparse
import urllib.request
import select

SKYRC_VID = 0x0000
SKYRC_PID = 0x0001

CORE_NAMES = {
    "100198": "B6neo / X1 NANO USB",
    "100200": "B6ACneo / X1 NANO CUBE",
    "100211": "B6neo+ / X1 NANO USB+",
    "100225": "B6neo 2",
    "100182": "T1000 Maestro / V1000",
    "100196": "D200neo / X2 800",
    "100197": "Q200neo / X4 AC PLUS 400",
    "100199": "D100neo / X2 AC Plus 200",
    "100202": "S100neo / X1 AC PLUS IV",
    "100203": "D200neo / GMA6800SP",
    "100223": "Q100neo / Pocket Base",
    "100213": "GMA620MP / RC-S100neo",
    "100220": "PC1080neo / 1080 UAV Charger",
    "100221": "PC3020-1",
    "100222": "YZ-114PLUS",
    "100226": "PC1500neo",
    "100227": "PC3020-2",
    "100231": "PC7200",
    "100236": "CH003 / X2 V Pocket",
    "100237": "CH005 / X1 Pocket EX",
    "100239": "CH007",
    "100201": "D750 MIX",
    "100185": "NC2500 Pro",
    "100084": "MC3000",
}


def format_timestamp(ts):
    try:
        ts_int = int(ts)
        if ts_int <= 0:
            return "Unknown"
        dt = datetime.datetime.fromtimestamp(ts_int, datetime.timezone.utc)
        return dt.strftime("%Y-%m-%d %H:%M UTC")
    except Exception:
        return "Unknown"


def format_size(num_bytes):
    if num_bytes is None:
        return ""
    if num_bytes >= 1024 * 1024:
        return f"{num_bytes / (1024*1024):.2f} MB ({num_bytes} bytes)"
    elif num_bytes >= 1024:
        return f"{num_bytes / 1024:.1f} KB ({num_bytes} bytes)"
    return f"{num_bytes} bytes"


def fetch_size(url):
    try:
        req = urllib.request.Request(url, method="HEAD", headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=5) as resp:
            cl = resp.headers.get("Content-Length")
            if cl:
                return int(cl)
    except Exception:
        pass
    return None


def make_sn(core_type="100198", upgrade_type=1, is_encrypt=0, customer_id=0, language_id=0, sw_ver=(1, 0), hw_ver=1, reserved=0):
    """Generate 16-byte hex SN string for SkyRC update API."""
    b = bytearray()
    b.extend(core_type.encode("ascii")[:6])
    b.append(upgrade_type)
    b.append(is_encrypt)
    b.append((customer_id >> 8) & 0xFF)
    b.append(customer_id & 0xFF)
    b.append(language_id)
    b.append(sw_ver[0])
    b.append(sw_ver[1])
    b.append(hw_ver)
    b.append(reserved)
    chk = sum(b) & 0xFF
    chk = ((~chk) + 1) & 0xFF
    b.append(chk)
    return b.hex()


def query_firmware(core_type="100198", hw_ver=1):
    """Query SkyRC online upgrade API for available firmware releases."""
    sn = make_sn(core_type=core_type, hw_ver=hw_ver)
    url = f"http://upgrade.skyrc.com/?SN={sn}&test=0"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 (SkyRC Updater)"})
    with urllib.request.urlopen(req, timeout=10) as resp:
        content = resp.read().decode("utf-8")
        if content and content != "[]":
            return json.loads(content)
    return []


def prompt_for_core():
    dev = find_hidraw_device()
    if dev:
        try:
            with SkyRCCharger(dev) as charger:
                info = charger.get_device_info()
                if info:
                    ans = input(f"Detected connected charger: {info['model_name']} (Core: {info['core_type']}). Use this device? [Y/n]: ").strip()
                    if not ans or ans.lower() == 'y':
                        return info['core_type'], info['hardware_version']
        except Exception:
            pass

    print("\nSelect a SkyRC device model:")
    items = list(CORE_NAMES.items())
    for idx, (core, name) in enumerate(items, 1):
        print(f"  {idx:2}) {name:<26} (Core ID: {core})")
    print("   0) Enter custom Core ID manually")

    while True:
        choice = input(f"\nEnter choice [1-{len(items)} or Core ID]: ").strip()
        if not choice:
            continue
        if choice.isdigit():
            val = int(choice)
            if 1 <= val <= len(items):
                return items[val - 1][0], 1
            elif val == 0:
                manual = input("Enter Core ID (e.g. 100198): ").strip()
                if manual:
                    return manual, 1
        if len(choice) >= 4:
            return choice, 1
        print("Invalid choice, please select a valid number or enter a Core ID.")


def find_hidraw_device(target_vid=SKYRC_VID, target_pid=SKYRC_PID):
    """Find /dev/hidraw* device corresponding to the SkyRC VID/PID."""
    for path in glob.glob("/sys/class/hidraw/hidraw*"):
        uevent_file = os.path.join(path, "device", "uevent")
        if not os.path.exists(uevent_file):
            continue
        try:
            with open(uevent_file, "r") as f:
                data = f.read()
            for line in data.splitlines():
                if line.startswith("HID_ID="):
                    parts = line.split("=")[1].split(":")
                    if len(parts) >= 3:
                        vid = int(parts[1], 16)
                        pid = int(parts[2], 16)
                        if vid == target_vid and pid == target_pid:
                            dev_name = os.path.basename(path)
                            return f"/dev/{dev_name}"
        except Exception:
            continue
    return None


class SkyRCCharger:
    def __init__(self, dev_path=None):
        if dev_path is None:
            dev_path = find_hidraw_device()
            if not dev_path:
                raise FileNotFoundError(
                    f"No SkyRC charger found (USB VID {SKYRC_VID:04x}: PID {SKYRC_PID:04x}).\n"
                    "Make sure the device is plugged in via a USB data cable."
                )
        self.dev_path = dev_path
        self.fd = None

    def open(self):
        try:
            self.fd = os.open(self.dev_path, os.O_RDWR | os.O_NONBLOCK)
        except PermissionError:
            raise PermissionError(
                f"Permission denied accessing {self.dev_path}.\n"
                f"Run with sudo or add udev rule: SUBSYSTEM==\"hidraw\", ATTRS{{idVendor}}==\"0000\", ATTRS{{idProduct}}==\"0001\", MODE=\"0666\""
            )

    def close(self):
        if self.fd is not None:
            try:
                os.close(self.fd)
            except Exception:
                pass
            self.fd = None

    def __enter__(self):
        self.open()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()

    def drain(self):
        """Drain any pending unread reports."""
        while True:
            r, _, _ = select.select([self.fd], [], [], 0.05)
            if not r:
                break
            try:
                os.read(self.fd, 65)
            except Exception:
                break

    def send_report(self, data):
        """Write a 65-byte report (0x00 Report ID + 64 bytes)."""
        buf = bytearray(65)
        buf[0] = 0x00
        buf[1:1+len(data)] = data
        os.write(self.fd, buf)

    def read_report(self, timeout=5.0):
        """Read 64/65 byte report from device with timeout."""
        r, _, _ = select.select([self.fd], [], [], timeout)
        if not r:
            return None
        raw = os.read(self.fd, 65)
        pkt = bytearray(65)
        if len(raw) == 64:
            pkt[1:] = raw
        else:
            pkt[:] = raw
        return pkt

    def get_device_info(self):
        """Send CMD 87 (0x57) and parse device identification."""
        self.drain()
        cmd = bytearray(64)
        cmd[0] = 15     # 0x0F
        cmd[1] = 3      # length
        cmd[2] = 87     # 0x57
        cmd[3] = 1      # slot 0 (1 << 0)
        cmd[4] = 88     # checksum: 87 + 1 = 88
        self.send_report(cmd)

        reply = self.read_report(timeout=2.0)
        if not reply or reply[1] != 15 or reply[3] != 87:
            return None

        core_type = reply[6:12].decode("ascii", errors="ignore").strip("\x00")
        upgrade_type = reply[12]
        is_encrypt = bool(reply[13])
        customer_id = (reply[14] << 8) | reply[15]
        language_id = reply[16]
        software_version = float(reply[17]) + float(reply[18]) / 100.0
        hardware_version = reply[19]
        reserved = reply[20]

        return {
            "core_type": core_type,
            "model_name": CORE_NAMES.get(core_type, "Unknown SkyRC Model"),
            "upgrade_type": upgrade_type,
            "is_encrypt": is_encrypt,
            "customer_id": customer_id,
            "language_id": language_id,
            "software_version": f"{software_version:.2f}",
            "hardware_version": hardware_version,
            "reserved": reserved,
        }

    def enter_update_mode(self):
        """Send CMD 96 (0x60) to switch device into bootloader mode."""
        cmd = bytearray(64)
        cmd[0] = 15     # 0x0F
        cmd[1] = 2      # length
        cmd[2] = 96     # 0x60
        cmd[3] = 96     # checksum
        self.send_report(cmd)

    def flash_firmware(self, fw_data, progress_callback=None):
        """
        Flash raw firmware binary to the charger using SkyRC protocol.
        """
        fw_length = len(fw_data)
        fw_checksum = sum(fw_data) & 0xFFFFFFFF

        print(f"Firmware length: {format_size(fw_length)}, Checksum: {fw_checksum}")
        print("Switching charger to update mode (CMD 96)...")
        self.drain()
        self.enter_update_mode()

        # Charger takes 4 seconds to enter bootloader
        time.sleep(4.0)
        self.drain()

        # Step 1: Handshake with file length and checksum (CMD 1)
        print("Sending update handshake (CMD 1)...")
        cmd1 = bytearray(64)
        cmd1[0] = 1
        cmd1[1] = (fw_length >> 24) & 0xFF
        cmd1[2] = (fw_length >> 16) & 0xFF
        cmd1[3] = (fw_length >> 8) & 0xFF
        cmd1[4] = fw_length & 0xFF
        cmd1[5] = (fw_checksum >> 24) & 0xFF
        cmd1[6] = (fw_checksum >> 16) & 0xFF
        cmd1[7] = (fw_checksum >> 8) & 0xFF
        cmd1[8] = fw_checksum & 0xFF

        self.send_report(cmd1)

        ack1 = self.read_report(timeout=6.0)
        if not ack1 or ack1[1] != 1:
            raise RuntimeError(f"Handshake failed: unexpected response from bootloader ({list(ack1[:10]) if ack1 else 'None'})")

        for j in range(1, 9):
            if ack1[j] != cmd1[j - 1]:
                raise RuntimeError(f"Handshake mismatch at byte {j}: expected {cmd1[j-1]}, got {ack1[j]}")

        print("Handshake acknowledged! Flashing blocks...")

        # Step 2: Transfer 1024-byte blocks
        updata_index = 0

        while updata_index < fw_length:
            send_length = min(1024, fw_length - updata_index)

            # Send CMD 2 (block header)
            cmd2 = bytearray(64)
            cmd2[0] = 2
            cmd2[1] = (updata_index >> 24) & 0xFF
            cmd2[2] = (updata_index >> 16) & 0xFF
            cmd2[3] = (updata_index >> 8) & 0xFF
            cmd2[4] = updata_index & 0xFF
            cmd2[5] = (send_length >> 8) & 0xFF
            cmd2[6] = send_length & 0xFF

            self.send_report(cmd2)
            ack2 = self.read_report(timeout=5.0)
            if not ack2 or ack2[1] != 2 or ack2[8] != 1:
                raise RuntimeError(
                    f"Block announce failed at offset {updata_index} "
                    f"(ack={list(ack2[:10]) if ack2 else 'None'})"
                )

            # Send CMD 3 (34 data slices of up to 31 bytes each)
            block_data = fw_data[updata_index:updata_index + send_length]
            slice_offset = 0

            for _ in range(34):
                cmd3 = bytearray(64)
                cmd3[0] = 3
                slice_len = min(31, len(block_data) - slice_offset)
                if slice_len > 0:
                    cmd3[1:1+slice_len] = block_data[slice_offset:slice_offset + slice_len]
                    slice_offset += slice_len
                self.send_report(cmd3)
                time.sleep(0.0015)  # 1.5ms pacing for MCU USB buffer

            updata_index += send_length
            if progress_callback:
                progress_callback(updata_index, fw_length)

        # Step 3: Finalize & Verify (CMD 4)
        print("\nVerifying written firmware (CMD 4)...")
        cmd4 = bytearray(64)
        cmd4[0] = 4
        self.send_report(cmd4)

        ack4 = self.read_report(timeout=10.0)
        if not ack4 or ack4[1] != 4 or ack4[2] != 1:
            raise RuntimeError(f"Verification failed: device reported error (ack={list(ack4[:6]) if ack4 else None})")

        print("Firmware verified successfully by device!")

        # Step 4: Reboot device (CMD 5)
        print("Rebooting device (CMD 5)...")
        cmd5 = bytearray(64)
        cmd5[0] = 5
        self.send_report(cmd5)
        time.sleep(3.0)
        print("Upgrade complete! Device rebooted successfully.")


def print_progress(current, total):
    pct = (current / total) * 100.0
    bar_len = 40
    filled = int(bar_len * current / total)
    bar = "=" * filled + "-" * (bar_len - filled)
    sys.stdout.write(f"\r[{bar}] {pct:5.1f}% ({current}/{total} bytes)")
    sys.stdout.flush()


def cmd_check(args):
    core, hw_ver = (args.core, 1) if args.core else prompt_for_core()
    print(f"\nQuerying SkyRC server for core {core} ({CORE_NAMES.get(core, 'Unknown')})...")
    releases = query_firmware(core_type=core, hw_ver=hw_ver)
    if not releases:
        print("No firmware found on SkyRC server.")
        return

    print(f"\nFound {len(releases)} firmware release(s):")
    for r in releases:
        memo_en = r.get("memo", {}).get("en", "") or r.get("memo", {}).get("cn", "")
        dt_str = format_timestamp(r.get("timestamp"))
        size_bytes = fetch_size(r.get("download_url"))
        print(f"  - Version:    v{r.get('version')}")
        print(f"    Build Date: {dt_str}")
        if size_bytes:
            print(f"    Size:       {format_size(size_bytes)}")
        print(f"    Checksum:   {r.get('checksum')}")
        print(f"    URL:        {r.get('download_url')}")
        if memo_en:
            print(f"    Notes:      {memo_en.strip()}")
        print()


def cmd_download(args):
    core, hw_ver = (args.core, 1) if args.core else prompt_for_core()
    releases = query_firmware(core_type=core, hw_ver=hw_ver)
    if not releases:
        print("No firmware found.")
        return

    target = releases[0]  # default latest
    if args.version:
        matches = [r for r in releases if r.get("version") == args.version]
        if not matches:
            print(f"Version {args.version} not found.")
            return
        target = matches[0]
    elif len(releases) > 1:
        print(f"\nAvailable versions for {CORE_NAMES.get(core, 'SkyRC Device')}:")
        for idx, r in enumerate(releases, 1):
            latest_str = " (Latest)" if idx == 1 else ""
            dt_str = format_timestamp(r.get("timestamp"))
            size_val = fetch_size(r.get("download_url"))
            size_str = f" | {format_size(size_val)}" if size_val else ""
            print(f"  {idx:2}) v{r.get('version')}{latest_str} [Date: {dt_str}]{size_str} - Checksum: {r.get('checksum')}")
        ch = input("\nSelect version to download [1]: ").strip()
        if ch.isdigit() and 1 <= int(ch) <= len(releases):
            target = releases[int(ch) - 1]

    url = target.get("download_url")
    ver = target.get("version")
    dt_str = format_timestamp(target.get("timestamp"))
    filename = args.output or f"skyrc_{core}_v{ver}.bin"
    print(f"\nDownloading Version {ver} [Date: {dt_str}] from {url} to {filename}...")
    urllib.request.urlretrieve(url, filename)

    with open(filename, "rb") as f:
        data = f.read()
    chk = sum(data)
    expected = int(target.get("checksum", 0))
    print(f"Downloaded {format_size(len(data))}.")
    print(f"Checksum: calculated = {chk}, expected = {expected}")
    if chk == expected:
        print("Checksum matched! File is ready.")
    else:
        print("WARNING: Checksum mismatch!")


def cmd_info(args):
    print("Searching for connected SkyRC charger...")
    dev = find_hidraw_device()
    if not dev:
        print("No charger detected.")
        return
    print(f"Found USB HID device at {dev}")
    with SkyRCCharger(dev) as charger:
        info = charger.get_device_info()
        if not info:
            print("Failed to read device info (is device ready?).")
            return
        print("\n--- Device Information ---")
        for k, v in info.items():
            print(f"  {k:18}: {v}")


def cmd_flash(args):
    fw_path = args.file
    if not os.path.exists(fw_path):
        print(f"File not found: {fw_path}")
        return

    with open(fw_path, "rb") as f:
        fw_data = f.read()

    dev = args.device or find_hidraw_device()
    if not dev:
        print("No charger detected. Please connect device via USB.")
        return

    print(f"Found charger at {dev}")
    with SkyRCCharger(dev) as charger:
        info = charger.get_device_info()
        if info:
            print(f"Connected: {info['model_name']} (Current Version: {info['software_version']}, HW: {info['hardware_version']})")
        else:
            print("Notice: Device did not reply to standard query (it might already be in bootloader mode).")

        confirm = input(f"Are you sure you want to flash '{fw_path}' ({format_size(len(fw_data))}) to this device? [y/N]: ")
        if confirm.lower() != "y":
            print("Cancelled.")
            return

        charger.flash_firmware(fw_data, progress_callback=print_progress)


def cmd_upgrade(args):
    print("=== SkyRC Auto-Upgrade ===")
    dev = find_hidraw_device()
    if not dev:
        print("No charger detected. Please connect device via USB.")
        return
    print(f"Found charger at {dev}")
    with SkyRCCharger(dev) as charger:
        info = charger.get_device_info()
        if not info:
            print("Failed to read device info.")
            return

        print(f"Connected: {info['model_name']} [Core: {info['core_type']}, HW: {info['hardware_version']}]")
        print(f"Installed Version: v{info['software_version']}")

        releases = query_firmware(core_type=info['core_type'], hw_ver=info['hardware_version'])
        if not releases:
            print("No releases found on SkyRC server.")
            return

        latest = releases[0]
        latest_ver = float(latest.get("version", 0))
        cur_ver = float(info['software_version'])

        dt_str = format_timestamp(latest.get("timestamp"))
        size_bytes = fetch_size(latest.get("download_url"))

        print(f"Latest Available:  v{latest.get('version')}")
        print(f"Build Date:        {dt_str}")
        if size_bytes:
            print(f"Firmware Size:     {format_size(size_bytes)}")

        if latest_ver <= cur_ver and not args.force:
            print(f"\nDevice is already up to date (v{cur_ver}). Use --force to reinstall.")
            return

        if not args.yes:
            conf = input(f"Proceed with downloading and flashing v{latest.get('version')}? [y/N]: ").strip()
            if conf.lower() != 'y':
                print("Cancelled.")
                return

        print(f"\nDownloading v{latest.get('version')}...")
        req = urllib.request.Request(latest.get("download_url"), headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req) as resp:
            fw_data = resp.read()

        chk = sum(fw_data)
        if chk != int(latest.get("checksum", 0)):
            print("Checksum mismatch! Aborting.")
            return

        charger.flash_firmware(fw_data, progress_callback=print_progress)


def main():
    parser = argparse.ArgumentParser(description="SkyRC Smart Charger Firmware Tool for Linux")
    subparsers = parser.add_subparsers(dest="subcommand")

    # upgrade
    p_upg = subparsers.add_parser("upgrade", help="Auto-detect, download and flash firmware")
    p_upg.add_argument("-f", "--force", action="store_true", help="Force flash if already up to date")
    p_upg.add_argument("-y", "--yes", action="store_true", help="Skip confirmation prompt")
    p_upg.set_defaults(func=cmd_upgrade)

    # check
    p_check = subparsers.add_parser("check", help="Check for firmware updates on SkyRC server")
    p_check.add_argument("--core", help="SkyRC Core ID. If omitted, you will be prompted.")
    p_check.set_defaults(func=cmd_check)

    # download
    p_down = subparsers.add_parser("download", help="Download firmware from SkyRC server")
    p_down.add_argument("--core", help="SkyRC Core ID. If omitted, you will be prompted.")
    p_down.add_argument("--version", help="Specific version (default: prompt or latest)")
    p_down.add_argument("-o", "--output", help="Output filename")
    p_down.set_defaults(func=cmd_download)

    # info
    p_info = subparsers.add_parser("info", help="Read device info from connected USB charger")
    p_info.set_defaults(func=cmd_info)

    # flash
    p_flash = subparsers.add_parser("flash", help="Flash firmware .bin to connected charger")
    p_flash.add_argument("file", help="Path to firmware .bin file")
    p_flash.add_argument("--device", help="Specific /dev/hidrawX device (optional)")
    p_flash.set_defaults(func=cmd_flash)

    args = parser.parse_args()
    if args.subcommand is None:
        cmd_upgrade(argparse.Namespace(force=False, yes=False))
    else:
        args.func(args)


if __name__ == "__main__":
    main()
