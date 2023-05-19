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
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

fn default_enabled() -> bool {
    false
}

fn default_request_buffer_size() -> u32 {
    128
}

fn default_concurrency() -> u32 {
    8
}

fn default_request_limit() -> u32 {
    8
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub addr: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct LimitedRequestConfig {
    #[serde(default = "default_request_buffer_size")]
    pub request_buffer_size: u32,
    #[serde(default = "default_concurrency")]
    pub max_concurrency_number: u32,
    #[serde(default = "default_request_limit")]
    pub limit_request_per_seconds: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub spotify: Option<SpotifyConfig>,
    pub bilibili: Option<BilibiliConfig>,
    pub netease: Option<NeteaseConfig>,
    pub youtube: Option<YouTubeConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct SpotifyConfig {
    #[serde(flatten)]
    pub request_limit: LimitedRequestConfig,

    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub username: String,
    pub password: String,

    pub client_id: String,
    pub client_secret: String,

    pub cache_dir: String,
    pub static_dir: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct BilibiliConfig {
    #[serde(flatten)]
    pub request_limit: LimitedRequestConfig,

    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub cookie_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct NeteaseConfig {
    #[serde(flatten)]
    pub request_limit: LimitedRequestConfig,

    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub cookie_path: String,
    pub instance: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct YouTubeConfig {
    #[serde(flatten)]
    pub request_limit: LimitedRequestConfig,

    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub instance: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub server: ServerConfig,
    pub provider: ProviderConfig,
}

impl Setting {
    pub fn check_validation(&self) -> Result<()> {
        if self.provider.bilibili.is_none()
            && self.provider.spotify.is_none()
            && self.provider.netease.is_none()
            && self.provider.youtube.is_none()
        {
            bail!(
                "[Setting] at lease one provider should be enabled but all of them are disabled."
            );
        }
        Ok(())
    }

    fn from_str(s: &str) -> Result<Self> {
        let config: Setting = toml::from_str(s).with_context(|| "Failed to parse the config")?;
        Ok(config)
    }

    pub async fn from_file(path: &Path) -> Result<Setting> {
        let s: String = fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read the config file {:?}", path))?;
        Setting::from_str(&s).with_context(|| {
            "Configuration is invalid. Please refer to the configuration specification."
        })
    }
}
