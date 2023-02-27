mod game_table;

pub use crate::game_table::GameTable;
#[cfg(target_os = "windows")]
pub use crate::game_table::GameTableAccess;
