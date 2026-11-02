mod api;
mod device;

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Write};
use std::path::PathBuf;

use crate::api::{download_firmware, format_size, get_model_name, query_firmware, KNOWN_MODELS};
use crate::device::SkyRCCharger;

#[derive(Parser)]
#[command(
    name = "skyrc-flasher",
    author = "SkyRC Linux Community",
    version = "1.0.0",
    about = "SkyRC Smart Charger firmware flasher and updater for Linux"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Specific hidraw device path (e.g. /dev/hidraw11)
    #[arg(short, long, global = true)]
    device: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Detect connected device, check for updates, download and flash automatically
    Upgrade {
        /// Force flashing even if already on the latest version
        #[arg(short, long)]
        force: bool,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Display hardware and firmware details of connected charger
    Info,

    /// Query SkyRC server for available firmware versions
    Check {
        /// SkyRC Core ID (e.g. 100198 for B6neo). If omitted, you will be prompted.
        #[arg(short, long)]
        core: Option<String>,
    },

    /// Download firmware binary from SkyRC server
    Download {
        /// SkyRC Core ID (e.g. 100198 for B6neo). If omitted, you will be prompted.
        #[arg(short, long)]
        core: Option<String>,

        /// Specific version to download (if omitted, you will be prompted to choose)
        #[arg(short, long)]
        version: Option<String>,

        /// Output file path (defaults to skyrc_<core>_v<ver>.bin)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Flash a local firmware binary (.bin) to connected charger
    Flash {
        /// Path to firmware binary file
        file: PathBuf,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

fn get_charger(dev_opt: Option<&PathBuf>) -> Result<SkyRCCharger, Box<dyn std::error::Error>> {
    let path = match dev_opt {
        Some(p) => p.clone(),
        None => SkyRCCharger::find_device().ok_or(
            "No SkyRC charger detected on USB (looking for VID 0000: PID 0001).\n\
             Make sure your charger is connected via a USB-C data cable and you have permissions on /dev/hidraw*.\n\
             Tip: Run with sudo or set up udev rules.",
        )?,
    };

    let charger = SkyRCCharger::open(&path).map_err(|e| {
        format!(
            "Failed to open {}: {}.\nTip: Try running with sudo or check udev permissions.",
            path.display(),
            e
        )
    })?;

    Ok(charger)
}

fn prompt_confirm(prompt: &str) -> bool {
    print!("{} [y/N]: ", prompt);
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        return input.trim().eq_ignore_ascii_case("y");
    }
    false
}

fn prompt_for_core(dev_opt: Option<&PathBuf>) -> Result<(String, u8), Box<dyn std::error::Error>> {
    // 1. Check if a device is currently connected
    if let Ok(charger) = get_charger(dev_opt) {
        if let Ok(Some(info)) = charger.get_device_info() {
            println!(
                "Detected connected charger: {} (Core: {}, FW: v{}, HW: {})",
                info.model_name, info.core_type, info.software_version_str, info.hardware_version
            );
            print!("Use detected device? [Y/n]: ");
            let _ = io::stdout().flush();
            let mut line = String::new();
            if io::stdin().read_line(&mut line).is_ok() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("y") {
                    return Ok((info.core_type, info.hardware_version));
                }
            }
        }
    }

    // 2. Prompt user to select from known models or enter manual core ID
    println!("\nSelect a SkyRC device model:");
    for (i, (core, name)) in KNOWN_MODELS.iter().enumerate() {
        println!("  {:2}) {:<26} (Core ID: {})", i + 1, name, core);
    }
    println!("   0) Enter custom Core ID manually");

    loop {
        print!("\nEnter choice [1-{} or Core ID]: ", KNOWN_MODELS.len());
        let _ = io::stdout().flush();
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if let Ok(choice) = trimmed.parse::<usize>() {
            if choice >= 1 && choice <= KNOWN_MODELS.len() {
                return Ok((KNOWN_MODELS[choice - 1].0.to_string(), 1));
            } else if choice == 0 {
                print!("Enter Core ID (e.g. 100198): ");
                let _ = io::stdout().flush();
                let mut manual = String::new();
                io::stdin().read_line(&mut manual)?;
                let manual_trimmed = manual.trim();
                if !manual_trimmed.is_empty() {
                    return Ok((manual_trimmed.to_string(), 1));
                }
            }
        }

        if trimmed.len() >= 4 && trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-') {
            return Ok((trimmed.to_string(), 1));
        }

        println!("Invalid choice, please select a valid number or enter a Core ID.");
    }
}

fn create_progress_bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb
}

