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
use std::{collections::HashMap, matches, sync::Arc, time::Duration};

use anyhow::{anyhow, bail, Context, Ok, Result};
use async_trait::async_trait;
use lazy_static::lazy_static;
use log::info;
use reqwest::{cookie::CookieStore, Method, Request, Url};
use serde::{Deserialize, Deserializer};
use serde_repr::Deserialize_repr;

use crate::{
    bragi::{
        detail_response::{detail_item, DetailItem},
        search_response::{search_item, SearchItem},
        stream_response::StreamItem,
        suggest_response::Suggestion,
        Artist, ArtistDetail, Image, Playlist, PlaylistDetail, Provider, Stream, Track, Zone,
    },
    utils::request::LimitedRequestClient,
};

use super::Scraper;

lazy_static! {
    static ref TITLE_REPLACER: regex::Regex =
        regex::RegexBuilder::new(r#"(<([^>]+)>)"#).build().unwrap();
}

#[derive(Debug, Deserialize)]
struct BiliResponse<T> {
    code: i32,
    message: String,
    data: T,
}

impl<T> BiliResponse<T> {
    fn get_data(self) -> Result<T> {
        if self.code != 0 {
            bail!("[Bili] fetch api failed");
        }
        Ok(self.data)
    }
}

impl From<String> for Image {
    fn from(value: String) -> Self {
        Self {
            url: value,
            width: None,
            length: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct BiliSuggestItem {
    value: String,
}

enum SearchType {
    User,
    Video,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct BiliSearchResponse {
    #[serde(rename = "pagesize")]
    page_size: i32,
    #[serde(rename = "numResults")]
    num_results: i32,
    result: Vec<BiliSearchResultItem>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum BiliSearchResultItem {
    #[serde(rename = "video")]
    Video(BiliSearchVideoItem),
    #[serde(rename = "bili_user")]
    User(BiliUser),
}

/// origin title format may be like: 【永雏塔菲】<em class=\"keyword\">taffy</em>已经开摆了
/// therefore, remove <em> tags to get pure title
fn deserialize_title<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    Result::Ok(TITLE_REPLACER.replace_all(s.as_str(), "").into())
}

/// origin title format may be like: //i0.hdslb.com/bfs/archive/23c4be1b7f62848b95e9b4b2e1d6ce2e50bedf17.jpg
/// therefore, add 'https:' scheme
fn deserialize_cover_url<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    if s.starts_with("//") {
        return Result::Ok(format!("https:{}", s));
    }
    Result::Ok(s)
}

#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
struct BiliSearchVideoItem {
    #[serde(rename = "bvid")]
    id: String,
    #[serde(rename = "pic", deserialize_with = "deserialize_cover_url")]
    cover_url: String,
    #[serde(deserialize_with = "deserialize_title")]
    title: String,
    #[serde(rename = "mid")]
    author_id: u64,
    #[serde(rename = "author")]
    author_name: String,
    #[serde(rename = "duration")]
    duration: String, // TODO(xylonx): maybe useful?
}

#[derive(Debug, Default, Clone, Deserialize)]
struct BiliUser {
    #[serde(rename = "mid")]
    id: u64,
    #[serde(alias = "uname")]
    name: String,
    #[serde(alias = "usign", alias = "sign")] // usign for search while sign for detail
    description: Option<String>, // optional when in videoDetail
    #[serde(
        alias = "upic",
        rename = "face",
        deserialize_with = "deserialize_cover_url"
    )]
    // upic for search while face for detail
    avatar_url: String,
}
impl From<BiliUser> for search_item::Item {
    fn from(u: BiliUser) -> Self {
        search_item::Item::User(ArtistDetail {
            artist: Some(Artist {
                id: u.id.to_string(),
                provider: Provider::Bilibili.into(),
                name: u.name,
            }),
            description: u.description,
            avatar: Some(Image {
                url: u.avatar_url,
                width: None,
                length: None,
            }),
        })
    }
}

impl Into<Artist> for BiliUser {
    fn into(self) -> Artist {
        Artist {
            id: self.id.to_string(),
            provider: Provider::Bilibili.into(),
            name: self.name,
        }
    }
}

impl Into<ArtistDetail> for BiliUser {
    fn into(self) -> ArtistDetail {
        ArtistDetail {
            artist: Some(Artist {
                id: self.id.to_string(),
                provider: Provider::Bilibili.into(),
                name: self.name,
            }),
            description: self.description,
            avatar: Some(Image::from(self.avatar_url)),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct BiliVideoDetail {
    #[serde(rename = "bvid")]
    id: String,
    #[serde(rename = "videos")]
    video_number: u32, // identity video collection
    // pic here contains scheme like "http://i0.hdslb.com/bfs/archive/c8d195be7b79b63879d306f6aaffdb2dea485b95.jpg". so just use it
    #[serde(rename = "pic")]
    cover_url: String,
    #[serde(deserialize_with = "deserialize_title")]
    title: String,
    #[serde(alias = "desc")]
    description: String,
    #[serde(rename = "owner")]
    author: BiliUser,
    #[serde(rename = "staff")]
    partner: Option<Vec<BiliUser>>,
    cid: u64, // cid of the first video
    #[serde(rename = "pages")]
    videos: Vec<BiliVideoDetailItem>,
}
#[derive(Debug, Default, Deserialize)]
struct BiliVideoDetailItem {
    cid: u64,
    #[serde(rename = "part")]
    title: String,
}

impl Into<Track> for BiliVideoDetail {
    fn into(self) -> Track {
        Track {
            id: trackid_from(self.id, self.cid.to_string()),
            provider: Provider::Bilibili.into(),
            name: self.title,
            artists: vec![self.author]
                .into_iter()
                .chain(self.partner.into_iter().flatten())
                .map(|i| i.into())
                .collect(),
            cover: Some(Image::from(self.cover_url)),
        }
    }
}

impl Into<Playlist> for BiliVideoDetail {
    fn into(self) -> Playlist {
        let user_infos: Vec<Artist> = vec![self.author]
            .into_iter()
            .chain(self.partner.into_iter().flatten())
            .map(|i| i.into())
            .collect();
        let cover = Image::from(self.cover_url);
        Playlist {
            id: self.id.clone(),
            provider: Provider::Bilibili.into(),
            name: self.title,
            artists: user_infos.clone(),
            cover: Some(cover.clone()),
        }
    }
}

impl Into<PlaylistDetail> for BiliVideoDetail {
    fn into(self) -> PlaylistDetail {
        let user_details: Vec<ArtistDetail> = vec![self.author]
            .into_iter()
            .chain(self.partner.into_iter().flatten())
            .map(|i| i.into())
            .collect();
        let user_infos: Vec<Artist> = user_details
            .iter()
            .map(|i| i.artist.clone().unwrap())
            .collect();
        let cover = Image::from(self.cover_url);
        PlaylistDetail {
            id: self.id.clone(),
            provider: Provider::Bilibili.into(),
            name: self.title,
            artists: user_details,
            cover: Some(cover.clone()),
            description: Some(self.description),
            tracks: self
                .videos
                .into_iter()
                .map(|v| v.into_track_info(self.id.clone(), cover.clone(), user_infos.clone()))
                .collect(),
        }
    }
}

impl BiliVideoDetailItem {
    fn into_track_info(self, bvid: String, cover: Image, artists: Vec<Artist>) -> Track {
        Track {
            id: trackid_from(bvid, self.cid.to_string()),
            provider: Provider::Bilibili.into(),
            name: self.title,
            artists: artists,
            cover: Some(cover),
        }
    }
}

#[derive(Debug, Clone, Deserialize_repr, PartialEq)]
#[repr(u32)]
enum AudioQuality {
    Bps64k = 30216,
    Bps132k = 30232,
    Bps192k = 30280,
    Dolby = 30250,
    HiRes = 30251,
}

impl Into<String> for AudioQuality {
    fn into(self) -> String {
        match self {
            AudioQuality::Bps64k => "64kbps".to_string(),
            AudioQuality::Bps132k => "132kbps".to_string(),
            AudioQuality::Bps192k => "192kbps".to_string(),
            AudioQuality::Dolby => "dolby".to_string(),
            AudioQuality::HiRes => "flac".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct BiliVideoStream {
    dash: BiliDashStream,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct BiliDashStream {
    duration: u32, // TODO(xylonx): convert to time::Duration. unit is ms
    audio: Vec<BiliAudioDash>,
    dolby: BiliDolbyDash,
    flac: Option<BiliFlacDash>,
}
#[derive(Debug, Clone, Deserialize)]
struct BiliAudioDash {
    #[serde(rename = "id")]
    quality: AudioQuality,
    base_url: String,
    backup_url: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct BiliDolbyDash {
    #[serde(rename = "type")]
    dolby_type: u32, // 1:普通杜比音效; 2:全景杜比音效
    audio: Option<Vec<BiliAudioDash>>,
}
#[derive(Debug, Deserialize)]
struct BiliFlacDash {
    audio: BiliAudioDash,
}

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.4 Safari/605.1.15";

#[derive(Debug)]
pub struct BiliScraper {
    client: LimitedRequestClient,
}

fn trackid_from(bvid: String, cid: String) -> String {
    format!("{}::{}", bvid, cid)
}
fn trackid_into(id: String) -> Result<(String, String)> {
    if let Some((bvid, cid)) = id.split_once("::") {
        if !bvid.is_empty() && !cid.is_empty() {
            return Ok((bvid.to_string(), cid.to_string()));
        }
    }
    bail!(
        "trackID {} format is wrong. it should be like {{bvid}}::{{cid}}",
        id
    );
}

impl BiliScraper {
    pub async fn try_new(
        cookie_store: Arc<impl CookieStore + 'static>,
        channel_buffer_size: usize,
        request_buffer_size: usize,
        max_concurrency_number: usize,
        rate_limit_number: u64,
        rate_limit_duration: Duration,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(cookie_store)
            .build()?;

        let scraper = Self {
            client: LimitedRequestClient::new(
                client,
                channel_buffer_size,
                request_buffer_size,
                max_concurrency_number,
                rate_limit_number,
                rate_limit_duration,
            ),
        };

        let username = scraper.update_token().await?;
        info!("login as {}", username);

        Ok(scraper)
    }

    async fn update_token(&self) -> Result<String> {
        #[derive(Debug, Deserialize)]
        struct Status {
            #[serde(rename = "isLogin")]
            is_login: bool,
            #[serde(rename = "uname")]
            username: Option<String>,
        }

        let resp = self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse("https://api.bilibili.com/x/web-interface/nav")?,
            ))
            .await
            .with_context(|| "[Bili] get self failed")?
            .json::<BiliResponse<Status>>()
            .await
            .with_context(|| "[Bili] parse self failed")?;
        if resp.code != 0 || !resp.data.is_login || resp.data.username.is_none() {
            bail!("login failed: {}", resp.message)
        }
        Ok(resp.data.username.unwrap())
    }

    async fn get_suggest(&self, keyword: String) -> Result<Vec<BiliSuggestItem>> {
        Ok(self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    "https://s.search.bilibili.com/main/suggest",
                    &[("term", keyword)],
                )?,
            ))
            .await
            .with_context(|| "[Bili] get suggest failed")?
            .json::<HashMap<String, BiliSuggestItem>>()
            .await
            .with_context(|| "[Bili] parse suggest failed")?
            .into_iter()
            .map(|i| i.1)
            .collect())
    }

    async fn bsearch(
        &self,
        keyword: &String,
        page: i32,
        stype: SearchType,
    ) -> Result<Vec<search_item::Item>> {
        let typ = match stype {
            SearchType::User => "bili_user",
            SearchType::Video => "video",
        };

        let data = self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    "https://api.bilibili.com/x/web-interface/search/type",
                    &[
                        ("search_type", &typ.to_string()),
                        ("keyword", keyword),
                        ("page", &page.to_string()),
                    ],
                )?,
            ))
            .await
            .with_context(|| "[Bili] send search request failed")?
            .json::<BiliResponse<BiliSearchResponse>>()
            .await
            .with_context(|| "[Bili] parse search response to json failed")?
            .get_data()?;

        info!(
            "[Bili] search {} with page {} get {} results",
            keyword, page, data.page_size
        );

        futures::future::try_join_all(data.result.iter().map(|i| async move {
            match i {
                BiliSearchResultItem::User(u) => Ok(search_item::Item::from(u.clone())),
                BiliSearchResultItem::Video(v) => {
                    // get detail to check whether the video is a playlist
                    let vdetail = self.video_detail(v.id.clone()).await?;
                    if vdetail.video_number == 1 {
                        Ok(search_item::Item::Track(vdetail.into()))
                    } else {
                        Ok(search_item::Item::Playlist(vdetail.into()))
                    }
                }
            }
        }))
        .await
    }

    async fn video_detail(&self, id: String) -> Result<BiliVideoDetail> {
        Ok(self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    "https://api.bilibili.com/x/web-interface/view",
                    &[("bvid", &id)],
                )?,
            ))
            .await
            .with_context(|| format!("[Bili][id={}] send video detail request failed", &id))?
            .json::<BiliResponse<BiliVideoDetail>>()
            .await
            .with_context(|| format!("[Bili][id={}] parse to VideoDetail failed", &id))
            .and_then(|v| {
                (v.code == 0)
                    .then_some(v.data)
                    .ok_or_else(|| anyhow!("search video detail failed: {}", v.message))
            })?)
    }

    async fn video_stream(&self, id: String) -> Result<Vec<BiliAudioDash>> {
        let (bvid, cid) = trackid_into(id)?;
        let resp = self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    "https://api.bilibili.com/x/player/playurl",
                    &[
                        ("bvid", &bvid),
                        ("cid", &cid),
                        ("fnval", &(16 | 256).to_string()),
                    ],
                )?,
            ))
            .await?
            .json::<BiliResponse<BiliVideoStream>>()
            .await
            .with_context(|| format!("[Bili][bvid={}][cid={}] get stream failed", &bvid, &cid))
            .and_then(|v| {
                (v.code == 0)
                    .then_some(v.data)
                    .ok_or_else(|| anyhow!("search video detail failed: {}", v.message))
            })?;

        Ok(resp
            .dash
            .audio
            .into_iter()
            .chain(resp.dash.dolby.audio.into_iter().flatten())
            .chain(resp.dash.flac.into_iter().map(|i| i.audio))
            .collect())
    }
}

