use anyhow::anyhow;
use html_escape::decode_html_entities;
use invidious::ClientAsyncTrait;

use crate::settings::YouTubeSettings;

use super::*;

fn thumbnails_to_cover(thumbnails: Vec<invidious::CommonThumbnail>) -> Option<String> {
    thumbnails
        .into_iter()
        .max_by(|x, y| x.width.cmp(&y.width))
        .map(|t| t.url)
}

fn images_to_cover(thumbnails: Vec<invidious::CommonImage>) -> Option<String> {
    thumbnails
        .into_iter()
        .max_by(|x, y| x.width.cmp(&y.width))
        .map(|t| t.url)
}

fn artists(id: String, name: String, avatar: Option<String>) -> Vec<Artist> {
    vec![Artist {
        id,
        name,
        description: None,
        avatar,
    }]
}

#[derive(Default)]
pub struct YouTubeScraper {
    client: invidious::ClientAsync,
}

impl YouTubeScraper {
    pub fn new(client: invidious::ClientAsync) -> Self {
        Self { client }
    }

    pub fn try_from_setting(setting: YouTubeSettings) -> anyhow::Result<Option<Self>> {
        if setting.enabled {
            return Ok(Some(Self {
                client: invidious::ClientAsync::new(
                    setting.instance,
                    invidious::MethodAsync::Reqwest,
                ),
            }));
        }

        Ok(None)
    }
}

impl From<invidious::CommonVideo> for Song {
    fn from(val: invidious::CommonVideo) -> Self {
        Song {
            id: val.id,
            name: decode_html_entities(&val.title).to_string(),
            artists: artists(val.author_id, val.author, None),
            cover: thumbnails_to_cover(val.thumbnails),
            duration: Some(val.length),
        }
    }
}

impl From<invidious::hidden::PlaylistItem> for Song {
    fn from(val: invidious::hidden::PlaylistItem) -> Self {
        Self {
            id: val.id,
            name: decode_html_entities(&val.title).to_string(),
            artists: artists(val.author_id, val.author, None),
            cover: thumbnails_to_cover(val.thumbnails),
            duration: Some(val.length),
        }
    }
}

impl From<invidious::CommonPlaylist> for SongCollection {
    fn from(val: invidious::CommonPlaylist) -> Self {
        let artists = artists(val.author_id, val.author, None);
        Self {
            id: val.id,
            name: decode_html_entities(&val.title).to_string(),
            cover: Some(val.thumbnail),
            description: None,
            songs: val
                .videos
                .into_iter()
                .map(|v| Song {
                    id: v.id,
                    name: v.title,
                    artists: artists.clone(),
                    cover: thumbnails_to_cover(v.thumbnails),
                    duration: Some(v.length),
                })
                .collect(),
            artists,
        }
    }
}

impl From<invidious::universal::Playlist> for SongCollection {
    fn from(val: invidious::universal::Playlist) -> Self {
        Self {
            id: val.id,
            name: decode_html_entities(&val.title).to_string(),
            artists: artists(
                val.author_id,
                val.author,
                images_to_cover(val.author_thumbnails),
            ),
            cover: Some(val.thumbnail),
            description: Some(val.description),
            songs: val.videos.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<invidious::CommonChannel> for Artist {
    fn from(val: invidious::CommonChannel) -> Self {
        Self {
            id: val.id,
            name: decode_html_entities(&val.name).to_string(),
            description: Some(val.description),
            avatar: images_to_cover(val.thumbnails),
        }
    }
}

impl From<invidious::hidden::SearchItem> for ScrapeItem {
    fn from(value: invidious::hidden::SearchItem) -> Self {
        match value {
            invidious::hidden::SearchItem::Video(v) => Self::Song(v.into()),
            invidious::hidden::SearchItem::Playlist(p) => Self::Playlist(p.into()),
            invidious::hidden::SearchItem::Channel(c) => Self::Artist(c.into()),
        }
    }
}

impl From<invidious::hidden::AdaptiveFormat> for Stream {
    fn from(val: invidious::hidden::AdaptiveFormat) -> Self {
        Self {
            quality: format!("{}({})", val.audio_quality, val.bitrate),
            url: val.url,
        }
    }
}

#[async_trait]
impl Scraper for YouTubeScraper {
    async fn suggest(&self, keyword: String) -> anyhow::Result<Vec<String>> {
        self.client
            .search_suggestions(Some(&format!("q={keyword}")))
            .await
            .map(|v| {
                v.suggestions
                    .into_iter()
                    .map(|s| decode_html_entities(&s).to_string())
                    .collect()
            })
            .map_err(|e| anyhow!("{}", e))
    }

    async fn search(&self, keyword: String, t: ScrapeType) -> Vec<ScrapeItem> {
        let query_type = match t {
            // Album is not supported by YouTube
            ScrapeType::Album => return vec![],
            ScrapeType::All => "all",
            ScrapeType::Song => "video",
            ScrapeType::Artist => "channel",
            ScrapeType::Playlist => "playlist",
        };

        self.client
            .search(Some(&format!("q={keyword}&type={query_type}")))
            .await
            .map(|v| v.items)
            .into_iter()
            .flatten()
            .map(Into::<ScrapeItem>::into)
            .collect()
    }

    async fn collection_detail(&self, id: String) -> anyhow::Result<SongCollection> {
        self.client
            .playlist(&id, None)
            .await
            .map(Into::into)
            .map_err(|e| anyhow!("{}", e))
    }

    async fn stream(&self, id: String) -> anyhow::Result<Vec<Stream>> {
        self.client
            .video(&id, None)
            .await
            .map(|v| {
                v.adaptive_formats
                    .into_iter()
                    .filter(|i| !i.audio_quality.is_empty())
                    .map(Into::into)
                    .collect()
            })
            .map_err(|e| anyhow!("{}", e))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_suggest() {
        let scraper = YouTubeScraper::default();
        let suggestions = scraper.suggest("早稻叽".into()).await.unwrap();
        println!("{:?}", suggestions);
    }

    #[tokio::test]
    async fn test_search() {
        let scraper = YouTubeScraper::default();
        scraper
            .search("早稻叽".into(), ScrapeType::All)
            .await
            .into_iter()
            .for_each(|i| println!("Search Item: {:?}", i));
    }

    #[tokio::test]
    async fn test_collection_detail() {
        let scraper = YouTubeScraper::default();
        let details = scraper
            .collection_detail("PLtrsXT0Azk1lh-F9RxHOlPBhpUcn-x96X".into())
            .await
            .unwrap();
        println!("{:?}", details);
    }

    #[tokio::test]
    async fn test_stream() {
        let scraper = YouTubeScraper::default();
        let streams = scraper.stream("K_x2r8vJxZ4".into()).await.unwrap();
        println!("{:?}", streams);
    }
}
