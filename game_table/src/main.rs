mod game_table;
use crate::game_table::*;
use std::io::Write;

fn main() {
    #[cfg(target_os = "windows")]
    GameTableAccess::new().get_game_table().map(|out| {
        std::io::stdout().write(unsafe {
            &std::mem::transmute::<GameTable, [u8; std::mem::size_of::<GameTable>()]>(out)[..]
        })
    });
}
