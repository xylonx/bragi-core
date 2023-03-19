use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use bragi_core::{
    bragi::bragi_server::BragiServer,
    load_config,
    scraper::{bilibili::BiliScraper, spotify::SpotifyScraper},
    server::MyBragiServer,
    utils::disk_cookie_store::PersistCookieStore,
};
use clap::Parser;
use log::info;
use reqwest::Url;
use tonic::transport::Server;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    /// The path to the configuration file
    #[arg(short, long, value_name = "CONFIG")]
    config_path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let cli = CliArgs::parse();
    info!("cli args: {:?}", cli);

    let settings =
        load_config(cli.config_path.unwrap(), Some("BRAGI".into())).expect("load config failed");

    let mut bragi = MyBragiServer::default();

    if let Some(config) = settings.provider.bilibili {
        if config.enabled {
            let cookie_store = PersistCookieStore::new(
                Url::parse("https://api.bilibili.com").unwrap(),
                Some(Path::new(config.cookie_path.as_str()).to_path_buf()),
            )
            .with_context(|| "new persisted cookie store failed")?;
            bragi.add_scraper(
                BiliScraper::new(Arc::new(cookie_store), 10)
                    .await
                    .with_context(|| "init bilibili scraper failed")?,
            );
        }
    };

    if let Some(config) = settings.provider.spotify {
        if config.enabled {
            // TODO(xylonx): cache dir
            bragi.add_scraper(
                SpotifyScraper::new(config.username, config.password, Path::new(""))
                    .await
                    .with_context(|| "init spotify scraper failed")?,
            );
        }
    }

    Server::builder()
        .add_service(BragiServer::new(bragi))
        .serve(settings.server.addr.parse()?)
        .await?;

    Ok(())
}
