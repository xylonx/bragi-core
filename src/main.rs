///
/// Created on Fri May 19 2023
///
/// The MIT License (MIT)
/// Copyright (c) 2023 xylonx
///
/// Permission is hereby granted, free of charge, to any person obtaining a copy
/// of this software and associated documentation files (the "Software"), to deal
/// in the Software without restriction, including without limitation the rights
/// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
/// copies of the Software, and to permit persons to whom the Software is
/// furnished to do so, subject to the following conditions:
///
/// The above copyright notice and this permission notice shall be included in all
/// copies or substantial portions of the Software.
///
/// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
/// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
/// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
/// SOFTWARE.
///
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use bragi_core::{
    bragi::bragi_service_server::BragiServiceServer,
    load_config,
    scraper::{
        bilibili::BiliScraper, netease::NeteaseScraper, spotify::SpotifyScraper,
        youtube::YouTubeScraper, ScraperManager,
    },
    server::MyBragiServer,
    utils::disk_cookie_store::AsyncPersistCookieStore,
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

    let mut manager = ScraperManager::new();

    if let Some(config) = settings.provider.spotify {
        if config.enabled {
            manager.add_scraper(
                SpotifyScraper::try_new(
                    config.username,
                    config.password,
                    config.client_id,
                    config.client_secret,
                    Path::new(&config.cache_dir).to_path_buf(),
                    Path::new(&config.static_dir).to_path_buf(),
                )
                .await
                .with_context(|| "init spotify scraper failed")?,
            );
            info!("init spotify scraper successfully");
        }
    }

    if let Some(config) = settings.provider.bilibili {
        if config.enabled {
            let cookie_store = AsyncPersistCookieStore::new(
                Url::parse("https://api.bilibili.com").unwrap(),
                Path::new(config.cookie_path.as_str()).to_path_buf(),
            )
            .await
            .with_context(|| "new persisted cookie store failed")?;
            manager.add_scraper(
                BiliScraper::try_new(
                    Arc::new(cookie_store),
                    1,
                    config.request_limit.request_buffer_size as usize,
                    config.request_limit.max_concurrency_number as usize,
                    config.request_limit.limit_request_per_seconds as u64,
                    Duration::from_secs(1),
                )
                .await
                .with_context(|| "init bilibili scraper failed")?,
            );
            info!("init bilibili scraper successfully");
        }
    };

    if let Some(config) = settings.provider.netease {
        if config.enabled {
            let cookie_store = AsyncPersistCookieStore::new(
                Url::parse("https://music.163.com").unwrap(),
                Path::new(config.cookie_path.as_str()).to_path_buf(),
            )
            .await
            .with_context(|| "new persisted cookie store failed")?;
            manager.add_scraper(
                NeteaseScraper::try_new(
                    config.instance,
                    Arc::new(cookie_store),
                    1,
                    config.request_limit.request_buffer_size as usize,
                    config.request_limit.max_concurrency_number as usize,
                    config.request_limit.limit_request_per_seconds as u64,
                    Duration::from_secs(1),
                )
                .await
                .with_context(|| "init netease music scraper failed")?,
            );
            info!("init netease scraper successfully");
        }
    }

    if let Some(config) = settings.provider.youtube {
        if config.enabled {
            manager.add_scraper(YouTubeScraper::new(
                config.instance,
                1,
                config.request_limit.request_buffer_size as usize,
                config.request_limit.max_concurrency_number as usize,
                config.request_limit.limit_request_per_seconds as u64,
                Duration::from_secs(1),
            ));
        }
        info!("init youtube scraper successfully");
    }

    info!("will run server at {}", settings.server.addr);

    let bragi = MyBragiServer::new(manager);

    Server::builder()
        .add_service(BragiServiceServer::new(bragi))
        .serve(settings.server.addr.parse()?)
        .await?;

    Ok(())
}
