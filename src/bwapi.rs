use crate::{Binary, Race};
use shared_memory::*;
use std::io::Write;
use std::mem::size_of;

#[derive(Debug, Eq, PartialEq)]
pub enum BwapiVersion {
    Bwapi375,
    Bwapi412,
    Bwapi420,
    Bwapi440,
}

impl BwapiVersion {
    pub fn from_u32(crc: u32) -> Option<BwapiVersion> {
        match crc {
            0x71CB208B => Some(Self::Bwapi440),
            0xD1E0DDDF => Some(Self::Bwapi420),
            0x267BD0D5 => Some(Self::Bwapi412),
            0x4E39C88A => Some(Self::Bwapi375),
            0x41128276 => Some(Self::Bwapi375),
            _ => None,
        }
    }

    pub fn version_short(&self) -> &'static str {
        match self {
            Self::Bwapi375 => "375",
            Self::Bwapi412 => "412",
            Self::Bwapi420 => "420",
            Self::Bwapi440 => "440",
        }
    }
}

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
    pub ai_module: String,
    pub tm_module: Option<String>,
    // default: 0 - full throttle
    pub game_speed: i32,
    pub auto_menu: AutoMenu,
}

impl BwapiIni {
    pub fn with_binary(mut self, binary: &Binary) -> Self {
        self.ai_module = match binary {
            Binary::Dll(x) => x.to_string_lossy().to_string(),
            Binary::Exe(_) | Binary::Jar(_) => "".to_string(),
        };
        self
    }

    pub fn write(&self, out: &mut impl Write) -> std::io::Result<()> {
        writeln!(out, "[ai]")?;
        writeln!(out, "ai = {}", self.ai_module)?;
        if let Some(tm) = &self.tm_module {
            writeln!(out, "tournament = {}", tm)?;
        }
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

#[cfg(test)]
mod test {
    use crate::bwapi::BwapiVersion;
    use crate::bwapi::BwapiVersion::{Bwapi375, Bwapi412, Bwapi420, Bwapi440};
    use crc::{Crc, CRC_32_ISO_HDLC};

    #[test]
    fn test_crc() {
        let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
        let chksum = crc.checksum(
            std::fs::read("test-resources/BWAPI440.dll")
                .unwrap()
                .as_slice(),
        );
        assert_eq!(BwapiVersion::from_u32(chksum), Some(Bwapi440));
        let chksum = crc.checksum(
            std::fs::read("test-resources/BWAPI420.dll")
                .unwrap()
                .as_slice(),
        );
        assert_eq!(BwapiVersion::from_u32(chksum), Some(Bwapi420));
        let chksum = crc.checksum(
            std::fs::read("test-resources/BWAPI412.dll")
                .unwrap()
                .as_slice(),
        );
        assert_eq!(BwapiVersion::from_u32(chksum), Some(Bwapi412));
        let chksum = crc.checksum(
            std::fs::read("test-resources/BWAPI375.dll")
                .unwrap()
                .as_slice(),
        );
        assert_eq!(BwapiVersion::from_u32(chksum), Some(Bwapi375));
        // BWAPI 375 is a replacement for 374
        let chksum = crc.checksum(
            std::fs::read("test-resources/BWAPI374.dll")
                .unwrap()
                .as_slice(),
        );
        assert_eq!(BwapiVersion::from_u32(chksum), Some(Bwapi375));
    }
}
