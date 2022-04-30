use std::collections::HashSet;
use std::fmt::{Debug, Display, Formatter};
use std::fs::{create_dir_all, metadata, read, remove_file, File};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

use anyhow::{anyhow, ensure, Context};
use clap::Parser;
use crc::{Crc, CRC_32_ISO_HDLC};
use log::{debug, info, warn, LevelFilter};
use registry::{Hive, Security};
use retry::delay::Fixed;
use retry::{retry, OperationResult};
use serde::de::Unexpected;
use serde::{Deserialize, Deserializer};
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};

use crate::botsetup::{Binary, BotSetup, LaunchBuilder};
use crate::bwapi::{AutoMenu, BwapiConnectMode, BwapiIni, BwapiVersion, GameTableAccess};
use crate::bwheadless::{BwHeadless, BwHeadlessConnectMode};
use crate::cli::Cli;
use crate::injectory::{Injectory, InjectoryConnectMode};
use crate::sandbox::SandboxMode;

mod botsetup;
mod bwapi;
mod bwheadless;
mod cli;
mod injectory;
mod sandbox;

#[derive(Deserialize, Debug, Default)]
struct ShotgunConfig {
    starcraft_path: Option<String>,
    java_path: Option<String>,
    #[serde(default)]
    sandbox: SandboxMode,
}

fn locate_starcraft() -> anyhow::Result<PathBuf> {
    Ok(Hive::LocalMachine
        .open(r"SOFTWARE\Blizzard Entertainment\Starcraft", Security::Read)
        .context("Could not find Starcraft installation")?
        .value("InstallPath")?
        .to_string()
        .into())
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub enum HeadfulMode {
    Off,
    On {
        #[serde(default)]
        no_wmode: bool,
        #[serde(default)]
        no_sound: bool,
    },
}

impl Default for HeadfulMode {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Deserialize, Debug)]
pub struct BotLaunchConfig {
    pub name: String,
    pub player_name: Option<String>,
    pub race: Option<Race>,
    #[serde(default)]
    pub headful: HeadfulMode,
}

#[derive(Deserialize, Debug)]
pub enum GameType {
    Melee(Vec<BotLaunchConfig>),
}

#[derive(Deserialize, Debug)]
pub struct GameConfig {
    pub map: Option<String>,
    pub game_name: Option<String>,
    pub game_type: GameType,
    #[serde(default)]
    pub human_host: bool,
    #[serde(default)]
    pub human_speed: bool,
    #[serde(default = "default_latency")]
    pub latency_frames: u32,
    pub time_out_at_frame: Option<u32>,
}

fn default_latency() -> u32 {
    3
}

impl GameConfig {
    fn load(starcraft_path: &Path) -> anyhow::Result<GameConfig> {
        let result: GameConfig =
            toml::from_slice(read(base_folder().join("game.toml"))?.as_slice())
                .context("'game.toml' is invalid")?;
        ensure!(
            result.human_host || matches!(&result.map, Some(s) if !s.is_empty()),
            "Map must be set for bot-hosted games"
        );
        if let Some(map_path) = result.map.as_ref().map(Path::new) {
            let map_path_rel = starcraft_path.join(map_path);
            ensure!(
                map_path.is_absolute() && map_path.exists() || map_path_rel.exists(),
                "Could not find map '{}'",
                map_path.to_string_lossy()
            );
        }
        Ok(result)
    }
}

#[derive(Deserialize, Debug)]
pub enum TournamentModule {
    None,
    Default,
    Custom { prefix: String },
}

impl Default for TournamentModule {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Deserialize, Debug)]
struct BotDefinition {
    race: Race,
    executable: Option<String>,
    #[serde(default)]
    tournament_module: TournamentModule,
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
    tournament_module: Option<String>,
    supports_character_name: bool,
    race: Race,
    name: String,
    working_dir: PathBuf,
    log_dir: PathBuf,
    headful: HeadfulMode,
}