fn cmd_info(dev_opt: Option<&PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    println!("Searching for connected SkyRC charger...");
    let charger = get_charger(dev_opt)?;
    println!("Found device at: {}", charger.path.display());

    let info = charger
        .get_device_info()?
        .ok_or("Failed to read device info (is charger powered on and in normal mode?)")?;

    println!("\n=== SkyRC Charger Information ===");
    println!("  Model:            {}", info.model_name);
    println!("  Core ID:          {}", info.core_type);
    println!("  Firmware Version: {}", info.software_version_str);
    println!("  Hardware Version: {}", info.hardware_version);
    println!("  Customer ID:      {}", info.customer_id);
    println!("  Language ID:      {}", info.language_id);
    println!("  Encryption:       {}", if info.is_encrypt { "Yes" } else { "No" });
    println!("  Upgrade Type:     {}", info.upgrade_type);
    Ok(())
}

fn cmd_check(core_opt: Option<String>, dev_opt: Option<&PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let (core, hw_ver) = match core_opt {
        Some(c) => (c, 1),
        None => prompt_for_core(dev_opt)?,
    };

    println!(
        "\nQuerying SkyRC server for core {} ({}) [HW: {}]...",
        core,
        get_model_name(&core),
        hw_ver
    );

    let releases = query_firmware(&core, hw_ver)?;
    if releases.is_empty() {
        println!("No firmware releases found on SkyRC server for this model.");
        return Ok(());
    }

    println!("\nFound {} release(s):", releases.len());
    for r in &releases {
        println!("  - Version:      v{}", r.version);
        println!("    Build Date:   {}", r.release_date());
        if let Some(size) = r.fetch_size() {
            println!("    Size:         {}", format_size(size));
        }
        println!("    Checksum:     {}", r.checksum);
        println!("    URL:          {}", r.download_url);
        let notes = r.release_notes();
        if !notes.is_empty() {
            println!("    Notes:        {}", notes.replace('\r', ""));
        }
        println!();
    }

    Ok(())
}

fn cmd_download(
    core_opt: Option<String>,
    version_opt: Option<String>,
    output_opt: Option<PathBuf>,
    dev_opt: Option<&PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (core, hw_ver) = match core_opt {
        Some(c) => (c, 1),
        None => prompt_for_core(dev_opt)?,
    };

    println!("\nChecking releases for core {} ({})...", core, get_model_name(&core));
    let releases = query_firmware(&core, hw_ver)?;
    if releases.is_empty() {
        return Err(format!("No releases found on SkyRC server for core {}.", core).into());
    }

    let target = match version_opt {
        Some(ref v) => releases
            .iter()
            .find(|r| r.version == *v)
            .ok_or_else(|| format!("Version {} not found for core {}", v, core))?,
        None => {
            if releases.len() == 1 {
                &releases[0]
            } else {
                println!("\nAvailable versions for {}:", get_model_name(&core));
                for (i, rel) in releases.iter().enumerate() {
                    let latest_tag = if i == 0 { " (Latest)" } else { "" };
                    let size_str = rel
                        .fetch_size()
                        .map(|s| format!(" | {}", format_size(s)))
                        .unwrap_or_default();
                    println!(
                        "  {:2}) v{:<6}{} [Date: {}]{}",
                        i + 1,
                        rel.version,
                        latest_tag,
                        rel.release_date(),
                        size_str
                    );
                    let notes = rel.release_notes();
                    if !notes.is_empty() {
                        println!("      Notes: {}", notes.replace('\r', ""));
                    }
                }
                print!("\nSelect version to download [1]: ");
                let _ = io::stdout().flush();
                let mut line = String::new();
                io::stdin().read_line(&mut line)?;
                let choice = line.trim();
                if choice.is_empty() {
                    &releases[0]
                } else if let Ok(idx) = choice.parse::<usize>() {
                    if idx >= 1 && idx <= releases.len() {
                        &releases[idx - 1]
                    } else {
                        println!("Invalid selection, defaulting to latest (v{}).", releases[0].version);
                        &releases[0]
                    }
                } else {
                    releases.iter().find(|r| r.version == choice).unwrap_or(&releases[0])
                }
            }
        }
    };

    let filename = match output_opt {
        Some(p) => p,
        None => PathBuf::from(format!("skyrc_{}_v{}.bin", core, target.version)),
    };

    println!(
        "\nDownloading Version {} [Date: {}] from {} to {}...",
        target.version,
        target.release_date(),
        target.download_url,
        filename.display()
    );

    let data = download_firmware(&target.download_url)?;
    let calculated_checksum: u32 = data.iter().map(|&b| b as u32).sum();
    let expected_checksum: u32 = target.checksum.parse().unwrap_or(0);

    println!("Downloaded {}.", format_size(data.len() as u64));
    println!(
        "Checksum: calculated = {}, expected = {}",
        calculated_checksum, expected_checksum
    );

    if calculated_checksum != expected_checksum {
        return Err("Checksum mismatch! The downloaded file may be corrupted.".into());
    }

    std::fs::write(&filename, data)?;
    println!("Successfully saved to {}!", filename.display());
    Ok(())
}

