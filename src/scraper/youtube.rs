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
///
/// Use invidious instead for its better api experience.
/// You can find all certificated instances at https://docs.invidious.io/instances/
/// And you can find the api at https://docs.invidious.io/api/
///
use std::{cmp::Reverse, format, time::Duration, vec};

use anyhow::{bail, Result};
use async_trait::async_trait;
use lazy_static::lazy_static;
use reqwest::{Method, Request, Url};
use serde::{Deserialize, Deserializer};

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

#[derive(Debug, Clone, Deserialize)]
struct InvidiousThumbnail {
    #[allow(dead_code)]
    quality: Option<String>,
    url: String,
    width: i32,
    height: i32,
}

impl Into<Image> for InvidiousThumbnail {
    fn into(self) -> Image {
        Image {
            url: self.url,
            width: Some(self.width.into()),
            length: Some(self.height.into()),
        }
    }
}

fn thumbnails_to_image(thumbnails: Vec<InvidiousThumbnail>) -> Option<Image> {
    thumbnails
        .into_iter()
        .max_by(|x, y| x.width.cmp(&y.width))
        .map(Into::into)
}

fn deserialize_html_unescape<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Vec<String> = Deserialize::deserialize(deserializer)?;
    Result::Ok(
        s.into_iter()
            .map(|i| html_escape::decode_html_entities(i.as_str()).to_string())
            .collect(),
    )
}

