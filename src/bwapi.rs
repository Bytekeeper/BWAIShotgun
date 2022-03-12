use crate::Race;
use shared_memory::*;
use std::io::Write;
use std::mem::size_of;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GameInstance {
    pub server_process_id: u32,
    pub is_connected: bool,
    pub last_keep_alive_time: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GameTable {
    pub game_instances: [GameInstance; 8],
}

pub struct GameTableAccess {
    pub game_table: Option<Shmem>,
}

impl GameTableAccess {
    pub fn new() -> Self {
        Self { game_table: None }
    }

    pub fn get_game_table(&mut self) -> Option<GameTable> {
        if self.game_table.is_none() {
            let shmmem = ShmemConf::new()
                .size(size_of::<GameTable>())
                .allow_raw(true)
                .os_id(r"Local\bwapi_shared_memory_game_list")
                .open();
            self.game_table = shmmem.ok();
        }
        self.game_table
            .as_ref()
            .map(|shmem| unsafe { *(shmem.as_ptr() as *const GameTable) })
    }

    pub fn get_connected_client_count(&mut self) -> usize {
        self.get_game_table()
            .map(|table| {
                table
                    .game_instances
                    .iter()
                    .filter(|it| it.is_connected)
                    .count()
            })
            .unwrap_or(0)
    }
}

pub enum BwapiConnectMode {
    Host { map: String, player_count: usize },
    Join,
}

pub enum AutoMenu {
    // Managed by bwheadless
    Unused,
    // Managed by BWAPI + injectory
    AutoMenu {
        name: String,
        race: Race,
        game_name: String,
        connect_mode: BwapiConnectMode,
    },
}

impl Default for AutoMenu {
    fn default() -> Self {
        Self::Unused
    }
}

/// Although BWAPI can manage multiple bots with one BWAPI.ini, we'll be using one per bot
#[derive(Default)]
pub struct BwapiIni {
    pub ai_module: Option<String>,
    // default: 0 - full throttle
    pub game_speed: i32,
    pub auto_menu: AutoMenu,
}

impl BwapiIni {
    pub fn write(&self, out: &mut impl Write) -> std::io::Result<()> {
        writeln!(out, "[ai]")?;
        writeln!(out, "ai = {}", self.ai_module.as_deref().unwrap_or(""))?;
        writeln!(out, "[auto_menu]")?;
        match &self.auto_menu {
            AutoMenu::Unused => (),
            AutoMenu::AutoMenu {
                name,
                race,
                game_name,
                connect_mode,
            } => {
                writeln!(out, "auto_menu=LAN")?;
                writeln!(out, "lan_mode=Local PC")?;
                writeln!(out, "character_name={}", name)?;
                writeln!(out, "race={}", race)?;
                match connect_mode {
                    BwapiConnectMode::Host { map, player_count } => {
                        writeln!(out, "map={}", map)?;
                        writeln!(out, "wait_for_min_players={}", player_count)?;
                        writeln!(out, "wait_for_max_players={}", player_count)?;
                    }
                    BwapiConnectMode::Join => {
                        writeln!(out, "game={}", game_name)?;
                    }
                }
            }
        }
        writeln!(
            out,
            "save_replay = replays/$Y $b $d/%MAP%_%BOTRACE%%ALLYRACES%vs%ENEMYRACES%_$H$M$S.rep"
        )?;
        writeln!(out, "[starcraft]")?;
        writeln!(out, "speed_override = {}", self.game_speed)?;
        writeln!(out, "sound = OFF")
    }
}
