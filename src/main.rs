mod botsetup;
mod bwapi;
mod bwheadless;
mod cli;
mod injectory;

use anyhow::{anyhow, bail};
use clap::Parser;
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::fs::{create_dir_all, metadata, read, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use crate::botsetup::{Binary, LaunchBuilder};
use crate::bwapi::{AutoMenu, BwapiConnectMode, BwapiIni, GameTableAccess};
use crate::bwheadless::{BwHeadless, BwHeadlessConnectMode};
use crate::cli::Cli;
use crate::injectory::{Injectory, InjectoryConnectMode};
use registry::{Hive, Security};
use retry::delay::Fixed;
use retry::{retry, OperationResult};
use serde::de::Unexpected;
use serde::{Deserialize, Deserializer};

#[derive(Deserialize, Debug, Default)]
struct ShotgunConfig {
    starcraft_path: Option<String>,
    java_path: Option<String>,
}

impl ShotgunConfig {
    fn get_starcraft_path(&self) -> anyhow::Result<PathBuf> {
        let path = if let Some(path) = &self.starcraft_path {
            path.to_owned()
        } else {
            Hive::LocalMachine
                .open(r"SOFTWARE\Blizzard Entertainment\Starcraft", Security::Read)?
                .value("InstallPath")?
                .to_string()
        };
        Ok(path.into())
    }
}

#[derive(Deserialize, Debug)]
pub struct BotConfig {
    pub name: String,
    pub player_name: Option<String>,
    pub race: Option<Race>,
    #[serde(default)]
    pub headful: bool,
}

#[derive(Deserialize, Debug)]
pub enum GameType {
    Melee(Vec<BotConfig>),
}

#[derive(Deserialize, Debug)]
pub struct GameConfig {
    pub map: String,
    pub game_name: Option<String>,
    pub game_type: GameType,
    #[serde(default)]
    pub human_host: bool,
    #[serde(default)]
    pub human_speed: bool,
}

impl GameConfig {
    fn load(config: &ShotgunConfig) -> anyhow::Result<GameConfig> {
        let result: GameConfig =
            toml::from_slice(read(base_folder().join("game.toml"))?.as_slice())
                .map_err(|e| e.to_string())
                .expect("'game.toml' is invalid");
        if result.map.is_empty() && !result.human_host {
            bail!("Map must be set for non-human hosted games");
        }
        let map_path_abs = Path::new(&result.map);
        let map_path_rel = config
            .get_starcraft_path()
            .expect("Could not find StarCraft installation")
            .join(&result.map);
        if map_path_abs.is_absolute() && !map_path_abs.exists() | !map_path_rel.exists() {
            bail!("Could not find map '{}'", result.map);
        }
        Ok(result)
    }
}

#[derive(Deserialize, Debug)]
struct BotDefinition {
    race: Race,
    executable: Option<String>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Race {
    Protoss,
    Terran,
    Zerg,
    Random,
}

impl<'d> Deserialize<'d> for Race {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'d>,
    {
        match String::deserialize(deserializer)?.to_lowercase().as_str() {
            "r" | "random" => Ok(Race::Random),
            "p" | "protoss" => Ok(Race::Protoss),
            "z" | "zerg" => Ok(Race::Zerg),
            "t" | "terran" => Ok(Race::Terran),
            x => Err(serde::de::Error::invalid_value(
                Unexpected::Str(x),
                &"One of Zerg/Protoss/Terran/Random or z/p/t/r",
            )),
        }
    }
}

impl Display for Race {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Race::Protoss => "Protoss",
                Race::Terran => "Terran",
                Race::Zerg => "Zerg",
                Race::Random => "Random",
            }
        )
    }
}

#[derive(Debug)]
pub enum SGError {
    MissingStarCraftExe(PathBuf),
    MultipleExecutables(PathBuf),
    ExecutableNotFound(PathBuf),
}

impl Error for SGError {}

