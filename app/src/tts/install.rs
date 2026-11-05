//! Downloads the official pocket-tts binary: pinned release, SHA-256 verified.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

const RELEASE_TAG: &str = "v4.0.1";
const VERSION: &str = "3.3.0";

#[derive(Clone, Copy)]
enum Archive {
    Zip,
    TarGz,
}

struct Asset {
    target: &'static str,
    archive: Archive,
    sha256: &'static str,
}

/// Digests published by GitHub for the release assets.
fn current_asset() -> Option<Asset> {
    let (target, archive, sha256) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => (
            "x86_64-pc-windows-msvc",
            Archive::Zip,
            "98753041f2e65744b989776f87bccb93f1f574a08afe1890c5427ec94cd6309c",
        ),
        ("linux", "x86_64") => (
            "x86_64-unknown-linux-gnu",
            Archive::TarGz,
            "9abff0aa138a804e44052952d8c3bdaae247fb267ca8f4b83fe178897d1a0559",
        ),
        ("linux", "aarch64") => (
            "aarch64-unknown-linux-gnu",
            Archive::TarGz,
            "2b9a727bffd86ea6d73370d675cbd91a7db118f11b7f5a60343c2d5da12b187a",
        ),
        ("macos", "x86_64") => (
            "x86_64-apple-darwin",
            Archive::TarGz,
            "f04baa94d4fc47a689cc9f11ca2b0ba869dc002f9e78e15d387c6ecaeb1d654b",
        ),
        ("macos", "aarch64") => (
            "aarch64-apple-darwin",
            Archive::TarGz,
            "ee9ca6c7c000ee72c5846e3a8ba01d08f56636d94a614d2259c3f12b8fd69afa",
        ),
        _ => return None,
    };
    Some(Asset { target, archive, sha256 })
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "pocket-tts.exe" } else { "pocket-tts" }
}

/// Returns the path of the pinned pocket-tts binary, downloading it on first use.
pub fn ensure_binary(bin_dir: &Path) -> Result<PathBuf> {
    let dir = bin_dir.join(format!("pocket-tts-{VERSION}"));
    let path = dir.join(exe_name());
    if path.is_file() {
        return Ok(path);
    }
    let asset = current_asset().with_context(|| {
        format!(
            "no official pocket-tts build for {}-{}: set [pocket] binary in the config",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let extension = match asset.archive {
        Archive::Zip => "zip",
        Archive::TarGz => "tar.gz",
    };
    let url = format!(
        "https://github.com/kyutai-labs/pocket-tts/releases/download/{RELEASE_TAG}/pocket-tts-{VERSION}-{}.{extension}",
        asset.target
    );
    tracing::info!(%url, "downloading pocket-tts");
    let bytes = ureq::get(&url)
        .call()
        .with_context(|| format!("cannot download {url}"))?
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .with_context(|| format!("cannot download {url}"))?;

    let digest: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if digest != asset.sha256 {
        bail!("checksum mismatch for {url}: expected {}, got {digest}", asset.sha256);
    }

    // Extract next to the final location, then rename: no half-written binary on failure.
    let staging = bin_dir.join(format!(".pocket-tts-{VERSION}.partial"));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    extract(asset.archive, &bytes, &staging)?;
    if !staging.join(exe_name()).is_file() {
        bail!("{} not found in {url}", exe_name());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(staging.join(exe_name()), std::fs::Permissions::from_mode(0o755))?;
    }
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::rename(&staging, &dir)?;
    tracing::info!(path = %path.display(), "pocket-tts installed");
    Ok(path)
}

/// Keeps only the executable and its licence, flattened (no paths from the archive are used).
fn extract(archive: Archive, bytes: &[u8], dest: &Path) -> Result<()> {
    let wanted = |name: &str| name == exe_name() || name == "LICENSE";
    match archive {
        Archive::Zip => {
            let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i)?;
                let name = entry.name()?.rsplit('/').next().unwrap_or("").to_string();
                if entry.is_file() && wanted(&name) {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data)?;
                    std::fs::write(dest.join(&name), data)?;
                }
            }
        }
        Archive::TarGz => {
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(bytes)));
            for entry in tar.entries()? {
                let mut entry = entry?;
                let name = entry
                    .path()?
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if entry.header().entry_type().is_file() && wanted(&name) {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data)?;
                    std::fs::write(dest.join(&name), data)?;
                }
            }
        }
    }
    Ok(())
}
