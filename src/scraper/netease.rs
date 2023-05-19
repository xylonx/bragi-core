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
use std::{format, matches, sync::Arc, time::Duration};

use anyhow::{bail, Result};
use async_trait::async_trait;
use log::info;
use rayon::prelude::*;
use reqwest::{cookie::CookieStore, Method, Request, Url};
use serde::Deserialize;

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

const USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.";

#[derive(Debug, Deserialize)]
struct NeteaseResponse<T> {
    code: i32,
    #[serde(flatten)]
    data: T,
}

#[derive(Debug, Deserialize)]
struct NeteaseResponseResult<T> {
    code: i32,
    #[serde(alias = "data")]
    result: T,
}

impl<T> NeteaseResponse<T> {
    fn data(self) -> Result<T> {
        if self.code == 200 {
            return Ok(self.data);
        }
        bail!("[Netease] call request failed: status code: {}", self.code);
    }
}

impl<T> NeteaseResponseResult<T> {
    fn data(self) -> Result<T> {
        if self.code == 200 {
            return Ok(self.result);
        }
        bail!("[Netease] call request failed: status code: {}", self.code);
    }
}

#[derive(Debug, Deserialize)]
struct NeteaseAccount {
    #[serde(alias = "userId", alias = "id")]
    user_id: i64,
    #[serde(alias = "userName")]
    nickname: String,
    #[serde(rename = "avatarUrl")]
    avatar_url: Option<String>,
    description: Option<String>,
}

impl Into<Artist> for NeteaseAccount {
    fn into(self) -> Artist {
        Artist {
            id: self.user_id.to_string(),
            provider: Provider::NeteaseMusic.into(),
            name: self.nickname,
        }
    }
}