#[async_trait]
impl Scraper for BiliScraper {
    fn provider(&self) -> Provider {
        Provider::Bilibili
    }

    async fn suggest(&self, keyword: String) -> Result<Vec<Suggestion>> {
        Ok(self
            .get_suggest(keyword)
            .await?
            .into_iter()
            .map(|v| Suggestion {
                provider: self.provider().into(),
                suggestion: v.value,
            })
            .collect())
    }

    async fn search(
        &self,
        keyword: String,
        page: i32,
        fields: Vec<Zone>,
    ) -> Result<Vec<SearchItem>> {
        for zone in fields.iter() {
            if matches!(zone, Zone::Unspecified) {
                bail!("unknown search zone: {:?}", zone);
            }
        }

        Ok(futures::future::try_join_all(fields.iter().map(|zone| {
            let k = keyword.clone();
            async move {
                match zone {
                    Zone::Track => Ok(self
                        .bsearch(&k, page, SearchType::Video)
                        .await
                        .with_context(|| "[Bili] search video failed")?
                        .into_iter()
                        .filter(|v| matches!(v, search_item::Item::Track(_)))
                        .collect()),
                    Zone::Artist => Ok(self
                        .bsearch(&k, page, SearchType::User)
                        .await
                        .with_context(|| "[Bili] search artist failed")?),
                    Zone::Playlist => Ok(self
                        .bsearch(&k, page, SearchType::Video)
                        .await
                        .with_context(|| "[Bili] search video failed")?
                        .into_iter()
                        .filter(|v| matches!(v, search_item::Item::Playlist(_)))
                        .collect()),
                    Zone::Unspecified => bail!("unknown search zone: {:?}", zone),
                }
            }
        }))
        .await?
        .into_iter()
        .flatten()
        .map(|v| SearchItem { item: Some(v) })
        .collect())
    }

    async fn detail(&self, id: String, zone: Zone) -> Result<DetailItem> {
        match zone {
            Zone::Playlist => self.video_detail(id).await.map(|t| DetailItem {
                item: Some(detail_item::Item::Playlist(t.into())),
            }),
            Zone::Unspecified => bail!("unknown detail zone: {:?}", zone),
            _ => bail!("[Bilibili] detail zone {:?} not supported", zone),
        }
    }

    async fn stream(&self, id: String) -> Result<Vec<StreamItem>> {
        Ok(self
            .video_stream(id)
            .await?
            .into_iter()
            .map(|s| {
                let q = s.quality;
                let backup = s.backup_url;
                vec![StreamItem {
                    audio: Some(Stream {
                        provider: Provider::Bilibili.into(),
                        quality: q.clone().into(),
                        url: s.base_url,
                    }),
                    video: None,
                }]
                .into_iter()
                .chain(backup.into_iter().map(move |i| StreamItem {
                    audio: Some(Stream {
                        provider: Provider::Bilibili.into(),
                        quality: q.clone().into(),
                        url: i,
                    }),
                    video: None,
                }))
            })
            .flatten()
            .collect())
    }
}
