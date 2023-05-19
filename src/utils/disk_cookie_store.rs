use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use log::warn;
use reqwest::{
    cookie::{CookieStore, Jar},
    header::HeaderValue,
    Url,
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};

pub struct AsyncPersistCookieStore(Arc<InternalPersistCookieStore>);

impl AsyncPersistCookieStore {
    // setting persist to none means non-persist
    pub async fn new(url: Url, cookie_path: PathBuf) -> Result<Self> {
        let internal = Arc::new(InternalPersistCookieStore {
            jar: Jar::default(),
            url,
            cookie_path,
        });

        internal.load().await?;

        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        tokio::spawn({
            let internal = internal.clone();
            async move {
                loop {
                    interval.tick().await;
                    if let Err(e) = internal.clone().write_back().await {
                        warn!(
                            "[AsyncPersistCookieStore] failed to write cookie back to file: {}",
                            e
                        );
                    }
                }
            }
        });

        Ok(Self(internal))
    }
}

impl CookieStore for AsyncPersistCookieStore {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        self.0.jar.set_cookies(cookie_headers, url);
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        self.0.jar.cookies(url)
    }
}

// NOTE(xylonx): jar.cookies() will just give the while cookie string joined by "; ". consider to replace it
struct InternalPersistCookieStore {
    jar: Jar,
    url: Url,
    cookie_path: PathBuf,
}

impl InternalPersistCookieStore {
    pub async fn load(&self) -> Result<()> {
        let mut f = File::open(self.cookie_path.as_path())
            .await
            .with_context(|| format!("open cookie file {:?} failed", self.cookie_path))?;
        let mut buf = String::new();
        f.read_to_string(&mut buf).await?;
        buf.split("; ").for_each(|c| {
            self.jar.add_cookie_str(c, &self.url);
        });
        Ok(())
    }

    pub async fn write_back(&self) -> Result<()> {
        let mut file = File::create(self.cookie_path.as_path()).await?;
        file.write_all(
            self.jar
                .cookies(&self.url)
                .map_or("".as_bytes().to_vec(), |c| c.as_bytes().to_vec())
                .as_slice(),
        )
        .await?;
        Ok(())
    }
}
