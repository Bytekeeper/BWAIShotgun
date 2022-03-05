use anyhow::bail;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::{Debug, Display, Formatter};
use std::fs::{create_dir_all, metadata, read, read_dir, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use registry::{Hive, Security};
use serde::de::Unexpected;
use serde::{Deserialize, Deserializer};
use toml::toml;

#[derive(Deserialize, Debug, Default)]
struct ShotgunConfig {
    starcraft_path: Option<String>,
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
struct BotConfig {
    name: String,
    player_name: Option<String>,
    race: Option<Race>,
}

#[derive(Deserialize, Debug)]
enum GameType {
    Melee(Vec<BotConfig>),
}

#[derive(Deserialize, Debug)]
struct GameConfig {
    map: String,
    game_name: Option<String>,
    game_type: GameType,
}

impl GameConfig {
    fn new(config: &ShotgunConfig) -> Result<Self, String> {
        let result: GameConfig = toml::from_slice(
            read(base_folder().join("game.toml"))
                .map_err(|e| e.to_string())
                .expect("'game.toml' not found")
                .as_slice(),
        )
        .map_err(|e| e.to_string())
        .expect("'game.toml' is invalid");
        if result.map.is_empty() {
            return Err("Missing map name".to_owned());
        }
        let map_path_abs = Path::new(&result.map);
        let map_path_rel = PathBuf::from(
            config
                .get_starcraft_path()
                .expect("Could not find StarCraft installation"),
        )
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
enum Race {
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
                Race::Protoss => "p",
                Race::Terran => "t",
                Race::Zerg => "z",
                Race::Random => "r",
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

fn search_bot_executable(ai_path: &Path) -> anyhow::Result<PathBuf> {
    enum Binary {
        None,
        Dll(PathBuf),
        Jar(PathBuf),
        Exe(PathBuf),
    }
    let mut executable: Binary = Binary::None;
    for file in read_dir(ai_path)?.flatten() {
        let path = file.path();
        if let Some(ext) = path.extension().map(|ext| ext.to_str()).flatten() {
            let mut ext = ext.to_string();
            ext.make_ascii_lowercase();
            executable = match executable {
                Binary::None if ext == "dll" => Binary::Dll(path.to_path_buf()),
                Binary::None if ext == "jar" => Binary::Jar(path.to_path_buf()),
                Binary::None if ext == "exe" => Binary::Exe(path.to_path_buf()),
                Binary::Dll(_) | Binary::Jar(_) if ext == "exe" => Binary::Exe(path.to_path_buf()),
                Binary::Dll(_) if ext == "jar" => Binary::Jar(path.to_path_buf()),
                _ => bail!(SGError::MultipleExecutables(ai_path.to_path_buf())),
            }
        }
    }
    match executable {
        Binary::None => bail!(SGError::ExecutableNotFound(ai_path.to_path_buf())),
        Binary::Dll(x) | Binary::Jar(x) | Binary::Exe(x) => Ok(x),
    }
}

fn main() {
    let config: ShotgunConfig = read("shotgun.toml")
        .map(|cfg| {
            toml::from_slice(cfg.as_slice())
                .map_err(|e| e.to_string())
                .expect("Invalid config in 'shotgun.toml'")
        })
        .unwrap_or_else(|_| {
            eprintln!("shotgun.toml not found, using defaults");
            ShotgunConfig::default()
        });

    let game_config = GameConfig::new(&config).expect("Could not load 'game.toml'");

    // let config: BotDefinition = toml::from_slice(read("bots/template/bot.toml")?.as_slice())?;
    // println!("{config:?}");
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
                            .map_err(|e| {
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
                        if &bot_definition.race != &Race::Random && &bot_definition.race != race {
                            eprintln!(
                                "Bot '{}' is configured to play as {}, but supports {} only!",
                                cfg.name, race, bot_definition.race
                            );
                        }
                    }
                    (cfg, bot_folder, bot_definition)
                })
                .collect();
            let mut host = false;
            let player_count = bots.len();
            let mut instances = vec![];
            for (config, path, definition) in bots {
                let bwapi_data_path = path.join("bwapi-data");
                let read_path = bwapi_data_path.join("read");
                let write_path = bwapi_data_path.join("write");
                let bwapi_ini_path = bwapi_data_path.join("bwapi.ini");
                create_dir_all(bwapi_data_path).expect("Could not create bwapi-data folder");
                create_dir_all(read_path).expect("Could not create read folder");
                create_dir_all(write_path).expect("Could not create write folder");
                let mut bwapi_ini =
                    File::create(bwapi_ini_path).expect("Could not create 'bwapi.ini'");
                // Workaround BWAPI 3.7.x "strangeness" of removing ":" ...
                let mut path = path.components();
                path.next();
                let path = path.as_path();
                BwapiIni {
                    ai: definition.executable.unwrap_or_else(|| {
                        search_bot_executable(path.join("AI").as_path())
                            .map(|p| p.to_str().expect("Could not retrieve path").to_string())
                            .expect("Could not find bot executable")
                    }),
                }
                .write(&mut bwapi_ini)
                .expect("Could not write 'bwapi.ini'");
                drop(bwapi_ini);

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
                    config.race.unwrap_or(definition.race),
                    config
                        .player_name
                        .as_deref()
                        .unwrap_or(config.name.as_str()),
                    path.join("BWAPI.dll")
                        .to_str()
                        .expect("Could not find path to BWAPI.dll"),
                    path.to_str()
                        .expect("Could not find path to bot config (bwapi.ini)"),
                );
                println!("Firing up {}", config.name);
                instances.push(cmd.spawn().expect(
                    "Could not run bwheadless (maybe deleted/blocked by a Virus Scanner?)",
                ));
            }
            for mut instance in instances {
                instance
                    .wait()
                    .expect("Failed to wait on bwheadless instance");
            }
        }
    }
}
