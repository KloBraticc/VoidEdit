use std::io::Read;
use std::time::Duration;

use sha2::{Digest, Sha256};

const LATEST: &str = "https://api.github.com/repos/KloBraticc/VoidEdit/releases/latest";
const DOWNLOAD_PREFIX: &str = "https://github.com/KloBraticc/VoidEdit/releases/download/";
const MAX_BYTES: u64 = 256 * 1024 * 1024;

pub const ASSET: &str = if cfg!(target_os = "windows") {
    "voidedit-windows-x64.exe"
} else if cfg!(target_os = "macos") {
    "voidedit-macos-arm64"
} else {
    "voidedit-linux-x64"
};

pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(a), Some(b)) if a > b)
}

pub fn select_asset(release: &serde_json::Value, name: &str) -> Option<(String, String)> {
    release["assets"].as_array()?.iter().find(|asset| asset["name"] == name).and_then(|asset| {
        let url = asset["browser_download_url"].as_str()?;
        let digest = asset["digest"].as_str()?.strip_prefix("sha256:")?;
        (url.starts_with(DOWNLOAD_PREFIX) && digest.len() == 64).then(|| (url.to_string(), digest.to_ascii_lowercase()))
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn verify(data: &[u8], digest: &str) -> bool {
    hex(&Sha256::digest(data)) == digest
}

pub fn check_and_install(current: &str) -> Result<Option<String>, String> {
    if cfg!(debug_assertions) {
        return Ok(None);
    }
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(120)).user_agent("VoidEdit").build();
    let release: serde_json::Value = agent.get(LATEST).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let tag = release["tag_name"].as_str().ok_or("the release has no tag")?;
    if !is_newer(tag, current) {
        return Ok(None);
    }
    let (url, digest) = select_asset(&release, ASSET).ok_or("the release has no verified file for this system")?;
    let response = agent.get(&url).call().map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    response.into_reader().take(MAX_BYTES).read_to_end(&mut data).map_err(|e| e.to_string())?;
    if !verify(&data, &digest) {
        return Err("the download failed its integrity check".into());
    }
    let temporary = std::env::temp_dir().join(format!("voidedit-update-{}", std::process::id()));
    std::fs::write(&temporary, &data).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let result = self_replace::self_replace(&temporary).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&temporary);
    result?;
    Ok(Some(tag.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("nonsense", "0.1.0"));
    }

    #[test]
    fn only_trusted_assets_are_selected() {
        let digest = "a".repeat(64);
        let good = serde_json::json!({"assets":[{"name":"x","browser_download_url":format!("{DOWNLOAD_PREFIX}v1/x"),"digest":format!("sha256:{digest}")}]});
        assert!(select_asset(&good, "x").is_some());
        let bad = serde_json::json!({"assets":[{"name":"x","browser_download_url":"https://evil.example/x","digest":format!("sha256:{digest}")}]});
        assert!(select_asset(&bad, "x").is_none());
        let unsigned = serde_json::json!({"assets":[{"name":"x","browser_download_url":format!("{DOWNLOAD_PREFIX}v1/x")}]});
        assert!(select_asset(&unsigned, "x").is_none());
    }

    #[test]
    fn digest_mismatch_is_refused() {
        assert!(!verify(b"data", &"0".repeat(64)));
    }
}
