mod game_table;
use crate::game_table::*;

fn main() {
    #[cfg(target_os = "windows")]
    GameTableAccess::new()
        .get_game_table()
        .and_then(|out| serde_json::to_string(&out).ok())
        .map(|out| println!("{out}"));
}
