use log::debug;
use serde::Deserialize;
use std::fs::{create_dir_all, File};
use std::io::copy;
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context};
use hex_literal::hex;
use log::info;
#[cfg(target_os = "windows")]
use registry::{Hive, Security};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::{base_folder, download_folder, internal_scbw_folder};

const SCBW_URL: &str = "http://www.cs.mun.ca/~dchurchill/startcraft/scbw_bwapi440.zip";
const SCBW_ZIP_HASHES: [[u8; 32]; 2] = [
    // "Original hash"
    hex!("C7FB49E6C170270192ABA1610F25105BF077A52E556B7A4E684484079FA9FA93"),
    // "Hash after 2023-01-25, bwapi.ini was modified
    hex!("4546155ECFEBD50F72DC407041EC0B65282AEFDF083E58F96C29F55B75EB0C0E"),
];

#[derive(Deserialize, Debug)]
pub enum StarCraftInstallation {
    Search,
    Internal,
    Path(PathBuf),
}

impl Default for StarCraftInstallation {
    fn default() -> Self {
        #[cfg(target_os = "windows")]
        return Self::Search;
        #[cfg(not(target_os = "windows"))]
        return Self::Internal;
    }
}

impl StarCraftInstallation {
    pub fn ensure_path(&self) -> anyhow::Result<PathBuf> {
        match self {
            StarCraftInstallation::Search => Self::locate_starcraft(),
            StarCraftInstallation::Internal => {
                let scbw_folder = internal_scbw_folder();
                if scbw_folder.exists() {
                    info!("Using internal StarCraft");
                } else {
                    let path = download_folder()?.join("scbw_bwapi440.zip");
                    let file = if !Self::check_scbw_zip_hash(&path)? {
                        info!(
                            "Downloading StarCraft 1.16.1 from '{}' to '{}'",
                            SCBW_URL,
                            path.to_string_lossy()
                        );
                        let mut file = File::create(&path)?;
                        let dl_bytes = reqwest::blocking::get(SCBW_URL)?.copy_to(&mut file)?;
                        debug!("Downloaded scbw distribution: {dl_bytes} bytes");
                        file.sync_data()?;
                        ensure!(
                            Self::check_scbw_zip_hash(&path)?,
                            "Hash check of downloaded SCBW failed, aborting!"
                        );
                        file
                    } else {
                        File::open(&path)?
                    };

                    info!(
                        "Unzipping '{}' to '{}'",
                        path.to_string_lossy(),
                        scbw_folder.to_string_lossy()
                    );
                    let mut zip = ZipArchive::new(file)?;
                    for i in 0..zip.len() {
                        let mut file = zip.by_index(i)?;
                        let outpath = match file.enclosed_name() {
                            Some(path) => scbw_folder.join(path),
                            None => continue,
                        };
                        if file.is_dir() {
                            create_dir_all(&outpath)?;
                        } else {
                            if let Some(parent) = outpath.parent() {
                                create_dir_all(parent)?;
                            }
                            copy(&mut file, &mut File::create(outpath)?)?;
                        }
                    }
                    info!("Installing SNP_DirectIP.snp");
                    copy(
                        &mut File::open(base_folder().join("SNP_DirectIP.snp"))?,
                        &mut File::create(scbw_folder.join("SNP_DirectIP.snp"))?,
                    )?;
                    info!("SCBW setup complete");
                }
                Ok(scbw_folder)
            }
            StarCraftInstallation::Path(path) => Ok(path.to_path_buf()),
        }
    }

    fn locate_starcraft() -> anyhow::Result<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            Ok(Hive::LocalMachine
                .open(r"SOFTWARE\Blizzard Entertainment\Starcraft", Security::Read)
                .context("Could not find Starcraft installation")?
                .value("InstallPath")?
                .to_string()
                .into())
        }
        #[cfg(not(target_os = "windows"))]
        anyhow::bail!("Only supported in Windows")
    }

    fn check_scbw_zip_hash(file: &Path) -> anyhow::Result<bool> {
        let mut file = if let Ok(file) = File::open(file) {
            file
        } else {
            return Ok(false);
        };
        let mut hasher = Sha256::new();
        copy(&mut file, &mut hasher)?;
        let hash = hasher.finalize();
        Ok(SCBW_ZIP_HASHES.contains(hash.as_ref()))
    }
}
