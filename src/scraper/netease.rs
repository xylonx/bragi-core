use std::{format, sync::Arc};

use anyhow::bail;
use async_trait::async_trait;
use serde::{Deserialize, Deserializer};
use tracing::{error, info};

use crate::{
    settings::NeteaseSettings,
    util::{self, cookie::PersistCookieStore},
};

use super::{Artist, ScrapeItem, ScrapeType, Scraper, Song, SongCollection, Stream};

/// cover pic id to pic url
fn deserialize_pic_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<i64> = Deserialize::deserialize(deserializer)?;
    Result::Ok(s.map(|id| format!("https://music.163.com/api/img/blur/{}.jpg", id)))
}

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
    fn data(self) -> anyhow::Result<T> {
        if self.code == 200 {
            return Ok(self.data);
        }
        bail!("[Netease] call request failed: status code: {}", self.code);
    }
}

impl<T> NeteaseResponseResult<T> {
    fn data(self) -> anyhow::Result<T> {
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

impl From<NeteaseAccount> for Artist {
    fn from(val: NeteaseAccount) -> Self {
        Artist {
            id: val.user_id.to_string(),
            name: val.nickname,
            description: val.description,
            avatar: val.avatar_url,
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

impl From<NeteaseArtist> for Artist {
    fn from(val: NeteaseArtist) -> Self {
        Artist {
            id: val.id.to_string(),
            name: val.name,
            description: None,
            avatar: val.pic_url.or(val.back_image_url).map(Into::into),
        }
    }
}

#[derive(Debug, Deserialize)]
struct NeteaseSong {
    id: i64,
    name: String,
    duration: Option<u32>, // unit ms
    #[serde(alias = "ar", default)]
    artists: Vec<NeteaseArtist>,
    #[serde(alias = "al")]
    album: Option<NeteaseAlbum>,
}

#[derive(Debug, Deserialize)]
struct NeteaseAlbum {
    id: i64,
    name: String,
    #[serde(rename = "picUrl")]
    pic_url: Option<String>,
    #[serde(rename = "picId", deserialize_with = "deserialize_pic_id")]
    pic_id: Option<String>,
    artist: NeteaseArtist,
}

impl From<NeteaseAlbum> for SongCollection {
    fn from(value: NeteaseAlbum) -> Self {
        Self {
            id: value.id.to_string(),
            name: value.name,
            artists: vec![value.artist.into()],
            cover: value.pic_url,
            description: None,
            songs: vec![],
        }
    }
}

impl From<NeteaseSong> for Song {
    fn from(val: NeteaseSong) -> Self {
        Song {
            id: val.id.to_string(),
            name: val.name,
            // Choose album image as default cover. Otherwise, choose the first artist image as back cover.
            cover: val
                .album
                .and_then(|a| a.pic_url.or(a.pic_id))
                .or(val.artists.first().and_then(|a| a.pic_url.clone())),
            artists: val.artists.into_iter().map(Into::into).collect(),
            duration: val.duration.map(|v| v / 1000),
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

impl From<NeteasePlaylist> for SongCollection {
    fn from(val: NeteasePlaylist) -> Self {
        SongCollection {
            id: val.id.to_string(),
            name: val.name,
            artists: vec![val.creator.into()],
            cover: val.cover_url.map(Into::into),
            description: val.description,
            songs: vec![],
        }
    }
}

#[derive(Debug, Deserialize)]
struct NeteaseSearchSuggest {
    #[serde(default)]
    artists: Vec<NeteaseArtist>,
    songs: Vec<NeteaseSong>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NeteaseSearch {
    Album { albums: Vec<NeteaseAlbum> },
    Playlist { playlists: Vec<NeteasePlaylist> },
    Song { songs: Vec<NeteaseSong> },
    Artist { artists: Vec<NeteaseArtist> },
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
    url: Option<String>,
    #[serde(rename = "br")]
    bitrate: u64,
}

#[derive(Debug)]
pub struct NeteaseScraper {
    instance: String,
    client: reqwest::Client,
}

impl NeteaseScraper {
    pub fn new(instance: String, client: reqwest::Client) -> Self {
        Self { instance, client }
    }

    pub fn try_from_setting(setting: NeteaseSettings) -> anyhow::Result<Option<Self>> {
        if setting.enabled {
            util::ensure_file(&setting.cookie_path)?;

            let jar = PersistCookieStore::try_new(setting.cookie_path)?;
            return Ok(Some(Self {
                instance: setting.instance,
                client: reqwest::Client::builder()
                    .cookie_provider(Arc::new(jar))
                    .build()
                    .unwrap(),
            }));
        }

        Ok(None)
    }

    async fn cloud_search(&self, keyword: String, t: ScrapeType) -> anyhow::Result<NeteaseSearch> {
        let t_str = match t {
            // ScrapeType::All => "1018",
            // All has some bugs now
            ScrapeType::All | ScrapeType::Song => "1",
            ScrapeType::Album => "10",
            ScrapeType::Artist => "100",
            ScrapeType::Playlist => "1000",
        };

        self.client
            .get(format!("{}/search", self.instance))
            .query(&[
                ("keywords", keyword.as_str()),
                ("type", t_str),
                ("realIP", "116.25.146.177"),
            ])
            .send()
            .await?
            .json::<NeteaseResponseResult<NeteaseSearch>>()
            .await?
            .data()
    }

    async fn batch_songs(&self, ids: Vec<String>) -> anyhow::Result<Vec<NeteaseSong>> {
        Ok(self
            .client
            .get(format!("{}/song/detail", self.instance))
            .query(&[
                ("ids", ids.join(",")),
                ("realIP", "116.25.146.177".to_string()),
            ])
            .send()
            .await?
            .json::<NeteaseResponse<NeteaseSongDetail>>()
            .await?
            .data()?
            .songs)
    }
}

#[async_trait]
impl Scraper for NeteaseScraper {
    async fn suggest(&self, keyword: String) -> anyhow::Result<Vec<String>> {
        let data = self
            .client
            .get(format!("{}/search/suggest", self.instance))
            .query(&[("keywords", keyword.as_str()), ("realIP", "116.25.146.177")])
            .send()
            .await?
            .json::<NeteaseResponseResult<NeteaseSearchSuggest>>()
            .await?
            .data()?;

        Ok(data
            .artists
            .into_iter()
            .map(|i| i.name)
            .chain(data.songs.into_iter().map(|i| i.name))
            .collect())
    }

    async fn search(&self, keyword: String, t: ScrapeType) -> Vec<ScrapeItem> {
        info!("[Netease] search {} with type {:?}", keyword, t);
        match self.cloud_search(keyword, t).await {
            Err(e) => {
                error!("cloud search failed: {}", e);
                vec![]
            }
            Ok(res) => match res {
                NeteaseSearch::Song { songs } => songs
                    .into_iter()
                    .map(|s| ScrapeItem::Song(s.into()))
                    .collect(),
                NeteaseSearch::Playlist { playlists } => playlists
                    .into_iter()
                    .map(|p| ScrapeItem::Playlist(p.into()))
                    .collect(),
                NeteaseSearch::Artist { artists } => artists
                    .into_iter()
                    .map(|a| ScrapeItem::Artist(a.into()))
                    .collect(),
                NeteaseSearch::Album { albums } => albums
                    .into_iter()
                    .map(|a| ScrapeItem::Album(a.into()))
                    .collect(),
            },
        }
    }

    async fn collection_detail(&self, id: String) -> anyhow::Result<SongCollection> {
        let playlist = self
            .client
            .get(format!("{}/playlist/detail", self.instance))
            .query(&[("id", id.as_str()), ("realIP", "116.25.146.177")])
            .send()
            .await?
            .json::<NeteaseResponse<NeteasePlaylistDetailResp>>()
            .await?
            .data()?
            .playlist;

        let songs = self
            .batch_songs(
                playlist
                    .track_ids
                    .into_iter()
                    .map(|i| i.id.to_string())
                    .collect(),
            )
            .await?;

        Ok(SongCollection {
            id: playlist.basic_info.id.to_string(),
            name: playlist.basic_info.name,
            artists: vec![playlist.basic_info.creator.into()],
            cover: playlist.basic_info.cover_url.map(Into::into),
            description: playlist.basic_info.description,
            songs: songs.into_iter().map(Into::into).collect(),
        })
    }

    async fn stream(&self, id: String) -> anyhow::Result<Vec<Stream>> {
        let resp = self
            .client
            .get(format!("{}/song/download/url", self.instance))
            .query(&[("id", id.as_str()), ("realIP", "116.25.146.177")])
            .send()
            .await?
            .json::<NeteaseResponseResult<NeteaseSongDownload>>()
            .await?
            .data()?;

        match resp.url {
            Some(url) => Ok(vec![Stream {
                url,
                quality: format!("lossless({})", resp.bitrate),
            }]),
            None => bail!(r#"{{"message": "now download url present"}}"#),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::scraper::{ScrapeType, Scraper};

    use super::NeteaseScraper;

    fn cli() -> NeteaseScraper {
        NeteaseScraper::new(
            "https://netease-cloud-music-api-xylonx.vercel.app".into(),
            reqwest::Client::default(),
        )
    }

    #[tokio::test]
    async fn test_nsearch() {
        let cli = cli();
        let resp = cli
            .cloud_search("早稻叽".to_string(), ScrapeType::Playlist)
            .await;
        println!("{:?}", resp);
    }

    #[tokio::test]
    async fn test_suggest() {
        let cli = cli();
        let search = cli.suggest("早稻叽".to_string()).await.unwrap();
        println!("{:?}", search);
    }

    #[tokio::test]
    async fn test_search() {
        let cli = cli();
        let search = cli.search("早稻叽".to_string(), ScrapeType::All).await;
        println!("{:?}", search);
    }

    #[tokio::test]
    async fn test_playlist() {
        let cli = cli();
        let search = cli
            .collection_detail("4934616945".to_string())
            .await
            .unwrap();
        println!("{:?}", search);
    }

    #[tokio::test]
    async fn test_stream() {
        let cli = cli();
        let search = cli.stream("1866231828".to_string()).await.unwrap();
        println!("{:?}", search);
    }
}
