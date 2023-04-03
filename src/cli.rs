use crate::{BotLaunchConfig, GameConfig, HeadfulMode};
use clap::{error::ErrorKind, Parser, Subcommand};

#[derive(Subcommand, Debug)]
enum GameType {
    /// Host a melee game
    Melee {
        /// Names of bots to play
        bots: Vec<String>,
    },
    /// You will host a game the bots can join (make sure to select Local PC network)
    Human {
        /// Names of bots to play
        bots: Vec<String>,
    },
}

#[derive(Parser, Debug)]
pub struct Cli {
    /// Absolute path of map to host
    #[arg(short, long)]
    map: Option<String>,
    #[clap(subcommand)]
    game_type: Option<GameType>,
    #[arg(short = 's', long)]
    human_speed: Option<bool>,
    /// Folder/File name to use for replays
    #[arg(long)]
    replay_path: Option<String>,
}

pub enum Error {
    ClapError(clap::Error),
}

impl Cli {
    pub fn merge_into(self, mut config: GameConfig) -> Result<GameConfig, Error> {
        if self.map.is_some() != self.game_type.is_some() {
            Err(Error::ClapError(clap::Error::raw(
                ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand,
                "Map and game type must either be both set, or not at all. Use '-h' to get help.\n",
            )))
        } else {
            if let Some(game_type) = self.game_type {
                config.human_host = matches!(game_type, GameType::Human { .. });
                config.game_type = match game_type {
                    GameType::Melee { bots } | GameType::Human { bots } => crate::GameType::Melee(
                        bots.iter()
                            .map(|name| BotLaunchConfig {
                                name: name.to_string(),
                                player_name: None,
                                race: None,
                                headful: HeadfulMode::Off,
                            })
                            .collect(),
                    ),
                };
            }
            if let Some(map) = self.map {
                config.map = Some(map);
            }
            if let Some(human_speed) = self.human_speed {
                config.human_speed = human_speed;
            }
            if let Some(replay_path) = self.replay_path {
                config.replay_path = Some(replay_path);
            }
            Ok(config)
        }
    }
}
