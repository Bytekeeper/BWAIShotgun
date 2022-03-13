use crate::botsetup::LaunchBuilder;
use crate::{tools_folder, Binary, BwapiIni, Race};
use anyhow::{anyhow, ensure};
use std::fs::File;
use std::path::PathBuf;
use std::process::Command;

pub enum BwHeadlessConnectMode {
    Host { map: String, player_count: usize },
    Join,
}

pub struct BwHeadless {
    pub starcraft_exe: PathBuf,
    /// Folder containing bwapi-data/AI
    pub bot_base_path: PathBuf,
    pub bot_name: String,
    pub race: Race,
    pub game_name: Option<String>,
    pub connect_mode: BwHeadlessConnectMode,
}

impl LaunchBuilder for BwHeadless {
    fn build_command(&self, bot_binary: &Binary) -> anyhow::Result<Command> {
        ensure!(
            self.starcraft_exe.exists(),
            "Could not find 'StarCraft.exe'"
        );
        let bwapi_data = self.bot_base_path.join("bwapi-data");
        ensure!(
            bwapi_data.exists(),
            "Missing '{}' - please read the instructions on how to setup a bot.",
            bwapi_data.to_string_lossy()
        );
        let bwapi_dll = bwapi_data.join("BWAPI.dll");
        ensure!(
            bwapi_dll.exists(),
            "Could not find '{}'",
            bwapi_dll.to_string_lossy()
        );

        let bwheadless = tools_folder().join("bwheadless.exe");
        ensure!(
            bwheadless.exists(),
            r"Could not find '{}'. Please make sure to extract all files, or check your antivirus software.",
            tools_folder().to_string_lossy()
        );
        let bwapi_ini = bwapi_data.join("bwapi.ini");
        let mut bwapi_ini_file = File::create(&bwapi_ini)?;
        BwapiIni {
            ai_module: Some(match &bot_binary {
                Binary::Dll(x) => x.to_string_lossy().to_string(),
                Binary::Exe(_) | Binary::Jar(_) => "".to_string(),
            }),
            ..Default::default()
        }
        .write(&mut bwapi_ini_file)?;

        let mut cmd = Command::new(bwheadless);
        cmd.arg("-e").arg(&self.starcraft_exe);
        if let Some(game_name) = &self.game_name {
            cmd.arg("-g").arg(game_name);
        }
        cmd.arg("-r").arg(&self.race.to_string());
        cmd.arg("-l").arg(bwapi_dll);
        cmd.arg("--installpath").arg(&self.bot_base_path);
        cmd.arg("-n").arg(&self.bot_name);
        // Newer versions of BWAPI no longer use the registry key (aka installpath) - but allow overriding the bwapi_ini location.
        cmd.env("BWAPI_CONFIG_INI", &*bwapi_ini.to_string_lossy());
        cmd.current_dir(&self.bot_base_path);
        let starcraft_path = self
            .starcraft_exe
            .parent()
            .ok_or(anyhow!("Folder containing 'StarCraft.exe' not found"))?;

        match &self.connect_mode {
            BwHeadlessConnectMode::Host { map, player_count } => {
                cmd.arg("-m").arg(starcraft_path.join(map));
                cmd.arg("-h").arg(player_count.to_string());
            }
            BwHeadlessConnectMode::Join => {}
        }
        Ok(cmd)
    }
}
