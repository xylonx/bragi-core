use std::{
    io::Write,
    ops::Sub,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::bail;
use async_trait::async_trait;
use chrono::Timelike;
use lazy_static::lazy_static;
use parking_lot::RwLock;
use serde::{de::IgnoredAny, Deserialize, Deserializer};
use tracing::{error, info};

use crate::{
    settings::BiliSettings,
    util::{self, cookie::PersistCookieStore},
};

use super::{Artist, ScrapeItem, ScrapeType, Scraper, Song, SongCollection, Stream};

const DEFAULT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/98.0.4758.102 Safari/537.36 Edg/98.0.1108.62";

const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

lazy_static! {
    static ref TITLE_REPLACER: regex::Regex =
        regex::RegexBuilder::new(r#"(<([^>]+)>)"#).build().unwrap();
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

/// origin cover url may be like: //i0.hdslb.com/bfs/archive/23c4be1b7f62848b95e9b4b2e1d6ce2e50bedf17.jpg
/// therefore, add 'https:' scheme
/// Or if the url star with http, replace it with https
fn deserialize_cover_url<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    if s.starts_with("//") {
        return Result::Ok(format!("https:{}", s));
    }
    if s.starts_with("http:") {
        return Result::Ok(s.replacen("http:", "https:", 1));
    }
    Result::Ok(s)
}

fn deserialize_audio_quality<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s: i64 = Deserialize::deserialize(deserializer)?;
    Result::Ok(
        match s {
            30216 => "64k",
            30232 => "132k",
            30280 => "192k",
            30250 => "Dolby",
            30251 => "Hi-Res lossless",
            _ => "unknown",
        }
        .to_string(),
    )
}

#[derive(Debug, Deserialize)]
struct BiliResponse<T> {
    code: i32,
    message: Option<String>,
    #[serde(alias = "result")]
    data: T,
}

impl<T> BiliResponse<T> {
    fn data(self) -> anyhow::Result<T> {
        if self.code == 0 {
            return Ok(self.data);
        }
        bail!(
            "[Bilibili] call request failed: status code: {} resp message: {}",
            self.code,
            self.message.unwrap_or_default()
        );
    }
}

#[derive(Deserialize)]
struct NavData {
    wbi_img: WbiImg,
}

#[derive(Deserialize)]
struct WbiImg {
    img_url: String,
    sub_url: String,
}

#[derive(Debug, Deserialize)]
struct BiliSuggest {
    tag: Vec<Suggestion>,
}

#[derive(Debug, Deserialize)]
struct Suggestion {
    value: String,
}

#[derive(Debug, Deserialize)]
struct ComprehensiveSearch {
    result: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct TypedSearch {
    result: Vec<TypedSearchItem>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "result_type", content = "data")]
#[serde(rename_all = "lowercase")]
enum SearchItem {
    Video(Vec<BiliVideo>),
    BiliUser(Vec<BiliUser>),
    #[serde(untagged)]
    Others(IgnoredAny),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
enum TypedSearchItem {
    Video(BiliVideo),
    BiliUser(BiliUser),
    #[serde(untagged)]
    Others(IgnoredAny),
}

#[derive(Debug, Deserialize)]
struct BiliUser {
    #[serde(rename = "mid")]
    author_id: u64,
    #[serde(deserialize_with = "deserialize_cover_url")]
    upic: String,
    #[serde(rename = "uname")]
    name: String,
    #[serde(rename = "usign")]
    description: String,
}

impl From<BiliUser> for Artist {
    fn from(val: BiliUser) -> Self {
        Self {
            id: val.author_id.to_string(),
            name: val.name,
            description: Some(val.description),
            avatar: Some(val.upic),
        }
    }
}

#[derive(Debug, Deserialize)]
struct BiliVideo {
    #[serde(rename = "bvid")]
    id: String,
    author: String,
    #[serde(rename = "mid")]
    author_id: u64,
    #[serde(deserialize_with = "deserialize_title")]
    title: String,
    #[serde(deserialize_with = "deserialize_cover_url")]
    pic: String,
    description: String,
}

/// NOTE(xylonx): it is not possible to distinguish whether a video is a single page video or a multi-page video
/// Therefore, treat all videos as multi-page videos
impl From<BiliVideo> for SongCollection {
    fn from(val: BiliVideo) -> Self {
        Self {
            id: val.id,
            name: val.title,
            artists: vec![Artist {
                id: val.author_id.to_string(),
                name: val.author,
                description: None,
                avatar: None,
            }],
            cover: Some(val.pic),
            description: Some(val.description),
            songs: vec![],
        }
    }
}

#[derive(Debug, Deserialize)]
struct BiliVideoDetail {
    #[serde(rename = "bvid")]
    id: String,
    #[serde(deserialize_with = "deserialize_cover_url")]
    pic: String,
    title: String,
    desc: String,
    pages: Vec<BiliPagedVideo>,
    owner: BiliOwner,
}

#[derive(Debug, Deserialize)]
struct BiliPagedVideo {
    cid: i64,
    #[serde(rename = "part")]
    name: String,
    duration: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct BiliOwner {
    mid: u64,
    name: String,
    #[serde(deserialize_with = "deserialize_cover_url")]
    face: String,
}

impl From<BiliOwner> for Artist {
    fn from(val: BiliOwner) -> Self {
        Self {
            id: val.mid.to_string(),
            name: val.name,
            description: None,
            avatar: Some(val.face),
        }
    }
}

impl From<BiliVideoDetail> for SongCollection {
    fn from(val: BiliVideoDetail) -> Self {
        Self {
            songs: val
                .pages
                .into_iter()
                .map(|i| Song {
                    id: format!("{}::{}", val.id, i.cid),
                    name: i.name,
                    artists: vec![val.owner.clone().into()],
                    cover: Some(val.pic.clone()),
                    duration: Some(i.duration),
                })
                .collect(),
            id: val.id,
            name: val.title,
            artists: vec![val.owner.into()],
            cover: Some(val.pic),
            description: Some(val.desc),
        }
    }
}

#[derive(Debug, Deserialize)]
struct BiliStream {
    dash: BiliDash,
}

#[derive(Debug, Deserialize)]
struct BiliDash {
    audio: Vec<BiliDashAudio>,
    dolby: BiliDashDolby,
    flac: Option<BiliDashLossless>,
}

#[derive(Debug, Deserialize)]
struct BiliDashAudio {
    #[serde(rename = "id", deserialize_with = "deserialize_audio_quality")]
    quality: String,
    base_url: String,
}

impl From<BiliDashAudio> for Vec<Stream> {
    fn from(val: BiliDashAudio) -> Self {
        vec![Stream {
            quality: val.quality.clone(),
            url: val.base_url,
        }]
        // .into_iter()
        // // .chain(val.backup_url.into_iter().map(|s| Stream {
        // //     quality: format!("{}(backup)", val.quality),
        // //     url: s,
        // // }))
        // .collect()
    }
}

#[derive(Debug, Deserialize)]
struct BiliDashDolby {
    #[serde(default)]
    audio: Option<Vec<BiliDashAudio>>,
}

#[derive(Debug, Deserialize)]
struct BiliDashLossless {
    #[serde(default)]
    audio: Vec<BiliDashAudio>,
}

pub type WbiCacheData = ((String, String), chrono::DateTime<chrono::FixedOffset>);

#[derive(Debug)]
pub struct BiliScraper {
    client: reqwest::Client,
    enable_dolby: bool,

    wbi_cache: Arc<RwLock<Option<WbiCacheData>>>,
    wbi_cache_file: String,
}

impl BiliScraper {
    pub fn try_from_setting(setting: BiliSettings) -> anyhow::Result<Option<Self>> {
        if setting.enabled {
            util::ensure_file(&setting.cookie_path)?;
            util::ensure_file(&setting.wbi_path)?;

            let jar = Arc::new(PersistCookieStore::try_new(setting.cookie_path)?);
            let wbi_cache_file =
                std::fs::File::open(&setting.wbi_path).map(std::io::BufReader::new)?;

            return Ok(Some(Self {
                client: reqwest::Client::builder()
                    .cookie_provider(jar)
                    .user_agent(DEFAULT_UA)
                    .build()
                    .unwrap(),
                enable_dolby: setting.enable_dolby,
                wbi_cache_file: setting.wbi_path,
                wbi_cache: Arc::new(RwLock::new(
                    serde_json::from_reader(wbi_cache_file).unwrap_or_default(),
                )),
            }));
        }

        Ok(None)
    }
    // 对 imgKey 和 subKey 进行字符顺序打乱编码
}

impl BiliScraper {
    pub async fn get_wbi_keys(&self) -> anyhow::Result<(String, String)> {
        let china_tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let china_time = chrono::Utc::now().with_timezone(&china_tz);

        {
            let cache = self.wbi_cache.read();
            if let Some((wbi, time)) = &*cache {
                // wbi cache key only available within the same day
                if china_time.sub(time).num_days() < 1 && china_time.hour() >= time.hour() {
                    return Ok(wbi.clone());
                }
            }
        }

        let wbi = self.req_wbi_keys().await?;

        let mut writer =
            std::fs::File::create(&self.wbi_cache_file).map(std::io::BufWriter::new)?;
        let new_cache = Some((wbi.clone(), china_time));
        writer.write_all(serde_json::to_string(&new_cache)?.as_bytes())?;

        {
            let mut cache = self.wbi_cache.write();
            *cache = new_cache;
        }

        Ok(wbi)
    }

    async fn req_wbi_keys(&self) -> anyhow::Result<(String, String)> {
        let wbi = self
            .client
            .get("https://api.bilibili.com/x/web-interface/nav")
            .send()
            .await?
            .json::<BiliResponse<NavData>>()
            .await?
            .data;

        Ok((wbi.wbi_img.img_url, wbi.wbi_img.sub_url))
    }

    // 对 imgKey 和 subKey 进行字符顺序打乱编码
    fn get_mixin_key(&self, orig: &[u8]) -> String {
        MIXIN_KEY_ENC_TAB
            .iter()
            .map(|&i| orig[i] as char)
            .collect::<String>()
    }

    fn get_url_encoded(&self, s: &str) -> String {
        s.chars()
            .filter_map(|c| match c.is_ascii_alphanumeric() || "-_.~".contains(c) {
                true => Some(c.to_string()),
                false => {
                    // 过滤 value 中的 "!'()*" 字符
                    if "!'()*".contains(c) {
                        return None;
                    }
                    let encoded = c
                        .encode_utf8(&mut [0; 4])
                        .bytes()
                        .fold("".to_string(), |acc, b| acc + &format!("%{:02X}", b));
                    Some(encoded)
                }
            })
            .collect::<String>()
    }

    pub fn encode_wbi(
        &self,
        mut params: Vec<(&str, String)>,
        img_key: String,
        sub_key: String,
    ) -> String {
        let mixin_key = self.get_mixin_key((img_key + &sub_key).as_bytes());
        let cur_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(t) => t.as_secs(),
            Err(_) => panic!("SystemTime before UNIX EPOCH!"),
        };

        let wts = cur_time.to_string();

        // 添加当前时间戳
        params.push(("wts", wts));
        // 重新排序
        params.sort_by(|a, b| a.0.cmp(b.0));
        let query = params.iter().fold(String::from(""), |acc, (k, v)| {
            acc + format!("{}={}&", self.get_url_encoded(k), self.get_url_encoded(v)).as_str()
        });

        let web_sign = format!("{:?}", md5::compute(query.clone() + &mixin_key));

        query + &format!("w_rid={}", web_sign)
    }
}

impl BiliScraper {
    fn handle_search_item(&self, item: SearchItem) -> Vec<ScrapeItem> {
        match item {
            SearchItem::Video(v) => v
                .into_iter()
                .map(|i| ScrapeItem::Playlist(i.into()))
                .collect(),
            SearchItem::BiliUser(u) => u
                .into_iter()
                .map(|i| ScrapeItem::Artist(i.into()))
                .collect(),
            _ => vec![],
        }
    }

    fn handle_typed_search_item(&self, item: TypedSearchItem) -> Option<ScrapeItem> {
        match item {
            TypedSearchItem::Video(v) => Some(ScrapeItem::Playlist(v.into())),
            TypedSearchItem::BiliUser(u) => Some(ScrapeItem::Artist(u.into())),
            _ => None,
        }
    }

    async fn bili_comprehensive_search(&self, keyword: String) -> anyhow::Result<Vec<ScrapeItem>> {
        let params = vec![("keyword", keyword)];
        info!("search param: {:?}", params);

        let (img_key, sub_key) = self.get_wbi_keys().await?;
        let query = self.encode_wbi(params, img_key, sub_key);
        info!("search query with wbi encoding: {}", query);

        Ok(self
            .client
            .get(format!(
                "https://api.bilibili.com/x/web-interface/wbi/search/all/v2?{}",
                query
            ))
            .send()
            .await?
            .json::<BiliResponse<ComprehensiveSearch>>()
            .await?
            .data()?
            .result
            .into_iter()
            .flat_map(|i| self.handle_search_item(i))
            .collect())
    }

    async fn bili_type_search(
        &self,
        keyword: String,
        search_type: String,
    ) -> anyhow::Result<Vec<ScrapeItem>> {
        let params = vec![("search_type", search_type), ("keyword", keyword)];
        info!("type search param: {:?}", params);

        let (img_key, sub_key) = self.get_wbi_keys().await?;
        let query = self.encode_wbi(params, img_key, sub_key);
        info!("type search query with wbi encoding: {}", query);

        Ok(self
            .client
            .get(format!(
                "https://api.bilibili.com/x/web-interface/wbi/search/type?{}",
                query
            ))
            .send()
            .await?
            .json::<BiliResponse<TypedSearch>>()
            .await?
            .data()?
            .result
            .into_iter()
            .filter_map(|i| self.handle_typed_search_item(i))
            .collect())
    }
}

#[async_trait]
impl Scraper for BiliScraper {
    async fn suggest(&self, keyword: String) -> anyhow::Result<Vec<String>> {
        Ok(self
            .client
            .get(format!(
                "https://s.search.bilibili.com/main/suggest?term={}",
                keyword,
            ))
            .send()
            .await?
            .json::<BiliResponse<BiliSuggest>>()
            .await?
            .data()?
            .tag
            .into_iter()
            .map(|i| i.value)
            .collect())
    }

    async fn search(&self, keyword: String, t: ScrapeType) -> Vec<ScrapeItem> {
        let items = match t {
            ScrapeType::All => self.bili_comprehensive_search(keyword).await,
            ScrapeType::Playlist => self.bili_type_search(keyword, "video".to_string()).await,
            ScrapeType::Artist => {
                self.bili_type_search(keyword, "bili_user".to_string())
                    .await
            }
            ScrapeType::Song => return vec![],
            ScrapeType::Album => return vec![],
        };

        match items {
            Ok(i) => i,
            Err(e) => {
                error!("comprehensive search failed: {}", e);
                println!("comprehensive search failed: {}", e);
                vec![]
            }
        }
    }

    async fn collection_detail(&self, id: String) -> anyhow::Result<SongCollection> {
        Ok(self
            .client
            .get("https://api.bilibili.com/x/web-interface/view")
            .query(&[("bvid", &id)])
            .send()
            .await?
            .json::<BiliResponse<BiliVideoDetail>>()
            .await?
            .data()?
            .into())
    }

    async fn stream(&self, id: String) -> anyhow::Result<Vec<Stream>> {
        let ids = id.split("::").collect::<Vec<_>>();
        if ids.len() != 2 {
            bail!("incorrect id: should be ${{bvid}}::${{cid}} but get {}", id);
        }

        // 16: DASH. 256: Dolby audio
        let fn_val = match self.enable_dolby {
            true => 16 | 256,
            false => 16,
        };

        let params = vec![
            ("bvid", ids[0].to_string()),
            ("cid", ids[1].to_string()),
            ("fnval", fn_val.to_string()),
        ];
        info!("stream param: {:?}", params);

        let (img_key, sub_key) = self.get_wbi_keys().await?;
        let query = self.encode_wbi(params, img_key, sub_key);
        info!("stream query with wbi encoding: {}", query);

        let dash = self
            .client
            .get(format!(
                "https://api.bilibili.com/x/player/wbi/playurl?{}",
                query
            ))
            .send()
            .await?
            .json::<BiliResponse<BiliStream>>()
            .await?
            .data()?
            .dash;

        let mut streams = vec![];
        if let Some(audio) = dash.dolby.audio {
            streams.extend(audio.into_iter().flat_map(Into::<Vec<Stream>>::into));
        }

        if let Some(flac) = dash.flac {
            streams.extend(flac.audio.into_iter().flat_map(Into::<Vec<Stream>>::into));
        }

        streams.extend(dash.audio.into_iter().flat_map(Into::<Vec<Stream>>::into));

        Ok(streams)
    }
}

#[cfg(test)]
mod test {
    use tracing::level_filters::LevelFilter;

    use crate::{
        scraper::{ScrapeType, Scraper},
        settings::BiliSettings,
    };

    use super::BiliScraper;

    fn cli() -> BiliScraper {
        tracing_subscriber::fmt::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::builder()
                    .with_default_directive(LevelFilter::TRACE.into())
                    .from_env_lossy(),
            )
            .init();

        BiliScraper::try_from_setting(BiliSettings {
            enabled: true,
            cookie_path: ".cookie/bili.json".into(),
            wbi_path: ".cookie/wbi.json".into(),
            enable_dolby: false,
        })
        .unwrap()
        .unwrap()
    }

    #[tokio::test]
    async fn test_suggest() {
        let cli = cli();

        let resp = cli.suggest("早稻叽".into()).await;
        println!("{:?}", resp);
    }

    #[tokio::test]
    async fn test_search_mix() {
        let cli = cli();

        let resp = cli.search("早稻叽".into(), ScrapeType::All).await;
        println!("{:?}", resp);
    }

    #[tokio::test]
    async fn test_search_playlist() {
        let cli = cli();

        let resp = cli.search("早稻叽".into(), ScrapeType::Playlist).await;
        println!("{:?}", resp);
    }

    #[tokio::test]
    async fn test_search_user() {
        let cli = cli();

        let resp = cli.search("早稻叽".into(), ScrapeType::Artist).await;
        println!("{:?}", resp);
    }

    #[tokio::test]
    async fn test_playlist_detail() {
        let cli = cli();

        let resp = cli
            .collection_detail("BV1dZ4y1g7ag".to_string())
            .await
            .unwrap();
        println!("{:?}", resp);
    }

    #[tokio::test]
    async fn test_stream() {
        let cli = cli();

        let resp = cli
            .stream("BV1dZ4y1g7ag::266767355".to_string())
            .await
            .unwrap();
        println!("{:?}", resp);
    }
}
