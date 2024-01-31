pub mod bili;
pub mod netease;
pub mod youtube;

use std::{collections::HashMap, fmt::Debug, sync::Arc};

use anyhow::anyhow;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::settings::Settings;

use self::{bili::BiliScraper, netease::NeteaseScraper, youtube::YouTubeScraper};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrapeType {
    All,
    Song,
    Artist,
    Playlist,
    Album,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrapeItem {
    Artist(Artist),
    Song(Song),
    Playlist(SongCollection),
    Album(SongCollection),
}

#[derive(Debug, Clone, Serialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Song {
    pub id: String,
    pub name: String,
    pub artists: Vec<Artist>,
    pub cover: Option<String>,
    pub duration: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SongCollection {
    pub id: String,
    pub name: String,
    pub artists: Vec<Artist>,
    pub cover: Option<String>,
    pub description: Option<String>,
    pub songs: Vec<Song>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stream {
    pub quality: String,
    pub url: String,
}

#[async_trait]
pub trait Scraper {
    async fn suggest(&self, keyword: String) -> anyhow::Result<Vec<String>>;

    async fn search(&self, keyword: String, t: ScrapeType) -> Vec<ScrapeItem>;

    async fn collection_detail(&self, id: String) -> anyhow::Result<SongCollection>;

    async fn stream(&self, id: String) -> anyhow::Result<Vec<Stream>>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Bilibili,
    NetEase,
    Spotify,
    Youtube,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithProvider<T> {
    provider: Provider,
    data: T,
}

impl<T> WithProvider<T> {
    fn new(provider: Provider, data: T) -> Self {
        Self { provider, data }
    }
}

#[derive(Default, Clone)]
pub struct ScraperManager {
    scrapers: Arc<RwLock<HashMap<Provider, Box<dyn Scraper>>>>,
}

unsafe impl Send for ScraperManager {}
unsafe impl Sync for ScraperManager {}

impl ScraperManager {
    pub async fn add_scraper(&mut self, provider: Provider, scraper: Box<dyn Scraper>) {
        info!("add scraper: provider: {:?}", provider);
        let mut scrapers = self.scrapers.write().await;
        scrapers.insert(provider, scraper);
    }

    pub async fn suggest(&self, keyword: String) -> Vec<WithProvider<String>> {
        futures::future::join_all(self.scrapers.read().await.iter().map(|(p, s)| {
            let keyword = keyword.clone();
            async move {
                s.suggest(keyword).await.map(|ss| {
                    ss.into_iter()
                        .map(|s| WithProvider::new(p.clone(), s))
                        .collect::<Vec<_>>()
                })
            }
        }))
        .await
        .into_iter()
        .filter_map(
            |v: Result<Vec<WithProvider<String>>, anyhow::Error>| match v {
                Ok(v) => Some(v),
                Err(e) => {
                    error!("suggest failed: {}", e);
                    None
                }
            },
        )
        .flatten()
        .collect()
    }

    pub async fn search(&self, keyword: String, t: ScrapeType) -> Vec<WithProvider<ScrapeItem>> {
        futures::future::join_all(self.scrapers.read().await.iter().map(|(p, s)| {
            let keyword = keyword.clone();
            let t = t.clone();
            async move {
                s.search(keyword, t)
                    .await
                    .into_iter()
                    .map(|s| WithProvider::new(p.clone(), s))
                    .collect::<Vec<_>>()
            }
        }))
        .await
        .into_iter()
        .flatten()
        .collect()
    }

    pub async fn collection_detail(
        &self,
        id: String,
        provider: Provider,
    ) -> anyhow::Result<SongCollection> {
        self.scrapers
            .read()
            .await
            .get(&provider)
            .map(|s| s.collection_detail(id))
            .ok_or(anyhow!("unsupported provider: {:?}", provider))?
            .await
    }

    pub async fn stream(&self, id: String, provider: Provider) -> anyhow::Result<Vec<Stream>> {
        self.scrapers
            .read()
            .await
            .get(&provider)
            .map(|s| s.stream(id))
            .ok_or(anyhow!("unsupported provider: {:?}", provider))?
            .await
    }

    pub async fn try_from_settings(settings: &Settings) -> anyhow::Result<Self> {
        let mut manager = Self::default();

        if let Some(cfg) = &settings.youtube {
            if let Some(scraper) = YouTubeScraper::try_from_setting(cfg.clone())? {
                manager
                    .add_scraper(Provider::Youtube, Box::new(scraper))
                    .await;
            }
        }

        if let Some(cfg) = &settings.netease {
            if let Some(scraper) = NeteaseScraper::try_from_setting(cfg.clone())? {
                manager
                    .add_scraper(Provider::NetEase, Box::new(scraper))
                    .await;
            }
        }

        if let Some(cfg) = &settings.bilibili {
            if let Some(scraper) = BiliScraper::try_from_setting(cfg.clone())? {
                manager
                    .add_scraper(Provider::Bilibili, Box::new(scraper))
                    .await;
            }
        }

        Ok(manager)
    }
}
