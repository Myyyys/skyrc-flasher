use std::fs::OpenOptions;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::api::{get_model_name, SKYRC_PID, SKYRC_VID};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub core_type: String,
    pub model_name: String,
    pub upgrade_type: u8,
    pub is_encrypt: bool,
    pub customer_id: u16,
    pub language_id: u8,
    pub software_version: f32,
    pub software_version_str: String,
    pub hardware_version: u8,
    pub reserved: u8,
}

pub struct SkyRCCharger {
    fd: OwnedFd,
    pub path: PathBuf,
}

impl SkyRCCharger {
    pub fn find_device() -> Option<PathBuf> {
        let entries = std::fs::read_dir("/sys/class/hidraw").ok()?;
        for entry in entries.flatten() {
            let uevent_path = entry.path().join("device/uevent");
            if let Ok(content) = std::fs::read_to_string(&uevent_path) {
                for line in content.lines() {
                    if let Some(id_str) = line.strip_prefix("HID_ID=") {
                        let parts: Vec<&str> = id_str.split(':').collect();
                        if parts.len() >= 3 {
                            if let (Ok(vid), Ok(pid)) = (
                                u16::from_str_radix(parts[1], 16),
                                u16::from_str_radix(parts[2], 16),
                            ) {
                                if vid == SKYRC_VID && pid == SKYRC_PID {
                                    let dev_name = entry.file_name();
                                    return Some(PathBuf::from("/dev").join(dev_name));
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub fn open(path: &Path) -> Result<Self, std::io::Error> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;

        // Set non-blocking mode
        let fd = file.as_raw_fd();
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL, 0);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }

        Ok(Self {
            fd: file.into(),
            path: path.to_path_buf(),
        })
    }

    pub fn drain(&self) {
        let mut pollfd = libc::pollfd {
            fd: self.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            let ret = unsafe { libc::poll(&mut pollfd, 1, 50) };
            if ret <= 0 {
                break;
            }
            let mut buf = [0u8; 65];
            let n = unsafe {
                libc::read(
                    self.fd.as_raw_fd(),
                    buf.as_mut_ptr() as *mut libc::c_void,
                    65,
                )
            };
            if n <= 0 {
                break;
            }
        }
    }

    pub fn send_report(&self, data: &[u8]) -> Result<(), std::io::Error> {
        let mut buf = [0u8; 65];
        buf[0] = 0x00; // Report ID 0
        let len = data.len().min(64);
        buf[1..1 + len].copy_from_slice(&data[..len]);

        let n = unsafe {
            libc::write(
                self.fd.as_raw_fd(),
                buf.as_ptr() as *const libc::c_void,
                65,
            )
        };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn read_report(&self, timeout_ms: i32) -> Result<Option<[u8; 65]>, std::io::Error> {
        let mut pollfd = libc::pollfd {
            fd: self.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if ret < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if ret == 0 {
            return Ok(None);
        }

        let mut buf = [0u8; 65];
        let n = unsafe {
            libc::read(
                self.fd.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                65,
            )
        };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }

        let mut pkt = [0u8; 65];
        if n == 64 {
            pkt[1..65].copy_from_slice(&buf[..64]);
        } else if n > 0 {
            let copy_len = (n as usize).min(65);
            pkt[..copy_len].copy_from_slice(&buf[..copy_len]);
        }
        Ok(Some(pkt))
    }

    pub fn get_device_info(&self) -> Result<Option<DeviceInfo>, std::io::Error> {
        self.drain();

        let mut cmd = [0u8; 64];
        cmd[0] = 15; // 0x0F
        cmd[1] = 3;  // length
        cmd[2] = 87; // 0x57
        cmd[3] = 1;  // slot 0
        cmd[4] = 88; // checksum (87 + 1)
        self.send_report(&cmd)?;

        let reply = match self.read_report(2000)? {
            Some(p) => p,
            None => return Ok(None),
        };

        if reply[1] != 15 || reply[3] != 87 {
            return Ok(None);
        }

        let raw_core = &reply[6..12];
        let core_type = String::from_utf8_lossy(raw_core)
            .trim_matches('\0')
            .trim()
            .to_string();

        let upgrade_type = reply[12];
        let is_encrypt = reply[13] != 0;
        let customer_id = ((reply[14] as u16) << 8) | (reply[15] as u16);
        let language_id = reply[16];
        let major = reply[17] as f32;
        let minor = reply[18] as f32;
        let software_version = major + (minor / 100.0);
        let software_version_str = format!("{}.{:02}", reply[17], reply[18]);
        let hardware_version = reply[19];
        let reserved = reply[20];
        let model_name = get_model_name(&core_type).to_string();

        Ok(Some(DeviceInfo {
            core_type,
            model_name,
            upgrade_type,
            is_encrypt,
            customer_id,
            language_id,
            software_version,
            software_version_str,
            hardware_version,
            reserved,
        }))
    }

    pub fn enter_update_mode(&self) -> Result<(), std::io::Error> {
        let mut cmd = [0u8; 64];
        cmd[0] = 15; // 0x0F
        cmd[1] = 2;  // length
        cmd[2] = 96; // 0x60
        cmd[3] = 96; // checksum
        self.send_report(&cmd)
    }

    pub fn flash_firmware(
        &self,
        fw_data: &[u8],
        progress: Option<&indicatif::ProgressBar>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let fw_length = fw_data.len();
        let fw_checksum: u32 = fw_data.iter().map(|&x| x as u32).sum();

        println!("Firmware length: {} bytes, Checksum: {}", fw_length, fw_checksum);
        println!("Switching charger to update mode (CMD 96)...");

        self.drain();
        self.enter_update_mode()?;

        // Wait 4 seconds for charger MCU to enter bootloader
        thread::sleep(Duration::from_millis(4000));
        self.drain();

        // Step 1: Handshake (CMD 1)
        println!("Sending update handshake (CMD 1)...");
        let mut cmd1 = [0u8; 64];
        cmd1[0] = 1;
        cmd1[1] = ((fw_length >> 24) & 0xFF) as u8;
        cmd1[2] = ((fw_length >> 16) & 0xFF) as u8;
        cmd1[3] = ((fw_length >> 8) & 0xFF) as u8;
        cmd1[4] = (fw_length & 0xFF) as u8;
        cmd1[5] = ((fw_checksum >> 24) & 0xFF) as u8;
        cmd1[6] = ((fw_checksum >> 16) & 0xFF) as u8;
        cmd1[7] = ((fw_checksum >> 8) & 0xFF) as u8;
        cmd1[8] = (fw_checksum & 0xFF) as u8;
        self.send_report(&cmd1)?;

        let ack1 = self
            .read_report(6000)?
            .ok_or("Timeout waiting for handshake ACK (bootloader did not respond)")?;

        if ack1[1] != 1 {
            return Err(format!("Handshake failed: bootloader returned byte[1] = {}", ack1[1]).into());
        }

        for j in 1..9 {
            if ack1[j] != cmd1[j - 1] {
                return Err(format!(
                    "Handshake mismatch at byte {}: expected {}, got {}",
                    j, cmd1[j - 1], ack1[j]
                )
                .into());
            }
        }

        println!("Handshake acknowledged! Flashing blocks...");

        // Step 2: Transfer 1024-byte blocks
        let mut updata_index = 0usize;
        while updata_index < fw_length {
            let send_length = 1024.min(fw_length - updata_index);

            // Announce block (CMD 2)
            let mut cmd2 = [0u8; 64];
            cmd2[0] = 2;
            cmd2[1] = ((updata_index >> 24) & 0xFF) as u8;
            cmd2[2] = ((updata_index >> 16) & 0xFF) as u8;
            cmd2[3] = ((updata_index >> 8) & 0xFF) as u8;
            cmd2[4] = (updata_index & 0xFF) as u8;
            cmd2[5] = ((send_length >> 8) & 0xFF) as u8;
            cmd2[6] = (send_length & 0xFF) as u8;
            self.send_report(&cmd2)?;

            let ack2 = self
                .read_report(5000)?
                .ok_or_else(|| format!("Timeout waiting for block announce at offset {}", updata_index))?;

            if ack2[1] != 2 || ack2[8] != 1 {
                return Err(format!(
                    "Block announce failed at offset {} (cmd={}, ack={})",
                    updata_index, ack2[1], ack2[8]
                )
                .into());
            }

            // Stream block slices (CMD 3)
            let block_slice = &fw_data[updata_index..updata_index + send_length];
            let mut slice_offset = 0usize;

            for _ in 0..34 {
                let mut cmd3 = [0u8; 64];
                cmd3[0] = 3;
                if slice_offset < block_slice.len() {
                    let s_len = 31.min(block_slice.len() - slice_offset);
                    cmd3[1..1 + s_len].copy_from_slice(&block_slice[slice_offset..slice_offset + s_len]);
                    slice_offset += s_len;
                }
                self.send_report(&cmd3)?;
                thread::sleep(Duration::from_micros(1500)); // 1.5ms pacing for MCU buffer
            }

            updata_index += send_length;
            if let Some(pb) = progress {
                pb.inc(send_length as u64);
            }
        }

        if let Some(pb) = progress {
            pb.finish_with_message("Done");
        }

        // Step 3: Finalize & Verify (CMD 4)
        println!("Verifying written firmware (CMD 4)...");
        self.send_report(&[4])?;

        let ack4 = self
            .read_report(10000)?
            .ok_or("Timeout waiting for firmware verification")?;

        if ack4[1] != 4 || ack4[2] != 1 {
            return Err(format!(
                "Verification failed: device reported error (byte[1]={}, byte[2]={})",
                ack4[1], ack4[2]
            )
            .into());
        }

        println!("Firmware verified successfully by device!");

        // Step 4: Reboot device (CMD 5)
        println!("Rebooting device (CMD 5)...");
        self.send_report(&[5])?;
        thread::sleep(Duration::from_millis(3000));
        println!("Upgrade complete! Charger rebooted successfully.");

        Ok(())
    }
}
