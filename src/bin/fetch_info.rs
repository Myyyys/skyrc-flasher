#![allow(dead_code, unused_imports)]

use std::env;
use std::fs;
use std::path::Path;

// Re-use functions from the skyrc-flasher library crate
#[path = "../api.rs"]
mod api;

use api::{format_size, format_timestamp, get_model_name, query_firmware};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let target = if args.len() > 1 {
        args[1].trim().to_string()
    } else {
        println!("SkyRC Firmware Binary Info Fetcher (Rust)");
        println!("Usage: cargo run --bin fetch_info -- <core_id | url | local_file.bin>");
        println!("\nExamples:");
        println!("  cargo run --bin fetch_info -- 100198");
        println!("  cargo run --bin fetch_info -- http://upgrade.skyrc.com/download/3901-0200-23_V217_UPDATEROM.bin.0025400f9805eca2dbd865765613d4f9");
        println!("  cargo run --bin fetch_info -- b6neo_v2.17.bin\n");
        println!("Defaulting to Core ID 100198 (B6neo)...\n");
        "100198".to_string()
    };

    if target.starts_with("http://") || target.starts_with("https://") {
        // Query direct URL via HTTP HEAD
        println!("Querying URL: {}", target);
        let resp = ureq::head(&target)
            .header("User-Agent", "Mozilla/5.0 (SkyRC Updater Rust)")
            .call()?;

        let size_str = if let Some(cl) = resp.headers().get("content-length") {
            let bytes = cl.to_str()?.parse::<u64>().unwrap_or(0);
            format_size(bytes)
        } else {
            "Unknown".to_string()
        };

        let date_str = if let Some(lm) = resp.headers().get("last-modified") {
            lm.to_str()?.to_string()
        } else {
            "Unknown".to_string()
        };

        println!("--------------------------------------------------");
        println!("  URL:           {}", target);
        println!("  Firmware Size: {}", size_str);
        println!("  Last-Modified: {}", date_str);
        if let Some(etag) = resp.headers().get("etag") {
            println!("  ETag:          {}", etag.to_str()?);
        }
        println!("--------------------------------------------------");
    } else if Path::new(&target).exists() && Path::new(&target).is_file() {
        // Inspect local binary file
        let meta = fs::metadata(&target)?;
        let size_bytes = meta.len();
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| format_timestamp(d.as_secs() as i64))
            .unwrap_or_else(|| "Unknown".to_string());

        // Calculate 32-bit checksum
        let data = fs::read(&target)?;
        let checksum: u32 = data.iter().map(|&b| b as u32).sum();

        println!("--------------------------------------------------");
        println!("  Local File:    {}", target);
        println!("  File Size:     {}", format_size(size_bytes));
        println!("  File Modified: {}", modified);
        println!("  32-bit Chksum: {}", checksum);
        println!("--------------------------------------------------");
    } else {
        // Query SkyRC server by Core ID
        let core = target;
        let model_name = get_model_name(&core);
        println!("Querying SkyRC API for Core ID: {} ({})", core, model_name);

        let releases = query_firmware(&core, 1)?;
        if releases.is_empty() {
            println!("No firmware releases found for Core ID {}.", core);
            return Ok(());
        }

        println!("\nFound {} release(s):", releases.len());
        for (i, r) in releases.iter().enumerate() {
            let size = r.fetch_size().map(format_size).unwrap_or_else(|| "Unknown".to_string());
            println!("  [{}] Version:      v{}", i + 1, r.version);
            println!("      Build Date:   {}", r.release_date());
            println!("      Binary Size:  {}", size);
            println!("      Checksum:     {}", r.checksum);
            println!("      Download URL: {}", r.download_url);
            let notes = r.release_notes();
            if !notes.is_empty() {
                println!("      Release Note: {}", notes.replace('\r', ""));
            }
            println!();
        }
    }

    Ok(())
}