impl Display for SGError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SGError::MissingStarCraftExe(path) => {
                write!(
                    f,
                    "StarCraft.exe not found here: {}",
                    path.to_str().expect("Path not found")
                )
            }
            SGError::MultipleExecutables(path) => write!(
                f,
                "Multiple bot executables found in '{}', please specify executable in 'bot.toml'",
                path.to_str().expect("Path not found")
            ),
            SGError::ExecutableNotFound(path) => write!(
                f,
                "No bot executable found in '{}'",
                path.to_str().expect("Path not found")
            ),
        }
    }
}

/// bwaishotgun base folder
pub fn base_folder() -> PathBuf {
    std::env::current_exe()
        .expect("Could not find executable")
        .parent()
        .expect("BWAIShotgun folder does not exist")
        .to_owned()
}

/// tools folder
pub fn tools_folder() -> PathBuf {
    base_folder().join("tools")
}

pub struct BotProcess {
    bwheadless: Child,
    bot: Option<Child>,
}

#[derive(Debug)]
pub struct PreparedBot {
    binary: Binary,
    race: Race,
    name: String,
    working_dir: PathBuf,
    log_dir: PathBuf,
    headful: bool,
}

impl PreparedBot {
    fn prepare(config: &BotConfig, path: &Path, definition: &BotDefinition) -> Self {
        let bwapi_data_path = path.join("bwapi-data");
        // Workaround BWAPI 3.7.x "strangeness" of removing ":" ...
        let mut ai_module_path = bwapi_data_path.components();
        ai_module_path.next();
        let ai_module_path = ai_module_path.as_path().join("AI");
        let read_path = bwapi_data_path.join("read");
        let write_path = bwapi_data_path.join("write");
        let log_dir = path.join("logs");
        create_dir_all(read_path).expect("Could not create read folder");
        create_dir_all(write_path).expect("Could not create write folder");
        create_dir_all(&log_dir).expect("Colud not create log folder");

        let bot_binary = definition
            .executable
            .as_deref()
            .and_then(|s| Binary::from_path(Path::new(s)))
            .unwrap_or_else(|| {
                Binary::search(ai_module_path.as_path())
                    .expect("Could not find bot binary in 'bwapi-data/AI'")
            });
        let race = config.race.unwrap_or(definition.race);

        Self {
            binary: bot_binary,
            race,
            name: config
                .player_name
                .clone()
                .unwrap_or_else(|| config.name.clone()),
            working_dir: path.to_path_buf(),
            log_dir,
            headful: config.headful,
        }
    }
}

