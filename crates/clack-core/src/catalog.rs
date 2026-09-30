use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::config::data_dir;

const INDEX_URL: &str =
    "https://github.com/DarrylMathias/clack-packs/releases/latest/download/index.json";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub family: String,
    pub license: String,
    #[serde(default)]
    pub starter: bool,
    pub size: u64,
    pub sha256: String,
    pub url: String,
    pub blurb: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub packs: Vec<CatalogEntry>,
}

fn cache_path() -> PathBuf {
    data_dir().join("index.json")
}

pub fn fetch_catalog() -> Result<Catalog> {
    let cache = cache_path();
    let mut needs_fetch = true;

    if cache.exists() {
        if let Ok(metadata) = fs::metadata(&cache) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(age) = SystemTime::now().duration_since(modified) {
                    if age < CACHE_TTL {
                        needs_fetch = false;
                    }
                }
            }
        }
    }

    if needs_fetch {
        let url = std::env::var("CLACK_INDEX_URL").unwrap_or_else(|_| INDEX_URL.to_string());
        debug!("fetching catalog from {url}");

        match ureq::get(&url).call() {
            Ok(resp) => {
                let text = resp.into_string().context("reading catalog response")?;
                if let Ok(cat) = serde_json::from_str::<Catalog>(&text) {
                    // Try to save to cache, but don't fail if we can't
                    if let Some(parent) = cache.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if let Err(e) = fs::write(&cache, text) {
                        warn!("failed to cache catalog: {e}");
                    }
                    return Ok(cat);
                }
            }
            Err(e) => {
                warn!("failed to fetch catalog: {e}");
            }
        }
    }

    // Fallback to cache
    if cache.exists() {
        debug!("falling back to cached catalog");
        let text = fs::read_to_string(&cache).context("reading cached catalog")?;
        let cat: Catalog = serde_json::from_str(&text).context("parsing cached catalog")?;
        return Ok(cat);
    }

    anyhow::bail!("catalog not available offline and no cache exists")
}

pub fn install_pack(entry: &CatalogEntry) -> Result<()> {
    use flate2::read::GzDecoder;
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let dest_dir = crate::config::packs_dir().join(&entry.id);
    if dest_dir.exists() {
        return Ok(());
    }

    println!("Downloading {}...", entry.id);
    let resp = ureq::get(&entry.url).call().context("downloading pack")?;
    let mut reader = resp.into_reader();

    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;

    let hash = format!("{:x}", Sha256::digest(&bytes));
    if hash != entry.sha256 {
        anyhow::bail!("sha256 mismatch for {}", entry.id);
    }

    let temp_dir = std::env::temp_dir().join(format!("clack-pack-{}", entry.id));
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)?;
    }

    let tar = GzDecoder::new(std::io::Cursor::new(bytes));
    let mut archive = tar::Archive::new(tar);
    archive.unpack(&temp_dir).context("unpacking pack")?;

    fs::rename(&temp_dir, &dest_dir).context("moving pack to final destination")?;

    Ok(())
}
