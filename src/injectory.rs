use std::fs::{copy, create_dir_all, File};
use std::process::Command;

use anyhow::ensure;

use crate::botsetup::{BotSetup, LaunchBuilder};
use crate::{tools_folder, AutoMenu, BwapiConnectMode, BwapiIni, GameConfig};

pub enum InjectoryConnectMode {
    Host {
        map: Option<String>,
        player_count: usize,
    },
    Join,
}

pub struct Injectory {
    pub bot_setup: BotSetup,
    pub game_name: String,
    pub connect_mode: InjectoryConnectMode,
    pub wmode: bool,
    pub sound: bool,
    pub game_speed: i32,
}

impl LaunchBuilder for Injectory {
    fn build_command(&self, _game_config: &GameConfig) -> anyhow::Result<Command> {
        ensure!(
            self.bot_setup.starcraft_exe.exists(),
            "Could not find 'StarCraft.exe'"
        );
        let bwapi_data = self.bot_setup.bot_base_path.join("bwapi-data");
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
        let injectory = tools_folder().join("injectory_x86.exe");
        ensure!(
            injectory.exists(),
            r"Could not find '{}'. Please make sure to extract all files, or check your antivirus software.",
            tools_folder().to_string_lossy()
        );
        let bwapi_ini = bwapi_data.join("bwapi.ini");
        let mut bwapi_ini_file = File::create(&bwapi_ini)?;
        BwapiIni {
            auto_menu: match &self.connect_mode {
                InjectoryConnectMode::Host { map, player_count } => AutoMenu::AutoMenu {
                    name: self.bot_setup.player_name.clone(),
                    game_name: self.game_name.clone(),
                    race: self.bot_setup.race,
                    connect_mode: BwapiConnectMode::Host {
                        map: map.clone(),
                        player_count: *player_count,
                    },
                },
                InjectoryConnectMode::Join => AutoMenu::AutoMenu {
                    name: self.bot_setup.player_name.clone(),
                    game_name: self.game_name.clone(),
                    race: self.bot_setup.race,
                    connect_mode: BwapiConnectMode::Join,
                },
            },
            game_speed: self.game_speed,
            sound: self.sound,
            tm_module: self.bot_setup.tournament_module.clone(),
            ..BwapiIni::from(&self.bot_setup)
        }
        .write(&mut bwapi_ini_file)?;

        // BWAPI will look for the map in the "bot" folder, not in the starcraft path, so we'll copy the map over.
        // We really need to copy, because it will open the map to check for settings.
        // One caveat: BWAPI does not allow game speed selection, so this might host with an invalid game speed
        if let InjectoryConnectMode::Host { map: Some(map), .. } = &self.connect_mode {
            let original_map = self.bot_setup.starcraft_path.join(map);
            ensure!(
                original_map.exists(),
                "Map '{}' does not exist",
                original_map.to_string_lossy()
            );
            let tmp_map = self.bot_setup.bot_base_path.join(map);
            create_dir_all(tmp_map.parent().expect("Map file has no parent directory"))?;
            copy(original_map, tmp_map)?;
        }

        let mut cmd = self.bot_setup.wrapper.wrap_executable(injectory);
        cmd.arg("-l").arg(&self.bot_setup.starcraft_exe);
        cmd.arg("-i")
            .args([tools_folder().join("oldbwapi.dll"), bwapi_dll]);
        if self.wmode {
            cmd.arg(tools_folder().join("WMode.dll"));
        }
        cmd.arg("--wait-for-exit").arg("--kill-on-exit");
        // Newer versions of BWAPI no longer use the registry key (aka installpath) - but allow overriding the bwapi_ini location.
        // Note that injectory does NOT do any registry trickery (bwheadless does) - so old bots (< 4.x) will most likely not work.
        cmd.env("BWAPI_CONFIG_INI", &*bwapi_ini.to_string_lossy());

        // Old versions of BWAPI need a hack: We replace the value returned from the registry query with this path:
        cmd.env("BWAISHOTGUN_INSTALLPATH", &self.bot_setup.bot_base_path);
        cmd.current_dir(&self.bot_setup.bot_base_path);
        Ok(cmd)
    }
}
