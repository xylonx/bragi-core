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

use anyhow::Result;
use async_trait::async_trait;

use crate::bragi::{
    detail_replay::detail_item::Item as DetailItem, search_replay::search_item::Item as SearchItem,
    Provider, SearchZone, Stream,
};

#[async_trait]
pub trait Scraper {
    fn provider(&self) -> Provider;

    async fn suggest(&self, keyword: String) -> Result<Vec<String>>;

    async fn search(
        &self,
        keyword: String,
        page: i32,
        fields: Vec<SearchZone>,
    ) -> Result<Vec<SearchItem>>;

    async fn detail(&self, id: String, zone: SearchZone) -> Result<DetailItem>;

    async fn stream(&self, id: String) -> Result<Vec<Stream>>;
}
