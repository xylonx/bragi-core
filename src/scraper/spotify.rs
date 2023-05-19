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
use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use librespot::{
    core::{cache::Cache, Session, SessionConfig, SpotifyId},
    discovery::Credentials,
    playback::{
        audio_backend,
        config::{AudioFormat, Bitrate, PlayerConfig},
        mixer::NoOpVolume,
        player::Player,
    },
};
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use rspotify::{
    model::{PlayableItem, PlaylistId, SearchResult, SearchType},
    prelude::BaseClient,
};

use crate::bragi::{
    detail_response::{detail_item, DetailItem},
    search_response::{search_item, SearchItem},
    stream_response::StreamItem,
    suggest_response::Suggestion,
    Artist, ArtistDetail, Image, Playlist, PlaylistDetail, Provider, Stream, Track, Zone,
};

use super::Scraper;

impl Into<Image> for rspotify::model::image::Image {
    fn into(self) -> Image {
        Image {
            url: self.url,
            width: self.width.map(Into::into),
            length: self.width.map(Into::into),
        }
    }
}

impl TryInto<Artist> for rspotify::model::artist::SimplifiedArtist {
    type Error = anyhow::Error;
    fn try_into(self) -> Result<Artist> {
        Ok(Artist {
            id: self
                .id
                .map(|i| i.to_string())
                .ok_or_else(|| anyhow!("[Spotify] artist {} id is empty", self.name))?,
            provider: Provider::Spotify.into(),
            name: self.name,
        })
    }
}

impl Into<Artist> for rspotify::model::artist::FullArtist {
    fn into(self) -> Artist {
        Artist {
            id: self.id.to_string(),
            provider: Provider::Spotify.into(),
            name: self.name,
        }
    }
}

impl Into<Artist> for rspotify::model::user::PublicUser {
    fn into(self) -> Artist {
        Artist {
            id: self.id.to_string(),
            provider: Provider::Spotify.into(),
            name: self.display_name.unwrap_or(self.id.to_string()),
        }
    }
}

impl Into<ArtistDetail> for rspotify::model::user::PublicUser {
    fn into(self) -> ArtistDetail {
        ArtistDetail {
            artist: Some(Artist {
                id: self.id.to_string(),
                provider: Provider::Spotify.into(),
                name: self.display_name.unwrap_or(self.id.to_string()),
            }),
            description: None,
            avatar: self
                .images
                .into_par_iter()
                .max_by(|x, y| x.width.cmp(&y.width))
                .map(Into::into),
        }
    }
}

impl Into<ArtistDetail> for rspotify::model::artist::FullArtist {
    fn into(self) -> ArtistDetail {
        ArtistDetail {
            artist: Some(Artist {
                id: self.id.to_string(),
                provider: Provider::Spotify.into(),
                name: self.name,
            }),
            description: None,
            avatar: self
                .images
                .into_par_iter()
                .max_by(|x, y| x.width.cmp(&y.width))
                .map(Into::into),
        }
    }
}

impl Into<Playlist> for rspotify::model::playlist::SimplifiedPlaylist {
    fn into(self) -> Playlist {
        Playlist {
            id: self.id.to_string(),
            provider: Provider::Spotify.into(),
            name: self.name,
            artists: vec![self.owner.into()],
            cover: self
                .images
                .into_par_iter()
                .max_by(|x, y| x.width.cmp(&y.width))
                .map(Into::into),
        }
    }
}

impl TryInto<Track> for rspotify::model::track::FullTrack {
    type Error = anyhow::Error;
    fn try_into(self) -> Result<Track> {
        Ok(Track {
            id: self
                .id
                .map(|i| i.to_string())
                .ok_or_else(|| anyhow!("[Spotify] track id is empty"))?,
            provider: Provider::Spotify.into(),
            name: self.name,
            artists: self
                .artists
                .into_par_iter()
                .filter_map(|i| i.try_into().ok())
                .collect(),
            cover: self
                .album
                .images
                .into_par_iter()
                .max_by(|x, y| x.width.cmp(&y.width))
                .map(Into::into),
        })
    }
}

#[allow(unused)]
pub struct SpotifyScraper {
    credentials: Credentials,
    session_config: SessionConfig,
    player_config: PlayerConfig,
    cache: Cache,
    session: Session,
    static_dir: PathBuf,

    client_id: String,
    client_secret: String,
    client: rspotify::ClientCredsSpotify,
}

impl SpotifyScraper {
    /// will cache credential and audios to ${cache_dir}/credential and ${cache_dir}/audio separately
    pub async fn try_new(
        username: String,
        password: String,
        client_id: String,
        client_secret: String,
        cache_dir: PathBuf,
        static_dir: PathBuf,
    ) -> Result<Self> {
        let session_config = SessionConfig {
            tmp_dir: cache_dir.clone().join("/tmp"),
            ..SessionConfig::default()
        };
        let player_config = PlayerConfig {
            bitrate: Bitrate::Bitrate320,
            passthrough: true,
            ..PlayerConfig::default()
        };
        let credentials = Credentials::with_password(username, password);
        let cache = Cache::new(
            Some(cache_dir.clone().join("credential")),
            None,
            Some(cache_dir.clone().join("audio")),
            None,
        )?;

        let session = Session::new(session_config.clone(), Some(cache.clone()));
        session.connect(credentials.clone(), true).await?;

        let rcred = rspotify::Credentials {
            id: client_id.clone(),
            secret: Some(client_secret.clone()),
        };
        let rspotclient = rspotify::ClientCredsSpotify::new(rcred);
        rspotclient.request_token().await?;

        Ok(Self {
            credentials,
            session_config,
            player_config,
            cache,
            static_dir,
            session,

            client_id: client_id,
            client_secret: client_secret,
            client: rspotclient,
        })
    }

