# Bragi

Bragi is the All-In-One audio player for all platforms.

Basically, it is a Client-Server Architecture.

## Bragi-Core

In server side, Bragi will scrape resources from multiple upstream service providers (e.g. YouTube, Spotify, NetEase Music), reformat them and expose HTTP Restful API for execute.

The main reason of such a design is the various authentication and authorization among multiple upstream service providers. For example, you need store cookies and refresh it periodically for YouTube, meanwhile, you also need to store authorization TOKEN and refresh it frequently for Spotify. It is hard to support so much auth methods for all of these sites in Bragi Client side.

> IMPORTANT NOTE: It is not designed for multiple users with one server but aimed to provide service for a single user with one server. Therefore, it is not proper for someone who want to use it as SaaS. 
>
> If You still want to share your server with multiple users, **PLEASE BE CAREFUL** since all of these users will use **YOUR** accounts instead of theirs.

For some high DRM service providers like Spotify, the tracks may be encrypted and hard to feed it directly. Therefore, it is essential to decipher it and provide stream in server side.

### as a library

If you want to use bragi-core as a library, you should reference how `main.rs` does.

Below is a simple example showing how to init scraper manager and load spotify scarper.

```rust
use bragi_core::scraper::{youtube::YouTubeScraper, ScraperManager};

async fn main() {
    let mut manager = ScraperManager::default();

    manager.add_scraper(YouTubeScraper::default()).await;

    manager.suggest(...);
    manager.search(...);
    manager.detail(...);
    manager.stream(...);
}
```