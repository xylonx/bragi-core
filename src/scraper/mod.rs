///
/// Created on Sun Mar 19 2023
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
pub mod bilibili;
pub mod netease;
pub mod spotify;
pub mod youtube;

use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use log::{error, info};

use crate::bragi::{
    detail_response::DetailItem, search_response::SearchItem, stream_response::StreamItem,
    suggest_response::Suggestion, DetailRequest, DetailResponse, Provider, SearchRequest,
    SearchResponse, StreamRequest, StreamResponse, SuggestRequest, SuggestResponse, Zone,
};

// Scraper - scrape data from provider. any errors occurred when scraping MUST throw out and be handled by ScraperManager.
#[async_trait]
pub trait Scraper {
    fn provider(&self) -> Provider;

    async fn suggest(&self, keyword: String) -> Result<Vec<Suggestion>>;

    /// For search, when the fields contains unsupported SearchZone,
    /// it should not throw an error. Instead, it should just notice user and handle the remaining ones
    async fn search(&self, keyword: String, page: i32, zones: Vec<Zone>)
        -> Result<Vec<SearchItem>>;

    // Now detail just support ZONE::PLAYLIST. Maybe support other zone like artist to list all works about the artists
    async fn detail(&self, id: String, zone: Zone) -> Result<DetailItem>;

    async fn stream(&self, id: String) -> Result<Vec<StreamItem>>;
}

#[derive(Default)]
pub struct ScraperManager {
    scrapers: Arc<DashMap<Provider, Box<dyn Scraper + Send + Sync>>>,
}

impl ScraperManager {
    pub fn new() -> Self {
        Self {
            scrapers: Arc::new(DashMap::new()),
        }
    }

    pub fn add_scraper<T: Scraper + 'static + Send + Sync>(&mut self, scraper: T) {
        info!("add scraper {:?}", scraper.provider());
        self.scrapers.insert(scraper.provider(), Box::new(scraper));
    }
}

impl ScraperManager {
    pub async fn suggest(&self, req: SuggestRequest) -> Result<SuggestResponse> {
        if req.providers.is_empty() {
            bail!("[ScraperManager] providers MUST be provided but is nil.");
        }

        Ok(SuggestResponse {
            suggestions: futures::future::join_all(req.providers().map(|p| {
                let keywords = req.keyword.clone();
                async move {
                    self.scrapers
                        .get(&p)
                        .ok_or_else(|| {
                            anyhow!("[ScraperManager] provider {:?} not enabled now", p)
                        })?
                        .suggest(keywords)
                        .await
                }
            }))
            .await
            .into_iter()
            .filter_map(|i| {
                if i.is_err() {
                    error!("[ScraperManager] failed to scrape search result: {:?}", i);
                }
                i.ok()
            })
            .flatten()
            .collect(),
        })
    }

    pub async fn search(&self, req: SearchRequest) -> Result<SearchResponse> {
        if req.providers.is_empty() {
            bail!("[ScraperManager] providers MUST be provided but is nil.");
        }

        let zones = req.fields().collect::<Vec<_>>();

        Ok(SearchResponse {
            items: futures::future::join_all(req.providers().map(|p| {
                let keyword = req.keyword.clone();
                let zones = zones.clone();
                async move {
                    self.scrapers
                        .get(&p)
                        .ok_or_else(|| {
                            anyhow!("[ScraperManager] provider {:?} not enabled now", p)
                        })?
                        .search(keyword, req.page, zones)
                        .await
                }
            }))
            .await
            .into_iter()
            .filter_map(|i| {
                if i.is_err() {
                    error!("[ScraperManager] failed to scrape search result: {:?}", i);
                }
                i.ok()
            })
            .flatten()
            .collect(),
        })
    }

    pub async fn detail(&self, req: DetailRequest) -> Result<DetailResponse> {
        let zone = req.zone();
        Ok(DetailResponse {
            item: Some(
                self.scrapers
                    .get(&req.provider())
                    .ok_or_else(|| {
                        anyhow!(
                            "[ScraperManager] provider {:?} not enabled now",
                            req.provider()
                        )
                    })?
                    .detail(req.id, zone)
                    .await?,
            ),
        })
    }

    pub async fn stream(&self, req: StreamRequest) -> Result<StreamResponse> {
        Ok(StreamResponse {
            streams: self
                .scrapers
                .get(&req.provider())
                .unwrap()
                .stream(req.id)
                .await?,
        })
    }
}