    async fn store_audio(&self, track_id: SpotifyId) -> Result<PathBuf> {
        let file = self
            .static_dir
            .clone()
            .join(format!("{}.ogg", track_id.to_string()));
        let filename = file
            .clone()
            .to_str()
            .ok_or_else(|| anyhow!("[Spotify] audio file path {:?} not valid", file))?
            .to_string();

        // pipe backend exists in all features. Therefore, it is SAFE here to unwrap
        let backend = audio_backend::find(Some("pipe".into())).unwrap();

        let mut player = Player::new(
            self.player_config.clone(),
            self.session.clone(),
            Box::new(NoOpVolume),
            move || backend(Some(filename), AudioFormat::F64),
        );
        player.load(track_id, true, 0);
        println!("playing");

        // FIXME(xylonx): When occur error with 'Track should be available, but no alternatives found', below instruction will never success
        player.await_end_of_track().await;

        Ok(file)
    }

    async fn sposearch(
        &self,
        keyword: String,
        limit: u32,
        offset: u32,
        field: SearchType,
    ) -> Result<Vec<SearchItem>> {
        let resp = self
            .client
            .search(&keyword, field, None, None, Some(limit), Some(offset))
            .await?;
        match resp {
            SearchResult::Artists(a) => Ok(a
                .items
                .into_par_iter()
                .map(|i| SearchItem {
                    item: Some(search_item::Item::User(i.into())),
                })
                .collect()),
            SearchResult::Playlists(p) => Ok(p
                .items
                .into_par_iter()
                .map(|i| SearchItem {
                    item: Some(search_item::Item::Playlist(i.into())),
                })
                .collect()),
            SearchResult::Tracks(t) => Ok(t
                .items
                .into_par_iter()
                .filter_map(|i| {
                    i.try_into()
                        .map(|j| SearchItem {
                            item: Some(search_item::Item::Track(j)),
                        })
                        .ok()
                })
                .collect()),
            _ => bail!("[Spotify] unknown search result: {:?}", resp),
        }
    }
}

#[async_trait]
impl Scraper for SpotifyScraper {
    fn provider(&self) -> Provider {
        Provider::Spotify
    }

    async fn suggest(&self, keyword: String) -> Result<Vec<Suggestion>> {
        Ok(futures::future::try_join_all(
            vec![SearchType::Track, SearchType::Artist, SearchType::Playlist]
                .into_iter()
                .map(|t| {
                    let k = keyword.clone();
                    async move { self.sposearch(k, 5, 0, t).await }
                }),
        )
        .await?
        .into_par_iter()
        .flatten()
        .map(|i| Suggestion {
            provider: self.provider().into(),
            suggestion: match i.item.unwrap() {
                search_item::Item::Playlist(p) => p.name,
                search_item::Item::Track(p) => p.name,
                search_item::Item::User(p) => p.artist.unwrap().name,
            },
        })
        .collect())
    }

    async fn search(
        &self,
        keyword: String,
        page: i32,
        fields: Vec<Zone>,
    ) -> Result<Vec<SearchItem>> {
        Ok(futures::future::try_join_all(
            fields
                .into_iter()
                .map(|f| match f {
                    Zone::Artist => SearchType::Artist,
                    Zone::Playlist => SearchType::Playlist,
                    Zone::Track | Zone::Unspecified => SearchType::Track,
                })
                .map(|t| {
                    let k = keyword.clone();
                    async move { self.sposearch(k, 20, (page as u32 - 1) * 20, t).await }
                }),
        )
        .await?
        .into_par_iter()
        .flatten()
        .collect())
    }

    async fn detail(&self, id: String, zone: Zone) -> Result<DetailItem> {
        if !matches!(zone, Zone::Playlist) {
            bail!("[Spotify] unsupported zone: {:?}", zone);
        }
        let playlist_id = PlaylistId::from_id_or_uri(&id)?;
        let playlist = self
            .client
            .playlist(playlist_id.clone(), None, None)
            .await?;
        let tracks = self.client.playlist_items(playlist_id, None, None);

        Ok(DetailItem {
            item: Some(detail_item::Item::Playlist(PlaylistDetail {
                id: playlist.id.to_string(),
                provider: self.provider().into(),
                name: playlist.name,
                artists: vec![playlist.owner.into()],
                cover: playlist
                    .images
                    .into_par_iter()
                    .max_by(|x, y| x.width.cmp(&y.width))
                    .map(Into::into),
                description: playlist.description,
                tracks: tracks
                    .filter_map(|v| async move {
                        match v {
                            Ok(v) => match v.track {
                                Some(v) => match v {
                                    PlayableItem::Track(t) => Some(t.try_into()),
                                    _ => None,
                                },
                                None => None,
                            },
                            Err(e) => {
                                Some(Err(anyhow!("[Spotify] fetch playlist item failed: {}", e)))
                            }
                        }
                    })
                    .try_collect()
                    .await?,
            })),
        })
    }

    async fn stream(&self, id: String) -> Result<Vec<StreamItem>> {
        let path = self.store_audio(SpotifyId::from_uri(&id)?).await?;
        Ok(vec![StreamItem {
            video: None,
            audio: Some(Stream {
                provider: self.provider().into(),
                quality: format!("{:?}", Bitrate::Bitrate320),
                // TODO(xylonx): host it by http instead of local path
                url: path.to_str().unwrap().to_string(),
            }),
        }])
    }
}