impl Into<ArtistDetail> for NeteaseAccount {
    fn into(self) -> ArtistDetail {
        ArtistDetail {
            artist: Some(Artist {
                id: self.user_id.to_string(),
                provider: Provider::NeteaseMusic.into(),
                name: self.nickname,
            }),
            description: self.description,
            avatar: self.avatar_url.map(|i| Image {
                url: i,
                width: None,
                length: None,
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct NeteaseArtist {
    id: i64,
    name: String,
    #[serde(rename = "picUrl")]
    pic_url: Option<String>,
    #[serde(rename = "img1v1Url")]
    back_image_url: Option<String>,
}

impl Into<Artist> for NeteaseArtist {
    fn into(self) -> Artist {
        Artist {
            id: self.id.to_string(),
            provider: Provider::NeteaseMusic.into(),
            name: self.name,
        }
    }
}

impl Into<ArtistDetail> for NeteaseArtist {
    fn into(self) -> ArtistDetail {
        ArtistDetail {
            artist: Some(Artist {
                id: self.id.to_string(),
                provider: Provider::NeteaseMusic.into(),
                name: self.name,
            }),
            description: None,
            avatar: self.pic_url.or(self.back_image_url).map(|v| Image {
                url: v,
                width: None,
                length: None,
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct NeteaseSong {
    id: i64,
    name: String,
    #[serde(alias = "ar")]
    artists: Vec<NeteaseArtist>,
    #[serde(alias = "al")]
    album: Option<NeteaseAlbum>,
}

#[derive(Debug, Deserialize)]
struct NeteaseAlbum {
    #[serde(rename = "picUrl")]
    pic_url: Option<String>,
}

impl Into<Track> for NeteaseSong {
    fn into(self) -> Track {
        Track {
            id: self.id.to_string(),
            provider: Provider::NeteaseMusic.into(),
            name: self.name,
            artists: self.artists.into_iter().map(Into::into).collect(),
            cover: self
                .album
                .map(|a| {
                    a.pic_url.map(|p| Image {
                        url: p,
                        width: None,
                        length: None,
                    })
                })
                .flatten(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct NeteasePlaylist {
    id: i64,
    name: String,
    #[serde(rename = "coverImgUrl")]
    cover_url: Option<String>,
    creator: NeteaseAccount,
    description: Option<String>,
}

impl Into<Playlist> for NeteasePlaylist {
    fn into(self) -> Playlist {
        Playlist {
            id: self.id.to_string(),
            provider: Provider::NeteaseMusic.into(),
            name: self.name,
            artists: vec![self.creator.into()],
            cover: self.cover_url.map(|i| Image {
                url: i,
                width: None,
                length: None,
            }),
        }
    }
}

///

#[derive(Debug, Deserialize)]
struct NeteaseAccountResponse {
    account: NeteaseAccount,
}

#[derive(Debug, Deserialize)]
struct NeteaseSearchSuggest {
    artists: Vec<NeteaseArtist>,
    songs: Vec<NeteaseSong>,
}

enum SearchType {
    TRACK = 1,
    ARTIST = 100,
    PLAYLIST = 1000,
}

impl SearchType {
    fn value(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NeteaseSearch {
    SONG { songs: Vec<NeteaseSong> },
    PLAYLIST { playlists: Vec<NeteasePlaylist> },
    ARTIST { artists: Vec<NeteaseArtist> },
}

#[derive(Debug, Deserialize)]
struct NeteasePlaylistDetailResp {
    playlist: NeteasePlaylistDetail,
}

#[derive(Debug, Deserialize)]
struct NeteasePlaylistDetail {
    #[serde(flatten)]
    basic_info: NeteasePlaylist,
    #[serde(rename = "trackIds")]
    track_ids: Vec<NeteasePlaylistTrackID>,
}
#[derive(Debug, Deserialize)]
struct NeteasePlaylistTrackID {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct NeteaseSongDetail {
    songs: Vec<NeteaseSong>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct NeteaseSongDownload {
    url: String,
    #[serde(rename = "br")]
    bitrate: u64,
}

/// below are function impl

#[derive(Debug)]
pub struct NeteaseScraper {
    base_url: String,
    client: LimitedRequestClient,
}

impl NeteaseScraper {
    pub async fn try_new(
        instance: String,
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
            base_url: instance,
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
        Ok(self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse(format!("{}/user/account", self.base_url).as_str())?,
            ))
            .await?
            .json::<NeteaseResponse<NeteaseAccountResponse>>()
            .await?
            .data()?
            .account
            .user_id
            .to_string())
    }
}

impl NeteaseScraper {
    async fn nsearch(
        &self,
        keyword: String,
        offset: i32,
        search_type: SearchType,
    ) -> Result<Vec<SearchItem>> {
        let resp = self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    format!("{}/cloudsearch", self.base_url).as_str(),
                    &[
                        ("realIP", "116.25.146.177"),
                        ("keywords", &keyword),
                        ("type", &search_type.value().to_string()),
                        ("offset", &offset.to_string()),
                    ],
                )?,
            ))
            .await?
            .json::<NeteaseResponseResult<NeteaseSearch>>()
            .await?
            .data()?;

        Ok(match resp {
            NeteaseSearch::SONG { songs } => songs
                .into_par_iter()
                .map(|i| SearchItem {
                    item: Some(search_item::Item::Track(i.into())),
                })
                .collect(),
            NeteaseSearch::PLAYLIST { playlists } => playlists
                .into_par_iter()
                .map(|i| SearchItem {
                    item: Some(search_item::Item::Playlist(i.into())),
                })
                .collect(),
            NeteaseSearch::ARTIST { artists } => artists
                .into_par_iter()
                .map(|i| SearchItem {
                    item: Some(search_item::Item::User(i.into())),
                })
                .collect(),
        })
    }

    async fn playlist_detail(&self, id: String) -> Result<NeteasePlaylistDetail> {
        Ok(self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    format!("{}/playlist/detail", self.base_url).as_str(),
                    &[("realIP", "116.25.146.177"), ("id", &id)],
                )?,
            ))
            .await?
            .json::<NeteaseResponse<NeteasePlaylistDetailResp>>()
            .await?
            .data()?
            .playlist)
    }

    // ids: concated with ,
    async fn batch_songs(&self, ids: String) -> Result<Vec<NeteaseSong>> {
        Ok(self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    format!("{}/song/detail", self.base_url).as_str(),
                    &[("realIP", "116.25.146.177"), ("ids", &ids)],
                )?,
            ))
            .await?
            .json::<NeteaseResponse<NeteaseSongDetail>>()
            .await?
            .data()?
            .songs)
    }
}

#[async_trait]
impl Scraper for NeteaseScraper {
    fn provider(&self) -> Provider {
        Provider::NeteaseMusic
    }

    async fn suggest(&self, keyword: String) -> Result<Vec<Suggestion>> {
        let resp = self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    format!("{}/search/suggest", self.base_url).as_str(),
                    &[("realIP", "116.25.146.177"), ("keywords", &keyword)],
                )?,
            ))
            .await?
            .json::<NeteaseResponseResult<NeteaseSearchSuggest>>()
            .await?
            .data()?;
        Ok(resp
            .artists
            .into_par_iter()
            .map(|i| Suggestion {
                provider: self.provider().into(),
                suggestion: i.name,
            })
            .chain(resp.songs.into_par_iter().map(|i| Suggestion {
                provider: self.provider().into(),
                suggestion: i.name,
            }))
            .collect())
    }

    async fn search(
        &self,
        keyword: String,
        page: i32,
        zones: Vec<Zone>,
    ) -> Result<Vec<SearchItem>> {
        Ok(futures::future::try_join_all(zones.into_iter().map(|z| {
            let offset = (page - 1) * 30;
            let k = keyword.clone();
            async move {
                match z {
                    Zone::Track => self.nsearch(k, offset, SearchType::TRACK).await,
                    Zone::Artist => self.nsearch(k, offset, SearchType::ARTIST).await,
                    Zone::Playlist => self.nsearch(k, offset, SearchType::PLAYLIST).await,
                    Zone::Unspecified => bail!("[Netease] unknown zone: {:?}", z),
                }
            }
        }))
        .await?
        .into_par_iter()
        .flatten()
        .collect())
    }

    async fn detail(&self, id: String, zone: Zone) -> Result<DetailItem> {
        if !matches!(zone, Zone::Playlist) {
            bail!("[Netease] unsupoorted zone: {:?}", zone);
        }
        let playlist = self.playlist_detail(id).await?;
        let tracks = self
            .batch_songs(
                playlist
                    .track_ids
                    .into_par_iter()
                    .map(|i| i.id.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            )
            .await?;
        Ok(DetailItem {
            item: Some(detail_item::Item::Playlist(PlaylistDetail {
                id: playlist.basic_info.id.to_string(),
                provider: self.provider().into(),
                name: playlist.basic_info.name,
                artists: vec![playlist.basic_info.creator.into()],
                cover: playlist.basic_info.cover_url.map(|i| Image {
                    url: i,
                    width: None,
                    length: None,
                }),
                description: playlist.basic_info.description,
                tracks: tracks.into_par_iter().map(Into::into).collect(),
            })),
        })
    }

    async fn stream(&self, id: String) -> Result<Vec<StreamItem>> {
        let resp = self
            .client
            .call(Request::new(
                Method::GET,
                Url::parse_with_params(
                    format!("{}/song/download/url", self.base_url).as_str(),
                    &[("realIP", "116.25.146.177"), ("id", &id)],
                )?,
            ))
            .await?
            .json::<NeteaseResponseResult<NeteaseSongDownload>>()
            .await?
            .data()?;
        Ok(vec![StreamItem {
            video: None,
            audio: Some(Stream {
                provider: self.provider().into(),
                url: resp.url,
                quality: "lossless".to_string(),
            }),
        }])
    }
}
