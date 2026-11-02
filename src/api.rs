use serde::Deserialize;

pub const SKYRC_VID: u16 = 0x0000;
pub const SKYRC_PID: u16 = 0x0001;

pub const KNOWN_MODELS: &[(&str, &str)] = &[
    ("100198", "B6neo / X1 NANO USB"),
    ("100200", "B6ACneo / X1 NANO CUBE"),
    ("100211", "B6neo+ / X1 NANO USB+"),
    ("100225", "B6neo 2"),
    ("100182", "T1000 Maestro / V1000"),
    ("100196", "D200neo / X2 800"),
    ("100197", "Q200neo / X4 AC PLUS 400"),
    ("100199", "D100neo / X2 AC Plus 200"),
    ("100202", "S100neo / X1 AC PLUS IV"),
    ("100203", "D200neo / GMA6800SP"),
    ("100223", "Q100neo / Pocket Base"),
    ("100213", "GMA620MP / RC-S100neo"),
    ("100220", "PC1080neo / 1080 UAV Charger"),
    ("100221", "PC3020-1"),
    ("100222", "YZ-114PLUS"),
    ("100226", "PC1500neo"),
    ("100227", "PC3020-2"),
    ("100231", "PC7200"),
    ("100236", "CH003 / X2 V Pocket"),
    ("100237", "CH005 / X1 Pocket EX"),
    ("100239", "CH007"),
    ("100201", "D750 MIX"),
    ("100185", "NC2500 Pro"),
    ("100084", "MC3000"),
];

pub fn get_model_name(core_type: &str) -> &'static str {
    match core_type {
        "100182" => "T1000 Maestro / V1000",
        "100196" => "D200neo / X2 800",
        "100197" => "Q200neo / X4 AC PLUS 400",
        "100198" => "B6neo / X1 NANO USB",
        "100199" => "D100neo / X2 AC Plus 200",
        "100200" => "B6ACneo / X1 NANO CUBE",
        "100201" => "D750 MIX",
        "100202" => "S100neo / X1 AC PLUS IV",
        "100203" => "D200neo / GMA6800SP",
        "100211" => "B6neo+ / X1 NANO USB+",
        "100213" => "GMA620MP / RC-S100neo",
        "100220" => "PC1080neo / 1080 UAV Charger",
        "100221" => "PC3020-1",
        "100222" => "YZ-114PLUS",
        "100223" => "Q100neo / Pocket Base",
        "100225" => "B6neo 2",
        "100226" => "PC1500neo",
        "100227" => "PC3020-2",
        "100231" => "PC7200",
        "100236" => "CH003 / X2 V Pocket",
        "100237" => "CH005 / X1 Pocket EX",
        "100239" => "CH007",
        _ => "Unknown SkyRC Model",
    }
}

pub fn format_timestamp(ts: i64) -> String {
    if ts <= 0 {
        return "Unknown".to_string();
    }
    let mut days = ts / 86400;
    let day_seconds = (ts % 86400) as u32;
    let hours = day_seconds / 3600;
    let minutes = (day_seconds % 3600) / 60;

    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = (days - era * 146097) as u32;
    let yoe = (doe - doe / 1024 + doe / 1460 - doe / 36524) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", y, m, d, hours, minutes)
}

pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB ({} bytes)", bytes as f64 / (1024.0 * 1024.0), bytes)
    } else if bytes >= 1024 {
        format!("{:.1} KB ({} bytes)", bytes as f64 / 1024.0, bytes)
    } else {
        format!("{} bytes", bytes)
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct FirmwareMemo {
    pub en: Option<String>,
    pub cn: Option<String>,
    pub jp: Option<String>,
    pub tw: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct FirmwareRelease {
    pub id: String,
    pub timestamp: Option<serde_json::Value>,
    pub version: String,
    pub memo: Option<FirmwareMemo>,
    pub force: Option<String>,
    pub checksum: String,
    pub download_url: String,
}

impl FirmwareRelease {
    pub fn parsed_version(&self) -> f32 {
        self.version.parse::<f32>().unwrap_or(0.0)
    }

    pub fn release_date(&self) -> String {
        let ts: i64 = match self.timestamp {
            Some(serde_json::Value::Number(ref n)) => n.as_i64().unwrap_or(0),
            Some(serde_json::Value::String(ref s)) => s.parse().unwrap_or(0),
            _ => 0,
        };
        format_timestamp(ts)
    }

    pub fn fetch_size(&self) -> Option<u64> {
        let resp = ureq::head(&self.download_url)
            .header("User-Agent", "Mozilla/5.0 (SkyRC Updater Rust)")
            .call()
            .ok()?;
        let cl = resp.headers().get("content-length")?;
        let s = cl.to_str().ok()?;
        s.parse::<u64>().ok()
    }

    pub fn release_notes(&self) -> &str {
        if let Some(ref memo) = self.memo {
            if let Some(ref en) = memo.en {
                if !en.trim().is_empty() {
                    return en.trim();
                }
            }
            if let Some(ref cn) = memo.cn {
                if !cn.trim().is_empty() {
                    return cn.trim();
                }
            }
        }
        ""
    }
}

pub fn make_sn(
    core_type: &str,
    upgrade_type: u8,
    is_encrypt: u8,
    customer_id: u16,
    language_id: u8,
    sw_ver_major: u8,
    sw_ver_minor: u8,
    hw_ver: u8,
    reserved: u8,
) -> String {
    let mut b = Vec::with_capacity(16);
    let mut core_bytes = [0u8; 6];
    let bytes = core_type.as_bytes();
    let copy_len = bytes.len().min(6);
    core_bytes[..copy_len].copy_from_slice(&bytes[..copy_len]);

    b.extend_from_slice(&core_bytes);
    b.push(upgrade_type);
    b.push(is_encrypt);
    b.push(((customer_id >> 8) & 0xFF) as u8);
    b.push((customer_id & 0xFF) as u8);
    b.push(language_id);
    b.push(sw_ver_major);
    b.push(sw_ver_minor);
    b.push(hw_ver);
    b.push(reserved);

    let sum: u32 = b.iter().map(|&x| x as u32).sum();
    let chk = (((!(sum as u8)) as u16 + 1) & 0xFF) as u8;
    b.push(chk);

    b.iter().map(|x| format!("{:02x}", x)).collect()
}

pub fn query_firmware(core_type: &str, hw_ver: u8) -> Result<Vec<FirmwareRelease>, Box<dyn std::error::Error>> {
    let sn = make_sn(core_type, 1, 0, 0, 0, 1, 0, hw_ver, 0);
    let url = format!("http://upgrade.skyrc.com/?SN={}&test=0", sn);

    let mut resp = ureq::get(&url)
        .header("User-Agent", "Mozilla/5.0 (SkyRC Updater Rust)")
        .call()?;

    let text = resp.body_mut().read_to_string()?;
    if text.trim().is_empty() || text.trim() == "[]" {
        return Ok(Vec::new());
    }

    let releases: Vec<FirmwareRelease> = serde_json::from_str(&text)?;
    Ok(releases)
}

pub fn download_firmware(url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut resp = ureq::get(url)
        .header("User-Agent", "Mozilla/5.0 (SkyRC Updater Rust)")
        .call()?;

    let data = resp.body_mut().read_to_vec()?;
    Ok(data)
}