fn cmd_flash(
    file: PathBuf,
    yes: bool,
    dev_opt: Option<&PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !file.exists() {
        return Err(format!("File not found: {}", file.display()).into());
    }

    let fw_data = std::fs::read(&file)?;
    println!(
        "Loaded firmware file: {} ({})",
        file.display(),
        format_size(fw_data.len() as u64)
    );

    let charger = get_charger(dev_opt)?;
    println!("Found charger at: {}", charger.path.display());

    if let Ok(Some(info)) = charger.get_device_info() {
        println!(
            "Connected: {} (Current FW: {}, HW: {})",
            info.model_name, info.software_version_str, info.hardware_version
        );
    } else {
        println!("Notice: Device did not reply to standard query (it might already be in bootloader mode).");
    }

    if !yes && !prompt_confirm(&format!("Are you sure you want to flash '{}'?", file.display())) {
        println!("Flashing cancelled.");
        return Ok(());
    }

    let pb = create_progress_bar(fw_data.len() as u64);
    charger.flash_firmware(&fw_data, Some(&pb))?;
    Ok(())
}

fn cmd_upgrade(
    force: bool,
    yes: bool,
    dev_opt: Option<&PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== SkyRC Auto-Upgrade ===");
    println!("Detecting connected charger...");
    let charger = get_charger(dev_opt)?;
    println!("Device detected at: {}", charger.path.display());

    let info = charger
        .get_device_info()?
        .ok_or("Failed to query device (is charger powered on and in normal mode?)")?;

    println!(
        "Connected Device: {} [Core: {}, HW: {}]",
        info.model_name, info.core_type, info.hardware_version
    );
    println!("Installed Version: v{}", info.software_version_str);

    println!("\nChecking for updates on SkyRC server...");
    let releases = query_firmware(&info.core_type, info.hardware_version)?;
    if releases.is_empty() {
        println!("No firmware releases found for this model.");
        return Ok(());
    }

    let latest = &releases[0];
    let latest_ver = latest.parsed_version();
    let current_ver = info.software_version;

    println!("Latest Available:  v{}", latest.version);
    println!("Build Date:        {}", latest.release_date());
    if let Some(size) = latest.fetch_size() {
        println!("Firmware Size:     {}", format_size(size));
    }
    let notes = latest.release_notes();
    if !notes.is_empty() {
        println!("Release Notes:     {}", notes.replace('\r', ""));
    }

    if latest_ver <= current_ver && !force {
        println!(
            "\nDevice is already up to date (Installed: v{}, Latest: v{}).",
            info.software_version_str, latest.version
        );
        println!("Use '--force' if you wish to reinstall/reflash the firmware.");
        return Ok(());
    }

    if latest_ver > current_ver {
        println!(
            "\n==> An upgrade is available! (v{} -> v{})",
            info.software_version_str, latest.version
        );
    } else {
        println!("\n==> Reinstalling version v{} (--force active)", latest.version);
    }

    if !yes && !prompt_confirm("Do you want to proceed with downloading and flashing?") {
        println!("Upgrade cancelled.");
        return Ok(());
    }

    println!("\nDownloading firmware v{}...", latest.version);
    let fw_data = download_firmware(&latest.download_url)?;
    println!("Downloaded {}.", format_size(fw_data.len() as u64));

    let calculated_checksum: u32 = fw_data.iter().map(|&b| b as u32).sum();
    let expected_checksum: u32 = latest.checksum.parse().unwrap_or(0);
    if calculated_checksum != expected_checksum {
        return Err(format!(
            "Checksum verification failed! (calculated: {}, expected: {})",
            calculated_checksum, expected_checksum
        )
        .into());
    }
    println!("Checksum verified: {}", calculated_checksum);

    println!("\nStarting firmware flash...");
    let pb = create_progress_bar(fw_data.len() as u64);
    charger.flash_firmware(&fw_data, Some(&pb))?;

    println!("\n=== Upgrade Completed Successfully! ===");
    println!("Your charger has been updated to v{}!", latest.version);
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let dev = cli.device.as_ref();

    let result = match cli.command {
        Some(Commands::Upgrade { force, yes }) => cmd_upgrade(force, yes, dev),
        Some(Commands::Info) => cmd_info(dev),
        Some(Commands::Check { core }) => cmd_check(core, dev),
        Some(Commands::Download {
            core,
            version,
            output,
        }) => cmd_download(core, version, output, dev),
        Some(Commands::Flash { file, yes }) => cmd_flash(file, yes, dev),
        None => {
            // Default action if no subcommand is given: run upgrade
            cmd_upgrade(false, false, dev)
        }
    };

    if let Err(err) = result {
        eprintln!("\nError: {}", err);
        std::process::exit(1);
    }
}
