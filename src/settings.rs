use std::collections::HashSet;

use anyhow::bail;
use config::{Config, Environment, File};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ApplicationSettings {
    pub host: String,
    pub port: u16,

    pub tokens: HashSet<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NeteaseSettings {
    pub enabled: bool,

    pub instance: String,
    pub cookie_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct YouTubeSettings {
    pub enabled: bool,
    pub instance: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BiliSettings {
    pub enabled: bool,

    pub cookie_path: String,
    pub wbi_path: String,
    pub enable_dolby: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub application: ApplicationSettings,

    pub netease: Option<NeteaseSettings>,
    pub youtube: Option<YouTubeSettings>,
    pub bilibili: Option<BiliSettings>,
}

impl Settings {
    /// Accepting config file and env as config source. Env > file
    pub fn new(filename: Option<String>, env_prefix: Option<&str>) -> anyhow::Result<Self> {
        if filename.is_none() && env_prefix.is_none() {
            bail!("Settings: at least one source need to be included: no config file or env prefix configured");
        }

        let mut config_builder = Config::builder();

        if let Some(f) = filename {
            config_builder = config_builder.add_source(File::with_name(&f))
        }
        if let Some(prefix) = env_prefix {
            config_builder = config_builder.add_source(Environment::with_prefix(prefix));
        }

        Ok(config_builder.build()?.try_deserialize()?)
    }
}
