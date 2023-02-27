#[cfg(target_os = "windows")]
use log::debug;
#[cfg(target_os = "windows")]
use shared_memory::*;
#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
pub struct GameTableAccess {
    game_table: Option<Shmem>,
}

#[cfg(target_os = "windows")]
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
            match shmmem {
                Ok(table) => self.game_table = Some(table),
                Err(err) => debug!("{}", err),
            }
        }
        self.game_table
            .as_ref()
            .map(|shmem| unsafe { *(shmem.as_ptr() as *const GameTable) })
    }
}