impl PreparedBot {
    fn prepare(
        config: &BotLaunchConfig,
        path: &Path,
        definition: &BotDefinition,
    ) -> anyhow::Result<Self> {
        let bwapi_data_path = path.join("bwapi-data");
        // Workaround BWAPI 3.7.x "strangeness" of removing ":" ...
        let mut ai_module_path = bwapi_data_path.components();
        ai_module_path.next();
        let ai_module_path = ai_module_path.as_path().join("AI");
        let read_path = bwapi_data_path.join("read");
        let write_path = bwapi_data_path.join("write");
        let log_dir = path.join("logs");
        create_dir_all(read_path).context("Could not create read folder")?;
        create_dir_all(write_path).context("Could not create write folder")?;
        create_dir_all(&log_dir).context("Could not create log folder")?;
        let tm_path = path.join("tm");
        create_dir_all(&tm_path).context("Could not create tm folder")?;

        for entry in tm_path.read_dir()?.flatten().filter(|it| {
            it.path()
                .extension()
                .map(|os| os.to_string_lossy().as_ref() == "csv")
                .unwrap_or(false)
        }) {
            debug!("Removing {}", entry.path().to_string_lossy());
            remove_file(entry.path()).ok();
        }

        let bot_binary = definition.executable.as_deref().and_then(|s| {
            // First try from bot path
            Binary::from_path(path.join(s).as_path())
                // Then from base path
                .or_else(|| Binary::from_path(base_folder().join(s).as_path()))
        });
        let bot_binary = if let Some(bot_binary) = bot_binary {
            bot_binary
        } else {
            // Lastly search
            Binary::search(ai_module_path.as_path())
                .context("Could not find bot binary in 'bwapi-data/AI'")?
        };
        let race = config.race.unwrap_or(definition.race);

        let bwapi_dll = bwapi_data_path.join("BWAPI.dll");
        let bwapi_crc = Crc::<u32>::new(&CRC_32_ISO_HDLC).checksum(
            std::fs::read(&bwapi_dll)
                .with_context(|| format!("Could not check '{}'", bwapi_dll.to_string_lossy()))?
                .as_slice(),
        );
        let bwapi_version = BwapiVersion::from_u32(bwapi_crc);

        let tournament_module = match &definition.tournament_module {
            TournamentModule::None => None,
            TournamentModule::Default | TournamentModule::Custom { .. } => {
                let prefix =
                    if let TournamentModule::Custom { prefix } = &definition.tournament_module {
                        prefix
                    } else {
                        "tm"
                    };

                if let Some(version) = &bwapi_version {
                    let version = version.version_short();
                    let tm_name = format!("{}_{}.dll", prefix, version);
                    let tm_source_file = base_folder().join("tm").join(&tm_name);
                    std::fs::copy(&tm_source_file, path.join(&tm_name)).with_context(|| {
                        format!(
                            "Could not copy tournament module: '{}'",
                            tm_source_file.to_string_lossy(),
                        )
                    })?;
                    Some(tm_name)
                } else {
                    println!("Custom BWAPI.dll detected, not adding TM module");
                    None
                }
            }
        };

        Ok(Self {
            binary: bot_binary,
            race,
            name: config
                .player_name
                .clone()
                .unwrap_or_else(|| config.name.clone()),
            working_dir: path.to_path_buf(),
            log_dir,
            headful: config.headful,
            tournament_module,
            supports_character_name: !matches!(
                bwapi_version,
                Some(BwapiVersion::Bwapi375 | BwapiVersion::Bwapi412)
            ),
        })
    }
}

