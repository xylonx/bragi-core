use std::{
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
};

use anyhow::{bail, Context, Result};
use log::info;
use parking_lot::RwLock;
use reqwest::{
    cookie::{CookieStore, Jar},
    header::HeaderValue,
    Url,
};

pub enum PersistMode {
    Sync { cookie_path: PathBuf },
    Async { cookie_path: PathBuf },
}

pub struct PersistCookieStore<'a>(RwLock<InternalPersistCookieStore<'a>>);

impl<'a> PersistCookieStore<'a> {
    // setting persist to none means non-persist
    pub fn new(url: Url, persist: Option<PathBuf>) -> Result<Self> {
        let internal = InternalPersistCookieStore {
            jar: Jar::default(),
            url,
            cookie_path: None,
        };

        if let Some(cookie_path) = persist {
            let file = match cookie_path.exists() {
                false => {
                    info!("cookie file {:?} not exists. create it", cookie_path);
                    if let Some(p) = cookie_path.parent() {
                        if !p.exists() {
                            info!(
                                "cookie file parent dir {:?} not exists. create it",
                                cookie_path
                            );
                            fs::create_dir_all(p)
                                .with_context(|| "create cookie file parent dir failed")?;
                        }
                    }
                    fs::File::create(cookie_path).with_context(|| "create cookie file failed")?
                }
                true => fs::File::open(cookie_path).with_context(|| "open cookie file failed")?,
            };

            BufReader::new(file)
                .lines()
                .try_for_each(|c| match c {
                    Ok(cookie) => Ok(internal.jar.add_cookie_str(cookie.as_str(), &internal.url)),
                    Err(e) => bail!("read line from cookie failed: {}", e),
                })
                .with_context(|| "load cookie from file failed")?;
        }

        Ok(Self(RwLock::new(internal)))
    }
}

impl<'a> CookieStore for PersistCookieStore<'a> {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        self.0.write().jar.set_cookies(cookie_headers, url);
        // TODO(xylonx): persist cookies back to file
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        self.0.read().jar.cookies(url)
    }
}

// NOTE(xylonx): jar.cookies() will just give the while cookie string joined by "; ". consider to replace it
struct InternalPersistCookieStore<'a> {
    jar: Jar,
    url: Url,
    cookie_path: Option<&'a PathBuf>,
}
