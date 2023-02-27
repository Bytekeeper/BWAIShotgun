#[cfg(target_os = "windows")]
use anyhow::Context;
#[cfg(target_os = "windows")]
use registry::{Hive, Security};
use std::fs::File;
use std::io::copy;
use std::path::PathBuf;

use hex_literal::hex;
use log::info;

use crate::base_folder;
use crate::setup::{ComponentConfig, ComponentInstallation};

pub fn starcraft_component(config: ComponentConfig) -> ComponentInstallation {
    ComponentInstallation {
        name: "Starcraft 1.16.1",
        download_name: "scbw_bwapi440.zip",
        download_url: "http://www.cs.mun.ca/~dchurchill/startcraft/scbw_bwapi440.zip",
        locator: locate_starcraft,
        config,
        hashes: &[
            // "Original hash"
            hex!("C7FB49E6C170270192ABA1610F25105BF077A52E556B7A4E684484079FA9FA93"),
            // "Hash after 2023-01-25, bwapi.ini was modified
            hex!("4546155ECFEBD50F72DC407041EC0B65282AEFDF083E58F96C29F55B75EB0C0E"),
        ],
        internal_folder: base_folder().join("scbw"),
        provider: provide_starcraft,
    }
}

pub fn provide_starcraft(component: &ComponentInstallation) -> anyhow::Result<PathBuf> {
    if !component.download_and_unzip(false)? {
        return Ok(component.internal_folder.clone());
    }
    info!("Installing SNP_DirectIP.snp");
    copy(
        &mut File::open(base_folder().join("SNP_DirectIP.snp"))?,
        &mut File::create(component.internal_folder.join("SNP_DirectIP.snp"))?,
    )?;
    Ok(component.internal_folder.clone())
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

pub fn starcraft_default_config() -> ComponentConfig {
    #[cfg(target_os = "windows")]
    return ComponentConfig::Locate;
    #[cfg(not(target_os = "windows"))]
    return ComponentConfig::Internal;
}

// impl StarCraftInstallation {
//     pub fn ensure_path(&self) -> anyhow::Result<PathBuf> {
//         match self {
//             StarCraftInstallation::Search => Self::locate_starcraft(),
//             StarCraftInstallation::Internal => {
//                 let scbw_folder = internal_scbw_folder();
//                 if scbw_folder.exists() {
//                     info!("Using internal StarCraft");
//                 } else {
//                     let path = download_folder()?.join("scbw_bwapi440.zip");
//                     let file = if !crate::verify_hashes(&path, &SCBW_ZIP_HASHES)? {
//                         info!(
//                             "Downloading StarCraft 1.16.1 from '{}' to '{}'",
//                             SCBW_URL,
//                             path.to_string_lossy()
//                         );
//                         let mut file = File::create(&path)?;
//                         let dl_bytes = reqwest::blocking::get(SCBW_URL)?.copy_to(&mut file)?;
//                         debug!("Downloaded scbw distribution: {dl_bytes} bytes");
//                         file.sync_data()?;
//                         ensure!(
//                             crate::verify_hashes(&path, &SCBW_ZIP_HASHES)?,
//                             "Hash check of downloaded SCBW failed, aborting!"
//                         );
//                         file
//                     } else {
//                         File::open(&path)?
//                     };

//                     info!(
//                         "Unzipping '{}' to '{}'",
//                         path.to_string_lossy(),
//                         scbw_folder.to_string_lossy()
//                     );
//                     let mut zip = ZipArchive::new(file)?;
//                     for i in 0..zip.len() {
//                         let mut file = zip.by_index(i)?;
//                         let outpath = match file.enclosed_name() {
//                             Some(path) => scbw_folder.join(path),
//                             None => continue,
//                         };
//                         if file.is_dir() {
//                             create_dir_all(&outpath)?;
//                         } else {
//                             if let Some(parent) = outpath.parent() {
//                                 create_dir_all(parent)?;
//                             }
//                             copy(&mut file, &mut File::create(outpath)?)?;
//                         }
//                     }
//                     info!("Installing SNP_DirectIP.snp");
//                     copy(
//                         &mut File::open(base_folder().join("SNP_DirectIP.snp"))?,
//                         &mut File::create(scbw_folder.join("SNP_DirectIP.snp"))?,
//                     )?;
//                     info!("SCBW setup complete");
//                 }
//                 Ok(scbw_folder)
//             }
//             StarCraftInstallation::Path(path) => Ok(path.to_path_buf()),
//         }
//     }
// }
