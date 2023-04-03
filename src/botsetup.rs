use crate::{ExecutionWrapper, GameConfig, Race};
use anyhow::{bail, Context};
use log::debug;
use std::fs::read_dir;
use std::path::{Path, PathBuf};
use std::process::Command;

pub trait LaunchBuilder {
    fn build_command(&self, game_config: &GameConfig) -> anyhow::Result<Command>;
}

#[derive(Debug)]
pub struct BotSetup {
    pub starcraft_exe: PathBuf,
    pub starcraft_path: PathBuf,
    pub player_name: String,
    pub bot_binary: Binary,
    pub bot_base_path: PathBuf,
    pub tournament_module: Option<PathBuf>,
    pub race: Race,
    pub wrapper: ExecutionWrapper,
    pub replay_path: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Binary {
    Dll(PathBuf),
    Jar(PathBuf),
    Exe(PathBuf),
}

impl Binary {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
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

    pub(crate) fn search(search_path: &Path) -> anyhow::Result<Self> {
        let mut executable = None;
        debug!("Searching for bot in '{}'", search_path.display());
        for file in read_dir(search_path)
            .with_context(|| format!("Could not search in {}", search_path.display()))?
            .flatten()
        {
            let path = file.path();
            if let Some(detected_binary) = Binary::from_path(&path) {
                executable = Some(match (executable, detected_binary) {
                    (None, dll @ Binary::Dll(_)) | (Some(dll @ Binary::Dll(_)), Binary::Jar(_)) => {
                        dll
                    }
                    (None, jar @ Binary::Jar(_)) => jar,
                    (None, exe @ Binary::Exe(_))
                    | (Some(Binary::Dll(_) | Binary::Jar(_)), exe @ Binary::Exe(_)) => exe,
                    _ => bail!(
                        "Found multiple binary candidates in '{}', please select one in 'bot.toml'",
                        search_path.to_string_lossy()
                    ),
                })
            }
        }
        match executable {
            None => bail!("No binary found in '{}'", search_path.to_string_lossy()),
            Some(x) => Ok(x),
        }
    }
}
