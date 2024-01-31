use anyhow::anyhow;

pub struct PersistCookieStore {
    filename: String,
    store: reqwest_cookie_store::CookieStoreRwLock,
}

impl PersistCookieStore {
    pub fn try_new(filename: String) -> anyhow::Result<Self> {
        let file = std::fs::File::open(&filename).map(std::io::BufReader::new)?;
        let cookie_store =
            reqwest_cookie_store::CookieStore::load_json(file).map_err(|e| anyhow!("{}", e))?;

        Ok(Self {
            filename,
            store: reqwest_cookie_store::CookieStoreRwLock::new(cookie_store),
        })
    }
}

impl reqwest::cookie::CookieStore for PersistCookieStore {
    fn set_cookies(
        &self,
        cookie_headers: &mut dyn Iterator<Item = &reqwest::header::HeaderValue>,
        url: &reqwest::Url,
    ) {
        self.store.set_cookies(cookie_headers, url);

        // Write store back to disk
        let mut writer = std::fs::File::create(&self.filename)
            .map(std::io::BufWriter::new)
            .unwrap();
        let store = self.store.read().unwrap();
        store.save_json(&mut writer).unwrap();
    }

    fn cookies(&self, url: &reqwest::Url) -> Option<reqwest::header::HeaderValue> {
        self.store.cookies(url)
    }
}