fn main() -> anyhow::Result<()> {
    let config: ShotgunConfig = read(base_folder().join("shotgun.toml"))
        .map(|cfg| {
            toml::from_slice(cfg.as_slice())
                .map_err(|e| e.to_string())
                .expect("Invalid config in 'shotgun.toml'")
        })
        .unwrap_or_else(|_| {
            eprintln!("shotgun.toml not found, using defaults");
            ShotgunConfig::default()
        });

    let cli = Cli::parse();

    let game_config: Result<GameConfig, cli::Error> = cli.try_into();
    let game_config = match game_config {
        Ok(game_config) => game_config,
        Err(cli::Error::NoArguments) => {
            GameConfig::load(&config).expect("Could not load 'game.toml'")
        }
        Err(cli::Error::ClapError(err)) => err.exit(),
    };

    let starcraft_path = config
        .get_starcraft_path()
        .expect("Could not find StarCraft installation");
    let starcraft_exe = starcraft_path.join("StarCraft.exe");
    if let Ok(metadata) = metadata(starcraft_path.join("SNP_DirectIP.snp")) {
        if metadata.len() != 46100 {
            eprintln!("The 'SNP_DirectIP.snp' in your StarCraft installation might not support more than ~6 bots per game. Overwrite with the included 'SNP_DirectIP.snp' file to support more.");
        }
    } else {
        eprintln!("Could not find 'SNP_DirectIP.snp' in your StarCraft installation, please copy the provided one or install BWAPI.");
    }

    let mut game_table_access = GameTableAccess::new();
    for server_process_id in game_table_access
        .get_game_table()
        .iter()
        .flat_map(|table| table.game_instances.iter())
        .filter(|it| it.is_connected && it.server_process_id != 0)
        .map(|it| it.server_process_id)
    {
        eprintln!(
            "The process {} is in the game table already and will interfere with game creation.",
            server_process_id
        );
    }

    match game_config.game_type {
        GameType::Melee(bots) => {
            let bots: Vec<_> = bots
                .iter()
                .map(|cfg| {
                    let mut bot_folder = base_folder();
                    bot_folder.push("bots");
                    bot_folder.push(&cfg.name);
                    let bot_definition = toml::from_slice::<BotDefinition>(
                        read(bot_folder.join("bot.toml"))
                            .map_err(|_| {
                                format!(
                                    "Could not read 'bot.toml' for bot '{}' in: '{}'",
                                    cfg.name,
                                    bot_folder.to_string_lossy()
                                )
                            })
                            .expect("Folder for bot not found")
                            .as_slice(),
                    )
                    .map_err(|e| e.to_string())
                    .expect("Could not read 'bot.toml'");
                    if let Some(race) = &cfg.race {
                        if bot_definition.race != Race::Random && &bot_definition.race != race {
                            println!(
                                "Bot '{}' is configured to play as {}, but its default race is {}!",
                                cfg.name, race, bot_definition.race
                            );
                        }
                    }
                    (cfg, bot_folder, bot_definition)
                })
                .collect();
            let player_count = bots.len();
            let mut prepared_bots: Vec<_> = bots
                .iter()
                .map(|(config, path, definition)| PreparedBot::prepare(config, path, definition))
                .collect();
            prepared_bots.sort_by_key(|bot| {
                if matches!(bot.binary, Binary::Dll(_)) {
                    1
                } else {
                    0
                }
            });

            let mut bot_names = HashSet::new();
            for bot in prepared_bots.iter().map(|it| &it.name) {
                if !bot_names.insert(bot) {
                    println!("'{}' was added multiple times. All instances will use the same read/write/log folders and could fail to work properly. Also headful mode will not work as expected.", bot);
                }
            }
            let mut instances = vec![];
            // If a human is going to host, no need to fire up a host
            let mut host = !game_config.human_host;
            // Game name is mutable, BWAPI can't create games with names differing from the player name in LAN
            let mut game_name = game_config
                .game_name
                .as_deref()
                .unwrap_or("shotgun")
                .to_string();
            for bot in prepared_bots {
                let bwapi_launcher: Box<dyn LaunchBuilder> = if bot.headful {
                    if host {
                        // Headful + Host => All other bots need to join the game with this bots player name
                        game_name = bot.name.clone();
                    }
                    Box::new(Injectory {
                        starcraft_exe: starcraft_exe.clone(),
                        bot_base_path: bot.working_dir.clone(),
                        player_name: bot.name.clone(),
                        game_name: game_name.clone(),
                        race: bot.race,
                        connect_mode: if host {
                            InjectoryConnectMode::Host {
                                map: game_config.map.clone(),
                                player_count,
                            }
                        } else {
                            InjectoryConnectMode::Join
                        },
                        wmode: true,
                        game_speed: if game_config.human_speed { -1 } else { 0 },
                    })
                } else {
                    Box::new(BwHeadless {
                        starcraft_exe: starcraft_exe.clone(),
                        bot_base_path: bot.working_dir.clone(),
                        bot_name: bot.name.clone(),
                        race: bot.race,
                        game_name: game_name.clone(),
                        connect_mode: if host {
                            BwHeadlessConnectMode::Host {
                                map: game_config.map.clone(),
                                player_count,
                            }
                        } else {
                            BwHeadlessConnectMode::Join
                        },
                    })
                };
                if host {
                    println!("Hosting game with '{}'", bot.name);
                } else {
                    println!("Joining game with '{}'", bot.name);
                }
                host = false;

                let old_connected_client_count =
                    if matches!(bot.binary, Binary::Exe(_) | Binary::Dll(_)) {
                        game_table_access.get_connected_client_count()
                    } else {
                        0
                    };
                let mut cmd = bwapi_launcher.build_command(&bot.binary)?;
                cmd.stdout(File::create(bot.log_dir.join("game_out.log"))?)
                    .stderr(File::create(bot.log_dir.join("game_err.log"))?);
                let mut bwapi_child = cmd
                    .spawn()
                    .expect("Could not run bwheadless (maybe deleted/blocked by a Virus Scanner?)");

                let bot_out_log = File::create(bot.log_dir.join("bot_out.log"))
                    .expect("Could not create bot output log");
                let bot_err_log = File::create(bot.log_dir.join("bot_err.log"))
                    .expect("Could not create bot error log");
                let mut bot_process = None;
                match bot.binary {
                    Binary::Dll(_) => (), // Loaded by BWAPI
                    Binary::Jar(jar) => {
                        let mut cmd =
                            Command::new(config.java_path.as_deref().unwrap_or("java.exe"));
                        cmd.current_dir(bot.working_dir);
                        cmd.arg("-jar").arg(jar);
                        cmd.stdout(bot_out_log);
                        cmd.stderr(bot_err_log);

                        let mut child = cmd.spawn()?;

                        // Wait up to 10 seconds before bailing
                        retry(Fixed::from_millis(100).take(100), || {
                            let found = game_table_access.get_connected_client_count()
                                > old_connected_client_count;
                            if !matches!(bwapi_child.try_wait(), Ok(None)) {
                                OperationResult::Err("BWAPI process died")
                            } else if !matches!(child.try_wait(), Ok(None)) {
                                OperationResult::Err("Bot process died")
                            } else if found {
                                OperationResult::Ok(())
                            } else {
                                OperationResult::Retry(
                                    "Bot client executable did not connect to BWAPI server",
                                )
                            }
                        })
                        .map_err(|e| anyhow!(e))?;
                        bot_process = Some(child);

                        // Give some time for
                    }
                    Binary::Exe(exe) => {
                        let mut cmd = Command::new(exe);
                        cmd.current_dir(bot.working_dir);
                        cmd.stdout(bot_out_log);
                        cmd.stderr(bot_err_log);

                        let mut child = cmd.spawn()?;

                        // Wait up to 10 seconds before bailing
                        retry(Fixed::from_millis(100).take(100), || {
                            let found = game_table_access.get_connected_client_count()
                                > old_connected_client_count;
                            if !matches!(bwapi_child.try_wait(), Ok(None)) {
                                OperationResult::Err("BWAPI process died")
                            } else if !matches!(child.try_wait(), Ok(None)) {
                                OperationResult::Err("Bot process died")
                            } else if found {
                                OperationResult::Ok(())
                            } else {
                                OperationResult::Retry(
                                    "Bot client executable did not connect to BWAPI server",
                                )
                            }
                        })
                        .map_err(|e| anyhow!(e))?;
                        bot_process = Some(child);
                    }
                }
                instances.push(BotProcess {
                    bwheadless: bwapi_child,
                    bot: bot_process,
                });
            }

            // Clean up a bit, kill Client bots to prevent them from spamming the slot table
            // They will also print "Client And Server are not compatible" - if different versions of BWAPI are running with multiple clients
            while !instances.is_empty() {
                for i in (0..instances.len()).rev() {
                    let BotProcess {
                        ref mut bwheadless,
                        ref mut bot,
                    } = instances[i];
                    let remove = matches!(bwheadless.try_wait(), Ok(Some(_)));
                    if remove {
                        if let Some(ref mut bot) = bot {
                            bot.kill().ok();
                        }
                        instances.swap_remove(i);
                        println!("{} bots remaining", instances.len());
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            println!("Done");
            Ok(())
        }
    }
}
