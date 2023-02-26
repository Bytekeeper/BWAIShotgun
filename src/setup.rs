use log::debug;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::copy;
use std::path::{Path, PathBuf};

use anyhow::ensure;
use log::info;
use zip::ZipArchive;

use crate::download_folder;

#[derive(Deserialize, Debug, Default)]
pub enum ComponentConfig {
    #[default]
    Locate,
    Internal,
    Path(PathBuf),
}

pub struct ComponentInstallation {
    pub name: &'static str,
    pub download_name: &'static str,
    pub locator: fn() -> anyhow::Result<PathBuf>,
    pub provider: fn(&Self) -> anyhow::Result<PathBuf>,
    pub internal_folder: PathBuf,
    pub download_url: &'static str,
    pub hashes: &'static [[u8; 32]],
    pub config: ComponentConfig,
}

impl ComponentInstallation {
    pub fn download_and_unzip(&self, skip_zip_root: bool) -> anyhow::Result<bool> {
        if self.internal_folder.exists() {
            debug!("Using internal {}", self.name);
            return Ok(false);
        }
        let path = download_folder()?.join(self.download_name);
        let file = if !verify_hashes(&path, self.hashes)? {
            info!(
                "Downloading {} from '{}' to '{}'",
                self.name,
                self.download_url,
                path.to_string_lossy()
            );
            let mut file = OpenOptions::new()
                .write(true)
                .read(true)
                .create_new(true)
                .open(&path)?;
            let dl_bytes = reqwest::blocking::get(self.download_url)?.copy_to(&mut file)?;
            debug!("Downloaded {} distribution: {dl_bytes} bytes", self.name);
            file.sync_data()?;
            ensure!(
                verify_hashes(&path, self.hashes)?,
                "Hash check of downloaded {} failed, aborting!",
                self.name
            );
            file
        } else {
            File::open(&path)?
        };
        info!(
            "Unzipping '{}' to '{}'",
            path.to_string_lossy(),
            self.internal_folder.to_string_lossy()
        );
        let mut zip = ZipArchive::new(file)?;
        for i in 0..zip.len() {
            let mut file = zip.by_index(i)?;
            let outpath = match file.enclosed_name() {
                Some(path) => self.internal_folder.join(if skip_zip_root {
                    let mut components = path.components();
                    components.next();
                    components.as_path()
                } else {
                    path
                }),
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
        Ok(true)
    }

    pub fn to_path(&self) -> anyhow::Result<PathBuf> {
        match &self.config {
            ComponentConfig::Locate => (self.locator)(),
            ComponentConfig::Path(path) => Ok(path.clone()),
            ComponentConfig::Internal => {
                // info!("{} setup complete", self.name);
                (self.provider)(self)
            }
        }
    }
}

fn verify_hashes(file: &Path, hashes: &[[u8; 32]]) -> anyhow::Result<bool> {
    let mut file = if let Ok(file) = File::open(file) {
        file
    } else {
        return Ok(false);
    };
    let mut hasher = Sha256::new();
    copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(hashes.contains(hash.as_ref()))
}
