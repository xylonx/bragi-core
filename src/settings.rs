//
// Created on Sun Mar 19 2023
//
// The MIT License (MIT)
// Copyright (c) 2023 xylonx
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

fn default_enabled() -> bool {
    false
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
pub struct ProviderConfig {
    pub spotify: Option<SpotifyConfig>,
    pub bilibili: Option<BilibiliConfig>,
    pub netease: Option<NeteaseConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct SpotifyConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct BilibiliConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub cookie_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct NeteaseConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub server: ServerConfig,
    pub provider: ProviderConfig,
}

impl Setting {
    pub fn check_validation(&self) -> Result<()> {
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