fn main() -> anyhow::Result<()> {
    TermLogger::init(
        LevelFilter::Info,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )?;
    info!(
        "Welcome to {} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    let ShotgunConfig {
        starcraft_path,
        java_path,
        sandbox,
    } = if let Ok(cfg) = read(base_folder().join("shotgun.toml")) {
        toml::from_slice(cfg.as_slice()).context("'shotgun.toml' is invalid")?
    } else {
        warn!("'shotgun.toml' not found, using defaults");
        ShotgunConfig::default()
    };
    let starcraft_path = if let Some(starcraft_path) = starcraft_path {
        PathBuf::from(starcraft_path)
    } else {
        locate_starcraft()?
    };
    let starcraft_exe = starcraft_path.join("StarCraft.exe");

    ensure!(
        starcraft_exe.exists(),
        "Could not locate 'StarCraft.exe' in configured location: '{}'",
        starcraft_exe.to_string_lossy()
    );

    if matches!(sandbox, SandboxMode::Unconfigured | SandboxMode::NoSandbox) {
        // Currently, we don't support bot sandboxing
        // println!("You're running bots without a sandbox.");
        if let SandboxMode::Unconfigured = sandbox {
            warn!("If you are sure you don't want use a sandbox, please edit 'shotgun.toml' and set the sandbox to 'NoSandbox'.");
            warn!("Will wait for 15 seconds (press ctrl+c to abort now, or wait and start the bots anyways).");
            std::thread::sleep(Duration::from_secs(15));
        }
    }

    let cli = Cli::parse();

    let game_config: Result<GameConfig, cli::Error> = cli.try_into();
    let game_config = match game_config {
        Ok(game_config) => game_config,
        Err(cli::Error::NoArguments) => GameConfig::load(&starcraft_path)?,
        Err(cli::Error::ClapError(err)) => err.exit(),
    };

    if let Ok(metadata) = metadata(starcraft_path.join("SNP_DirectIP.snp")) {
        if metadata.len() != 46100 {
            warn!("The 'SNP_DirectIP.snp' in your StarCraft installation might not support more than ~6 bots per game. Overwrite with the included 'SNP_DirectIP.snp' file to support more.");
        }
    } else {
        warn!("Could not find 'SNP_DirectIP.snp' in your StarCraft installation, please copy the provided one or install BWAPI.");
    }

    let mut game_table_access = GameTableAccess::new();
    if let Some(game_table) = game_table_access.get_game_table() {
        warn!(
            "Detected a stale game table. If you did not run Starcraft with BWAPI yourself, \
        you should kill all running instances of StarCraft and any lingering bots."
        );

        for server_process_id in game_table
            .game_instances
            .iter()
            .filter(|it| it.is_connected && it.server_process_id != 0)
            .map(|it| it.server_process_id)
        {
            warn!(
            "The process {} is in the game table already and will interfere with game creation.",
            server_process_id
        );
        }
    }

    match game_config.game_type {
        GameType::Melee(ref bots) => {
            let bots: anyhow::Result<Vec<_>> = bots
                .iter()
                .map(|cfg| {
                    let mut bot_folder = base_folder();
                    bot_folder.push("bots");
                    bot_folder.push(&cfg.name);
                    let bot_definition = toml::from_slice::<BotDefinition>(
                        read(bot_folder.join("bot.toml"))
                            .with_context(|| {
                                format!(
                                    "Could not read 'bot.toml' for bot '{}' in: '{}'",
                                    cfg.name,
                                    bot_folder.to_string_lossy(),
                                )
                            })?
                            .as_slice(),
                    )?;
                    if let Some(race) = &cfg.race {
                        if bot_definition.race != Race::Random && &bot_definition.race != race {
                            info!(
                                "Bot '{}' is configured to play as {}, but its default race is {}!",
                                cfg.name, race, bot_definition.race
                            );
                        }
                    }
                    Ok((cfg, bot_folder, bot_definition))
                })
                .collect();
            let bots = bots?;
            let player_count = bots.len();
            let prepared_bots: anyhow::Result<Vec<_>> = bots
                .iter()
                .map(|(config, path, definition)| PreparedBot::prepare(config, path, definition))
                .collect();
            let mut prepared_bots = prepared_bots?;

            // Client bots *must* be ran first, as they need to connect to their resp. BWAPI Server
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
                    warn!("'{}' was added multiple times. All instances will use the same read/write/log folders and could fail to work properly. Also headful mode will not work as expected.", bot);
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
                let bot_setup = BotSetup {
                    starcraft_exe: starcraft_exe.clone(),
                    starcraft_path: starcraft_exe
                        .parent()
                        .context("Could not find parent path of 'StarCraft.exe'")?
                        .to_path_buf(),
                    bot_base_path: bot.working_dir.clone(),
                    tournament_module: bot.tournament_module.map(|s| s.into()),
                    player_name: bot.name.clone(),
                    race: bot.race,
                    sandbox: sandbox.clone(),
                    bot_binary: bot.binary.clone(),
                };
                let tournament_module = bot_setup.tournament_module.clone();
                let bwapi_launcher: Box<dyn LaunchBuilder> = if !matches!(
                    bot.headful,
                    HeadfulMode::Off
                ) {
                    if host {
                        // Headful + Host => All other bots need to join the game with this bots player name
                        if bot.supports_character_name {
                            game_name = bot.name.clone();
                        } else {
                            warn!("Headful hosting bot uses very old BWAPI version, please ensure there's only one character with the name 'BWAPI'.");
                            game_name = "BWAPI".to_string();
                        }
                    }
                    Box::new(Injectory {
                        bot_setup,
                        game_name: if game_config.human_host {
                            "JOIN_FIRST".to_string()
                        } else {
                            game_name.clone()
                        },
                        connect_mode: if host {
                            InjectoryConnectMode::Host {
                                map: game_config.map.clone(),
                                player_count,
                            }
                        } else {
                            InjectoryConnectMode::Join
                        },
                        wmode: matches!(bot.headful, HeadfulMode::On { no_wmode, .. } if !no_wmode),
                        sound: matches!(bot.headful, HeadfulMode::On { no_sound, ..} if !no_sound),
                        game_speed: if game_config.human_speed { -1 } else { 0 },
                    })
                } else {
                    Box::new(BwHeadless {
                        bot_setup,
                        game_name: if game_config.human_host {
                            None
                        } else {
                            Some(game_name.clone())
                        },
                        connect_mode: if host {
                            BwHeadlessConnectMode::Host {
                                map: game_config.map.clone().ok_or_else(|| {
                                    anyhow!("bwheadless cannot host without a map")
                                })?,
                                player_count,
                            }
                        } else {
                            BwHeadlessConnectMode::Join
                        },
                    })
                };
                info!(
                    "{} game with '{}'{}",
                    if host { "Hosting" } else { "Joining" },
                    bot.name,
                    tournament_module
                        .map(|tm| format!(" (with tournament module '{}')", tm.to_string_lossy()))
                        .unwrap_or_else(|| "".to_string())
                );
                host = false;

                let mut cmd = bwapi_launcher.build_command(&game_config)?;
                cmd.stdout(File::create(bot.log_dir.join("game_out.log"))?)
                    .stderr(File::create(bot.log_dir.join("game_err.log"))?);
                let cmd = cmd
                    .env("TM_LOG_FRAMETIMES", r"tm\frames.csv")
                    .env("TM_LOG_RESULTS", r"tm\result.csv")
                    .env("TM_LOG_UNIT_EVENTS", r"tm\unit_events.csv");
                if let Some(time_out_at_frame) = game_config.time_out_at_frame {
                    cmd.env("TM_TIME_OUT_AT_FRAME", time_out_at_frame.to_string());
                }
                let mut bwapi_child = cmd.spawn().context(
                    "Could not run bwheadless (maybe deleted/blocked by a Virus Scanner?)",
                )?;

                let bot_out_log = File::create(bot.log_dir.join("bot_out.log"))?;
                let bot_err_log = File::create(bot.log_dir.join("bot_err.log"))?;
                let bot_process = match bot.binary {
                    Binary::Dll(_) => None,
                    Binary::Jar(jar) => {
                        let mut cmd =
                            sandbox.wrap_executable(java_path.as_deref().unwrap_or("java.exe"));
                        cmd.arg("-jar").arg(jar);
                        Some(cmd)
                    }
                    Binary::Exe(exe) => Some(sandbox.wrap_executable(exe)),
                }
                .map(|ref mut cmd| -> anyhow::Result<Child> {
                    // Wait for server to be ready to accept connections
                    retry(Fixed::from_millis(100).take(100), || {
                        if game_table_access.has_free_slot() {
                            OperationResult::Ok(())
                        } else {
                            OperationResult::Retry("Server process not ready")
                        }
                    }).map_err(|e| anyhow!(e))?;

                    cmd.current_dir(bot.working_dir);
                    cmd.stdout(bot_out_log);
                    cmd.stderr(bot_err_log);

                    let mut child = cmd.spawn()?;

                    // Wait up to 10 seconds before bailing
                    retry(Fixed::from_millis(100).take(100), || {
                        let slots_filled = game_table_access.all_slots_filled();
                        if !matches!(bwapi_child.try_wait(), Ok(None)) {
                            OperationResult::Err("BWAPI process died")
                        } else if !matches!(child.try_wait(), Ok(None)) {
                            OperationResult::Err("Bot process died")
                        } else if slots_filled {
                            OperationResult::Ok(())
                        } else {
                            OperationResult::Retry(
                                "Bot client executable did not connect to BWAPI server (did you try to run a human hosted game without hosting it?)",
                            )
                        }
                    })
                    .map_err(|e| anyhow!(e))?;
                    Ok(child)
                })
                .transpose()?;
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
                        info!("{} bots remaining", instances.len());
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            info!("Done");
            Ok(())
        }
    }
}
