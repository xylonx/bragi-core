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
use async_trait::async_trait;
use dashmap::DashMap;
use lazy_static::lazy_static;
use log::info;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::{
    bragi::{
        bragi_server::Bragi, detail_replay::DetailItem, search_replay::SearchItem,
        search_suggestion_replay::Suggestion, DetailReplay, DetailRequest, Provider, SearchReplay,
        SearchRequest, SearchSuggestionReplay, SearchSuggestionRequest, SearchZone, StreamReplay,
        StreamRequest,
    },
    scraper::Scraper,
};

lazy_static! {
    static ref DEFAULT_PROVIDERS: Vec<i32> = vec![
        Provider::Bilibili.into(),
        Provider::Spotify.into(),
        Provider::Youtube.into(),
        Provider::NeteaseMusic.into()
    ];
    static ref DEFAULT_SEARCH_ZONE: Vec<i32> = vec![
        SearchZone::Track.into(),
        SearchZone::Playlist.into(),
        SearchZone::Artist.into()
    ];
}

#[derive(Default)]
pub struct MyBragiServer {
    scrapers: Arc<DashMap<Provider, Box<dyn Scraper + Send + Sync>>>,
}

impl MyBragiServer {
    pub fn add_scraper<T: Scraper + 'static + Send + Sync>(&mut self, scraper: T) {
        info!("add scraper: {:?}", scraper.provider());
        self.scrapers.insert(scraper.provider(), Box::new(scraper));
    }
}

#[async_trait]
impl Bragi for MyBragiServer {
    async fn suggest(
        &self,
        req: Request<SearchSuggestionRequest>,
    ) -> Result<Response<SearchSuggestionReplay>, Status> {
        let request = req.into_inner();
        info!("Get a request: {:?}", request);

        let providers = {
            if request.providers.is_empty() {
                DEFAULT_PROVIDERS.clone()
            } else {
                request.providers
            }
        };

        let suggestions = futures::future::try_join_all(
            providers
                .into_iter()
                .filter_map(|i| Provider::from_i32(i))
                .map(|p| {
                    let k = request.keyword.clone();
                    async move {
                        Result::<Vec<Suggestion>, Status>::Ok(
                            self.scrapers
                                .get(&p)
                                .ok_or_else(|| {
                                    Status::unimplemented(format!("provider not impl now"))
                                })?
                                .suggest(k)
                                .await
                                .map_err(|e| Status::unknown(format!("[Provider={:?}] {}", p, e)))?
                                .into_iter()
                                .map(|i| Suggestion {
                                    provider: p.into(),
                                    suggestion: i,
                                })
                                .collect::<Vec<Suggestion>>(),
                        )
                    }
                }),
        )
        .await?;

        Ok(Response::new(SearchSuggestionReplay {
            suggestions: suggestions.into_iter().flatten().collect(),
        }))
    }

    // #![feature(iterator_try_collect)]
    async fn search(&self, req: Request<SearchRequest>) -> Result<Response<SearchReplay>, Status> {
        let request = req.into_inner();
        info!("Get a request: {:?}", request);

        // check provider and search zone
        let zones = {
            if request.fields.is_empty() {
                DEFAULT_SEARCH_ZONE.clone()
            } else {
                request.fields
            }
        }
        .into_iter()
        .filter_map(|i| SearchZone::from_i32(i))
        .collect::<Vec<_>>();

        let providers = {
            if request.providers.is_empty() {
                DEFAULT_PROVIDERS.clone()
            } else {
                request.providers
            }
        };

        let search_items = futures::future::try_join_all(
            providers
                .into_iter()
                .filter_map(|i| Provider::from_i32(i))
                .map(|p| {
                    let k = request.keyword.clone();
                    let page = request.page;
                    let z = zones.clone();
                    async move {
                        self.scrapers
                            .get(&p)
                            .ok_or_else(|| Status::unimplemented(format!("provider not impl now")))?
                            .search(k, page, z)
                            .await
                            .map_err(|e| Status::unknown(format!("[Provider={:?}] {}", p, e)))
                    }
                }),
        )
        .await?;

        Ok(Response::new(SearchReplay {
            items: search_items
                .into_iter()
                .flatten()
                .map(|i| SearchItem { item: Some(i) })
                .collect(),
        }))
    }

    async fn detail(&self, req: Request<DetailRequest>) -> Result<Response<DetailReplay>, Status> {
        let request = req.into_inner();
        info!("Get a request: {:?}", request);

        let provider = request.provider();
        let zone = SearchZone::from_i32(request.zone).ok_or_else(|| {
            Status::unimplemented(format!("search zone {:?} not impl now", request.zone))
        })?;

        let detail = self
            .scrapers
            .get(&provider)
            .ok_or_else(|| Status::unimplemented(format!("provider {:?} not impl now", provider)))?
            .detail(request.id, zone)
            .await
            .map_err(|e| Status::unknown(format!("[Provider={:?}] {}", provider, e)))?;

        Ok(Response::new(DetailReplay {
            items: Some(DetailItem { item: Some(detail) }),
        }))
    }

    async fn stream(
        &self,
        req: Request<StreamRequest>,
    ) -> Result<Response<StreamReplay>, tonic::Status> {
        let request = req.into_inner();
        info!("Get a request: {:?}", request);

        let provider = request.provider();

        let audio_streams = self
            .scrapers
            .get(&provider)
            .ok_or_else(|| Status::unimplemented(format!("provider {:?} not impl now", provider)))?
            .stream(request.id)
            .await
            .map_err(|e| Status::unknown(format!("[Provider={:?}] {}", provider, e)))?;

        Ok(Response::new(StreamReplay {
            audios: audio_streams,
            videos: vec![],
        }))
    }
}