#[derive(Debug, Deserialize)]
struct InvidiousSuggest {
    #[serde(deserialize_with = "deserialize_html_unescape")]
    suggestions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum InvidiousSearchResponse {
    #[serde(rename = "video")]
    SearchVideo(InvidiousSearchVideo),
    #[serde(rename = "channel")]
    SearchUser(InvidiousSearchChannel),
    #[serde(rename = "playlist")]
    SearchPlaylist(InvidiousSearchPlaylist),
}

impl Into<SearchItem> for InvidiousSearchResponse {
    fn into(self) -> SearchItem {
        SearchItem {
            item: Some(match self {
                Self::SearchVideo(v) => v.into(),
                Self::SearchUser(u) => u.into(),
                Self::SearchPlaylist(p) => p.into(),
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct InvidiousSearchVideo {
    #[serde(rename = "videoId")]
    id: String,
    title: String,
    #[serde(rename = "author")]
    author_name: String,
    #[serde(rename = "authorId")]
    author_id: String,
    #[serde(rename = "videoThumbnails")]
    video_covers: Vec<InvidiousThumbnail>,
}
impl InvidiousSearchVideo {
    fn search_fields() -> Vec<String> {
        vec![
            "type".into(),
            "videoId".into(),
            "title".into(),
            "author".into(),
            "authorId".into(),
            "videoThumbnails".into(),
        ]
    }
}

impl Into<search_item::Item> for InvidiousSearchVideo {
    fn into(self) -> search_item::Item {
        search_item::Item::Track(Track {
            id: self.id,
            provider: Provider::Youtube.into(),
            name: self.title,
            artists: vec![Artist {
                id: self.author_id,
                provider: Provider::Bilibili.into(),
                name: self.author_name,
            }],
            cover: thumbnails_to_image(self.video_covers),
        })
    }
}

#[derive(Debug, Deserialize)]
struct InvidiousSearchChannel {
    #[serde(rename = "authorId")]
    id: String,
    #[serde(rename = "author")]
    name: String,
    description: String,
    #[serde(rename = "authorThumbnails")]
    thumbnails: Vec<InvidiousThumbnail>,
}
impl InvidiousSearchChannel {
    fn search_fields() -> Vec<String> {
        vec![
            "type".into(),
            "authorId".into(),
            "author".into(),
            "description".into(),
            "authorThumbnails".into(),
        ]
    }
}

impl Into<search_item::Item> for InvidiousSearchChannel {
    fn into(self) -> search_item::Item {
        search_item::Item::User(ArtistDetail {
            artist: Some(Artist {
                id: self.id,
                provider: Provider::Youtube.into(),
                name: self.name,
            }),
            description: Some(self.description),
            avatar: thumbnails_to_image(self.thumbnails),
        })
    }
}

#[derive(Debug, Deserialize)]
struct InvidiousSearchPlaylist {
    #[serde(rename = "playlistId")]
    id: String,
    title: String,
    #[serde(rename = "playlistThumbnail")]
    cover_url: String,
    #[serde(rename = "author")]
    author_name: String,
    #[serde(rename = "authorId")]
    author_id: String,
}
impl InvidiousSearchPlaylist {
    fn search_fields() -> Vec<String> {
        vec![
            "type".into(),
            "title".into(),
            "playlistThumbnail".into(),
            "playlistId".into(),
            "author".into(),
            "authorId".into(),
        ]
    }
}

impl Into<search_item::Item> for InvidiousSearchPlaylist {
    fn into(self) -> search_item::Item {
        let artists = vec![Artist {
            id: self.author_id,
            provider: Provider::Youtube.into(),
            name: self.author_name,
        }];

        search_item::Item::Playlist(Playlist {
            id: self.id,
            provider: Provider::Youtube.into(),
            name: self.title,
            cover: Some(Image {
                url: self.cover_url,
                width: None,
                length: None,
            }),
            artists, // put author behind tracks to move it instead of another useless clone
        })
    }
}

#[derive(Debug, Deserialize)]
struct InvidiousVideoDetail {
    #[serde(rename = "adaptiveFormats")]
    adaptive_formats_streams: Vec<InvidiousFormatsStream>,
}

impl InvidiousVideoDetail {
    fn search_fields() -> Vec<String> {
        vec!["adaptiveFormats".into()]
    }
}

#[derive(Debug, Deserialize)]
struct InvidiousFormatsStream {
    url: String,
    #[serde(rename = "audioQuality")]
    audio_quality: AudioQuality,
}

impl Into<Stream> for InvidiousFormatsStream {
    fn into(self) -> Stream {
        Stream {
            provider: Provider::Youtube.into(),
            quality: format!("{:?}", self.audio_quality),
            url: self.url,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum AudioQuality {
    #[serde(rename = "AUDIO_QUALITY_ULTRALOW")]
    ULTRALOW,
    #[serde(rename = "AUDIO_QUALITY_LOW")]
    LOW,
    #[serde(rename = "AUDIO_QUALITY_MEDIUM")]
    MEDIUM,
    #[serde(rename = "AUDIO_QUALITY_HIGH")]
    HIGH,
}

#[derive(Debug, Deserialize)]
struct InvidiousPlaylistDetail {
    #[serde(rename = "playlistId")]
    id: String,
    title: String,
    #[serde(rename = "author")]
    author_name: String,
    #[serde(rename = "authorId")]
    author_id: String,
    #[serde(rename = "authorThumbnails")]
    author_thumbnails: Vec<InvidiousThumbnail>,
    description: String,
    videos: Vec<InvidiousPlaylistVideoItem>,
}
impl InvidiousPlaylistDetail {
    fn search_fields() -> Vec<String> {
        vec![
            "type".into(),
            "title".into(),
            "playlistId".into(),
            "author".into(),
            "authorId".into(),
            "authorThumbnails".into(),
            "description".into(),
            "videos".into(),
        ]
    }
}

#[derive(Debug, Deserialize)]
struct InvidiousPlaylistVideoItem {
    #[serde(rename = "videoId")]
    id: String,
    title: String,
    #[serde(rename = "videoThumbnails")]
    covers: Vec<InvidiousThumbnail>,
    #[serde(rename = "author")]
    author_name: String,
    #[serde(rename = "authorId")]
    author_id: String,
}

impl Into<Track> for InvidiousPlaylistVideoItem {
    fn into(self) -> Track {
        Track {
            id: self.id,
            provider: Provider::Youtube.into(),
            name: self.title,
            artists: vec![Artist {
                id: self.author_id,
                provider: Provider::Youtube.into(),
                name: self.author_name,
            }],
            cover: thumbnails_to_image(self.covers),
        }
    }
}

impl Into<PlaylistDetail> for InvidiousPlaylistDetail {
    fn into(self) -> PlaylistDetail {
        PlaylistDetail {
            id: self.id,
            provider: Provider::Youtube.into(),
            name: self.title,
            artists: vec![ArtistDetail {
                artist: Some(Artist {
                    id: self.author_id,
                    provider: Provider::Youtube.into(),
                    name: self.author_name,
                }),
                description: None,
                avatar: thumbnails_to_image(self.author_thumbnails),
            }],
            cover: thumbnails_to_image(self.videos[0].covers.clone()),
            description: Some(self.description),
            tracks: self.videos.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug)]
pub struct YouTubeScraper {
    instance: String,
    client: LimitedRequestClient,
}

lazy_static! {
    static ref USER_AGENT: &'static str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.4 Safari/605.1.15";
}

impl YouTubeScraper {
    pub fn new(
        instance: String,
        channel_buffer_size: usize,
        request_buffer_size: usize,
        max_concurrency_number: usize,
        rate_limit_number: u64,
        rate_limit_duration: Duration,
    ) -> Self {
        Self {
            instance,
            client: LimitedRequestClient::new(
                reqwest::Client::builder()
                    .user_agent(USER_AGENT.clone())
                    .build()
                    .unwrap(),
                channel_buffer_size,
                request_buffer_size,
                max_concurrency_number,
                rate_limit_number,
                rate_limit_duration,
            ),
        }
    }

    async fn ysearch(&self, keyword: &String, page: i32, zone: Zone) -> Result<Vec<SearchItem>> {
        let (search_type, search_fields) = match zone {
            Zone::Track => ("video", InvidiousSearchVideo::search_fields().join(",")),
            Zone::Playlist => (
                "playlist",
                InvidiousSearchPlaylist::search_fields().join(","),
            ),
            Zone::Artist => ("channel", InvidiousSearchChannel::search_fields().join(",")),
            _ => bail!("[YouTube] unsupported search zone: {:?}", zone),
        };

        Ok(self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    format!("{}/api/v1/search", self.instance).as_str(),
                    &[
                        ("q", keyword),
                        ("page", &page.to_string()),
                        ("type", &search_type.to_string()),
                        ("fields", &search_fields),
                    ],
                )?,
            ))
            .await?
            .json::<Vec<InvidiousSearchResponse>>()
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn video_stream(&self, id: String) -> Result<Vec<StreamItem>> {
        let mut resp = self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    format!("{}/api/v1/videos/{}", self.instance, id).as_str(),
                    &[("fields", InvidiousVideoDetail::search_fields().join(","))],
                )?,
            ))
            .await?
            .json::<InvidiousVideoDetail>()
            .await?
            .adaptive_formats_streams;
        resp.sort_by_cached_key(|w| Reverse(w.audio_quality.clone()));
        Ok(resp
            .into_iter()
            .map(|s| StreamItem {
                audio: Some(s.into()),
                video: None,
            })
            .collect())
    }

    async fn playlist_detail(&self, id: String) -> Result<PlaylistDetail> {
        Ok(self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    format!("{}/api/v1/playlists/{}", self.instance, id).as_str(),
                    &[("fields", InvidiousPlaylistDetail::search_fields().join(","))],
                )?,
            ))
            .await?
            .json::<InvidiousPlaylistDetail>()
            .await?
            .into())
    }
}

#[async_trait]
impl Scraper for YouTubeScraper {
    fn provider(&self) -> Provider {
        Provider::Youtube
    }

    async fn suggest(&self, keyword: String) -> Result<Vec<Suggestion>> {
        Ok(self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    format!("{}/api/v1/search/suggestions", self.instance).as_str(),
                    &[("q", keyword)],
                )?,
            ))
            .await?
            .json::<InvidiousSuggest>()
            .await?
            .suggestions
            .into_iter()
            .map(|s| Suggestion {
                provider: self.provider().into(),
                suggestion: s,
            })
            .collect())
    }

    async fn search(
        &self,
        keyword: String,
        page: i32,
        fields: Vec<Zone>,
    ) -> Result<Vec<SearchItem>> {
        Ok(futures::future::try_join_all(fields.iter().map(|zone| {
            let k = keyword.clone();
            async move { self.ysearch(&k, page, zone.clone()).await }
        }))
        .await?
        .into_iter()
        .flatten()
        .collect())
    }

    async fn detail(&self, id: String, zone: Zone) -> Result<DetailItem> {
        match zone {
            Zone::Playlist => Ok(self.playlist_detail(id).await.map(|v| DetailItem {
                item: Some(detail_item::Item::Playlist(v)),
            })?),
            _ => bail!("[YouTube] detail zone {:?} not supported", zone),
        }
    }

    async fn stream(&self, id: String) -> Result<Vec<StreamItem>> {
        self.video_stream(id).await
    }
}
