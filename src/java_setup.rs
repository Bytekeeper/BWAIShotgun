use crate::base_folder;
use crate::setup::{ComponentConfig, ComponentInstallation};
use hex_literal::hex;
use std::path::PathBuf;

pub fn java_component(config: ComponentConfig) -> ComponentInstallation {
    ComponentInstallation {
        name: "Java 8 JRE",
        download_name: "jre.zip",
        download_url: "https://github.com/adoptium/temurin8-binaries/releases/download/jdk8u362-b09/OpenJDK8U-jre_x86-32_windows_hotspot_8u362b09.zip",
        locator: || Ok(PathBuf::from("javaw.exe")),
        provider: |component| component.download_and_unzip(true).map(|_| component.internal_folder.join("bin").join("javaw.exe")),
        config,
        hashes: &[hex!("ab1c3756c0f94e982edf77e7048263d2c7fc1048c57dd1185e5f441f007e9653") ],
        internal_folder: base_folder().join("jre"),
    }
}

pub fn java_default_config() -> ComponentConfig {
    #[cfg(target_os = "windows")]
    return ComponentConfig::Locate;
    #[cfg(not(target_os = "windows"))]
    return ComponentConfig::Internal;
}
