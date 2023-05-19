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
use std::sync::Arc;

use async_trait::async_trait;
use log::info;
use tonic::{Request, Response, Status};

use crate::{
    bragi::{
        bragi_service_server::BragiService, DetailRequest, DetailResponse, SearchRequest,
        SearchResponse, StreamRequest, StreamResponse, SuggestRequest, SuggestResponse,
    },
    scraper::ScraperManager,
};

#[derive(Default)]
pub struct MyBragiServer {
    manager: Arc<ScraperManager>,
}

impl MyBragiServer {
    pub fn new(manager: ScraperManager) -> Self {
        Self {
            manager: Arc::new(manager),
        }
    }
}

#[async_trait]
impl BragiService for MyBragiServer {
    async fn suggest(
        &self,
        req: Request<SuggestRequest>,
    ) -> Result<Response<SuggestResponse>, Status> {
        let request = req.into_inner();
        info!("Get a request: {:?}", request);

        match self.manager.suggest(request).await {
            Ok(v) => Ok(Response::new(v)),
            Err(e) => Err(Status::unknown(e.to_string())),
        }
    }

    // #![feature(iterator_try_collect)]
    async fn search(
        &self,
        req: Request<SearchRequest>,
    ) -> Result<Response<SearchResponse>, Status> {
        let request = req.into_inner();
        info!("Get a request: {:?}", request);

        match self.manager.search(request).await {
            Ok(v) => Ok(Response::new(v)),
            Err(e) => Err(Status::unknown(e.to_string())),
        }
    }

    async fn detail(
        &self,
        req: Request<DetailRequest>,
    ) -> Result<Response<DetailResponse>, Status> {
        let request = req.into_inner();
        info!("Get a request: {:?}", request);

        match self.manager.detail(request).await {
            Ok(v) => Ok(Response::new(v)),
            Err(e) => Err(Status::unknown(e.to_string())),
        }
    }

    async fn stream(
        &self,
        req: Request<StreamRequest>,
    ) -> Result<Response<StreamResponse>, tonic::Status> {
        let request = req.into_inner();
        info!("Get a request: {:?}", request);

        match self.manager.stream(request).await {
            Ok(v) => Ok(Response::new(v)),
            Err(e) => Err(Status::unknown(e.to_string())),
        }
    }
}
