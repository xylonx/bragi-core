use std::path::PathBuf;

use anyhow::{Context, Result};
use config::Config;
use log::info;
use settings::Setting;

pub mod scraper;
pub mod server;
pub mod settings;
pub mod utils;

pub mod bragi {
    tonic::include_proto!("bragi");
}

pub fn load_config(filepath: PathBuf, env_prefix: Option<String>) -> Result<Setting> {
    info!("will load config from file {:?}", filepath);
    let mut builder = Config::builder().add_source(config::File::from(filepath));
    if let Some(prefix) = env_prefix {
        if !prefix.is_empty() {
            info!("will load config from env with prefix {}", prefix);
            builder = builder.add_source(config::Environment::with_prefix(prefix.as_str()));
        }
    }
    let settings = builder
        .build()?
        .try_deserialize::<Setting>()
        .with_context(|| "deserialize config failed")?;
    info!("deserialize config successfully");
    settings
        .check_validation()
        .with_context(|| "check config validation failed")?;
    info!("check config successfully");
    Ok(settings)
}
