mod bwapi;
mod cli;

use anyhow::bail;
use clap::Parser;
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::fs::{create_dir_all, metadata, read, read_dir, File};
use std::io::Write;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use crate::bwapi::GameTableAccess;
use crate::cli::Cli;
use registry::{Hive, Security};
use retry::delay::Fixed;
use retry::retry;
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
}

impl GameConfig {
    fn load(config: &ShotgunConfig) -> Result<Self, String> {
        let result: GameConfig = toml::from_slice(
            read(base_folder().join("game.toml"))
                .map_err(|e| e.to_string())
                .expect("'game.toml' not found")
                .as_slice(),
        )
        .map_err(|e| e.to_string())
        .expect("'game.toml' is invalid");
        if result.map.is_empty() && !result.human_host {
            return Err("Map must be set for non-human hosted games".to_owned());
        }
        let map_path_abs = Path::new(&result.map);
        let map_path_rel = config
            .get_starcraft_path()
            .expect("Could not find StarCraft installation")
            .join(&result.map);
        if map_path_abs.is_absolute() && !map_path_abs.exists() | !map_path_rel.exists() {
            return Err(format!("Could not find map '{}'", result.map));
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

struct BwapiIni {
    ai: String,
}

impl BwapiIni {
    fn write(&self, out: &mut impl Write) -> std::io::Result<()> {
        writeln!(out, "[ai]")?;
        writeln!(out, "ai = {}", self.ai)?;
        writeln!(out, "[auto_menu]")?;
        writeln!(
            out,
            "save_replay = replays/$Y $b $d/%MAP%_%BOTRACE%%ALLYRACES%vs%ENEMYRACES%_$H$M$S.rep"
        )?;
        writeln!(out, "[starcraft]")?;
        writeln!(out, "speed_override = 0")
    }
}
#[derive(Debug)]
enum SGError {
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

struct BwHeadless {
    starcraft_exe: PathBuf,
}

impl BwHeadless {
    fn new(starcraft_exe: &Path) -> Result<Self, SGError> {
        if !&starcraft_exe.exists() {
            return Err(SGError::MissingStarCraftExe(starcraft_exe.to_path_buf()));
        }
        Ok(Self {
            starcraft_exe: starcraft_exe.to_path_buf(),
        })
    }

    fn host_command(
        &self,
        map: &str,
        player_count: usize,
        game_name: &str,
    ) -> std::io::Result<Command> {
        let mut cmd = self.bwheadless_command()?;
        cmd.arg("-m").arg(map);
        cmd.arg("-h").arg(player_count.to_string());
        cmd.arg("-g").arg(game_name);
        Ok(cmd)
    }

    fn join_command(&self) -> std::io::Result<Command> {
        self.bwheadless_command()
    }

    fn bwheadless_command(&self) -> std::io::Result<Command> {
        let mut bwheadless = base_folder();
        bwheadless.push("bwheadless.exe");
        let mut cmd = Command::new(bwheadless);
        cmd.arg("-e").arg(&self.starcraft_exe);
        Ok(cmd)
    }

    fn add_bot_args(
        cmd: &mut Command,
        race: Race,
        bot_name: &str,
        bwapi_dll_path: &str,
        bot_bwapi_path: &str,
    ) {
        cmd.arg("-r").arg(race.to_string());
        cmd.arg("-l").arg(bwapi_dll_path);
        cmd.arg("--installpath").arg(&bot_bwapi_path);
        cmd.arg("-n").arg(bot_name);
        // Newer versions of BWAPI no longer use the registry key (aka installpath) - but allow overriding the bwapi_ini location
        let ini_path = PathBuf::from(bot_bwapi_path).join("bwapi-data/bwapi.ini");
        cmd.env(
            "BWAPI_CONFIG_INI",
            ini_path.to_str().expect("Could not find bwapi.ini"),
        );
        cmd.current_dir(bot_bwapi_path);
    }
}

fn base_folder() -> PathBuf {
    std::env::current_exe()
        .expect("Could not find executable")
        .parent()
        .expect("BWAIShotgun folder does not exist")
        .to_owned()
}

#[derive(Debug)]
enum Binary {
    Dll(PathBuf),
    Jar(PathBuf),
    Exe(PathBuf),
}

impl Binary {
    fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| {
                let mut ext = ext.to_string();
                ext.make_ascii_lowercase();
                let result = match ext.as_str() {
                    "dll" => Binary::Dll(path.to_path_buf()),
                    "jar" => Binary::Jar(path.to_path_buf()),
                    "exe" => Binary::Exe(path.to_path_buf()),
                    _ => return None,
                };
                Some(result)
            })
    }

    fn search(search_path: &Path) -> anyhow::Result<Self> {
        let mut executable = None;
        for file in read_dir(search_path)?.flatten() {
            let path = file.path();
            if let Some(detected_binary) = Binary::from_path(&path) {
                executable = Some(match (executable, detected_binary) {
                    (None, dll @ Binary::Dll(_)) | (Some(dll @ Binary::Dll(_)), Binary::Jar(_)) => {
                        dll
                    }
                    (None, jar @ Binary::Jar(_)) => jar,
                    (None, exe @ Binary::Exe(_))
                    | (Some(Binary::Dll(_) | Binary::Jar(_)), exe @ Binary::Exe(_)) => exe,
                    _ => bail!(SGError::MultipleExecutables(search_path.to_path_buf())),
                })
            }
        }
        match executable {
            None => bail!(SGError::ExecutableNotFound(search_path.to_path_buf())),
            Some(x) => Ok(x),
        }
    }
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
    bwapi_dll: PathBuf,
    working_dir: PathBuf,
    log_dir: PathBuf,
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
        let bwapi_ini_path = bwapi_data_path.join("bwapi.ini");
        create_dir_all(read_path).expect("Could not create read folder");
        create_dir_all(write_path).expect("Could not create write folder");
        create_dir_all(&log_dir).expect("Colud not create log folder");
        let mut bwapi_ini = File::create(bwapi_ini_path).expect("Could not create 'bwapi.ini'");

        let bot_binary = definition
            .executable
            .as_deref()
            .and_then(|s| Binary::from_path(Path::new(s)))
            .unwrap_or_else(|| {
                Binary::search(ai_module_path.as_path())
                    .expect("Could not find bot binary in 'bwapi-data/AI'")
            });
        BwapiIni {
            ai: match &bot_binary {
                Binary::Dll(x) => x.to_string_lossy().to_string(),
                Binary::Exe(_) | Binary::Jar(_) => "".to_string(),
            },
        }
        .write(&mut bwapi_ini)
        .expect("Could not write 'bwapi.ini'");
        drop(bwapi_ini);

        Self {
            binary: bot_binary,
            race: config.race.unwrap_or(definition.race),
            name: config
                .player_name
                .clone()
                .unwrap_or_else(|| config.name.clone()),
            bwapi_dll: bwapi_data_path.join("BWAPI.dll"),
            working_dir: path.to_path_buf(),
            log_dir,
        }
    }
}

fn main() {
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
    let bwheadless = BwHeadless::new(&starcraft_exe)
        .map_err(|e| e.to_string())
        .expect("StarCraft.exe could not be found");
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
                    println!("'{}' was added multiple times. All instances will use the same read/write/log folders and could fail to work properly.", bot);
                }
            }
            let mut instances = vec![];
            // If a human is going to host, no need to fire up a host
            let mut host = game_config.human_host;
            for bot in prepared_bots {
                let mut cmd = if !host {
                    host = true;
                    bwheadless.host_command(
                        &game_config.map,
                        player_count,
                        game_config.game_name.as_deref().unwrap_or("shotgun"),
                    )
                } else {
                    bwheadless.join_command()
                }
                .expect("Could not execute bwheadless");
                BwHeadless::add_bot_args(
                    &mut cmd,
                    bot.race,
                    &bot.name,
                    bot.bwapi_dll.to_string_lossy().deref(),
                    bot.working_dir.to_string_lossy().deref(),
                );
                let old_connected_client_count =
                    if matches!(bot.binary, Binary::Exe(_) | Binary::Dll(_)) {
                        game_table_access
                            .get_game_table()
                            .map(|table| {
                                table
                                    .game_instances
                                    .iter()
                                    .filter(|it| it.is_connected)
                                    .count()
                            })
                            .unwrap_or(0)
                    } else {
                        0
                    };
                cmd.stdout(
                    File::create(bot.log_dir.join("game_out.log"))
                        .expect("Could not create game output log"),
                );
                cmd.stderr(
                    File::create(bot.log_dir.join("game_err.log"))
                        .expect("Could not create game error log"),
                );
                println!("Firing up {}", bot.name);
                let bwheadless = cmd
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
                        cmd.current_dir(Path::new(bot.working_dir.to_string_lossy().deref()));
                        cmd.arg("-jar").arg(jar);
                        cmd.stdout(bot_out_log);
                        cmd.stderr(bot_err_log);

                        let child = cmd.spawn().expect("Could not execute bot binary");

                        // Wait up to 10 seconds before bailing
                        retry(Fixed::from_millis(100).take(100), || {
                            let found = game_table_access.get_game_table().map(|table| {
                                table
                                    .game_instances
                                    .iter()
                                    .filter(|game_instance| game_instance.is_connected)
                                    .count()
                                    > old_connected_client_count
                            });
                            match found {
                                None => Err("Game table not found"),
                                Some(true) => Ok(()),
                                Some(false) => Err("Bot process not found in game table"),
                            }
                        })
                        .expect("Bot failed to connect to BWAPI");
                        bot_process = Some(child);

                        // Give some time for
                    }
                    Binary::Exe(exe) => {
                        let mut cmd = Command::new(exe);
                        cmd.current_dir(Path::new(bot.working_dir.to_string_lossy().deref()));
                        cmd.stdout(bot_out_log);
                        cmd.stderr(bot_err_log);

                        let child = cmd.spawn().expect("Could not execute bot binary");

                        // Wait up to 10 seconds before bailing
                        retry(Fixed::from_millis(100).take(100), || {
                            let found = game_table_access.get_game_table().map(|table| {
                                table
                                    .game_instances
                                    .iter()
                                    .filter(|game_instance| game_instance.is_connected)
                                    .count()
                                    > old_connected_client_count
                            });
                            match found {
                                None => Err("Game table not found"),
                                Some(true) => Ok(()),
                                Some(false) => Err("Bot process not found in game table"),
                            }
                        })
                        .expect("Bot failed to connect to BWAPI");
                        bot_process = Some(child);
                    }
                }
                // TODO: Redirect stderr to stdout - bwheadless is very silent for a typical command prompt
                instances.push(BotProcess {
                    bwheadless,
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
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
