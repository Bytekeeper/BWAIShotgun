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
    human_speed: bool,
}

pub enum Error {
    NoArguments,
    ClapError(clap::Error),
}

impl TryFrom<Cli> for GameConfig {
    type Error = Error;

    fn try_from(cli: Cli) -> Result<Self, Self::Error> {
        if cli.map.is_none() && cli.game_type.is_none() {
            Err(Error::NoArguments)
        } else if cli.map.is_some() != cli.game_type.is_some() {
            Err(Error::ClapError(clap::Error::raw(
                ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand,
                "Either no or all arguments are required. Use '-h' to get help.\n",
            )))
        } else {
            let game_type = match cli.game_type.as_ref().expect("Game Type not set") {
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
            Ok(GameConfig {
                map: cli.map,
                game_name: None,
                game_type,
                human_host: matches!(cli.game_type.unwrap(), GameType::Human { .. }),
                human_speed: cli.human_speed,
                latency_frames: 3,
                time_out_at_frame: None,
            })
        }
    }
}
