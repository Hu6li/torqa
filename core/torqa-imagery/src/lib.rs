//! Aerial imagery for Torqa's 3D world: orthophotos cut to the terrain chunks, downloaded once and
//! cached for offline rides (R3).
//!
//! Currently Switzerland only: SWISSIMAGE by swisstopo (open data, attribution "© swisstopo").

use std::path::PathBuf;
use std::time::Duration;

use futures::{StreamExt, stream};
use tracing::warn;

/// Requests at the same time; the WMS renders on demand and rate-limits.
const PARALLEL_DOWNLOADS: usize = 4;
/// Bumped when the provider or request changes, so stale cached images are not reused.
const CACHE_VERSION: &str = "swissimage-wms-v1";
const WMS: &str = "https://wms.geo.admin.ch/";

/// A latitude/longitude rectangle in degrees (WGS84).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// Southern edge.
    pub south: f64,
    /// Western edge.
    pub west: f64,
    /// Northern edge.
    pub north: f64,
    /// Eastern edge.
    pub east: f64,
}

impl Bounds {
    /// Whether SWISSIMAGE covers the rectangle (a box around Switzerland; areas outside the
    /// country come back blank, which the renderer treats as photo).
    #[must_use]
    pub fn in_switzerland(&self) -> bool {
        self.south > 45.8 && self.north < 47.81 && self.west > 5.95 && self.east < 10.5
    }
}

/// One orthophoto to fetch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Request {
    /// The area; the image is aligned with latitude/longitude, north up.
    pub bounds: Bounds,
    /// Edge length in pixels.
    pub size: u32,
}

/// Downloads and caches orthophotos.
pub struct Imagery {
    cache_dir: PathBuf,
    client: reqwest::Client,
    online: bool,
}

impl Imagery {
    /// Creates a downloader caching under `cache_dir`.
    ///
    /// # Panics
    /// If the HTTP client cannot be initialised (no TLS backend), which is a build error.
    #[must_use]
    pub fn new(cache_dir: PathBuf) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "Torqa/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/bossm8/torqa)"
            ))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("HTTP client with TLS");
        Self {
            cache_dir,
            client,
            online: true,
        }
    }

    /// Uses only cached images and never downloads.
    #[must_use]
    pub fn offline(mut self) -> Self {
        self.online = false;
        self
    }

    /// JPEG orthophotos for `requests`, in order; `None` where no imagery is available.
    /// `progress` is called with (done, total).
    pub async fn photos(
        &self,
        requests: &[Request],
        progress: &mut (dyn FnMut(usize, usize) + Send),
    ) -> Vec<Option<Vec<u8>>> {
        let total = requests.len();
        progress(0, total);
        let mut results: Vec<(usize, Option<Vec<u8>>)> = Vec::with_capacity(total);
        // Owned copies: borrowed items would tie the stream to `requests` in a way spawned
        // tasks cannot express.
        let owned: Vec<(usize, Request)> = requests.iter().copied().enumerate().collect();
        let mut downloads = stream::iter(owned)
            .map(|(index, request)| async move { (index, self.photo(request).await) })
            .buffer_unordered(PARALLEL_DOWNLOADS);
        while let Some(result) = downloads.next().await {
            results.push(result);
            progress(results.len(), total);
        }
        results.sort_by_key(|(index, _)| *index);
        results.into_iter().map(|(_, photo)| photo).collect()
    }

    async fn photo(&self, request: Request) -> Option<Vec<u8>> {
        if !request.bounds.in_switzerland() {
            return None;
        }
        let path = self.cache_dir.join(CACHE_VERSION).join(cache_name(request));
        if let Ok(bytes) = tokio::fs::read(&path).await {
            return Some(bytes);
        }
        if !self.online {
            return None;
        }
        let bytes = self.download(request).await?;
        let stored = async {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            // Write atomically, so an interrupted download never leaves a broken cache file.
            let partial = path.with_extension("part");
            tokio::fs::write(&partial, &bytes).await?;
            tokio::fs::rename(&partial, &path).await
        }
        .await;
        if let Err(error) = stored {
            warn!(%error, "cannot cache aerial image");
        }
        Some(bytes)
    }

    async fn download(&self, request: Request) -> Option<Vec<u8>> {
        let url = wms_url(request);
        for attempt in 1..=2 {
            match self.client.get(&url).send().await {
                Ok(response)
                    if response.status().is_success()
                        && response
                            .headers()
                            .get(reqwest::header::CONTENT_TYPE)
                            .is_some_and(|t| t.as_bytes().starts_with(b"image/jpeg")) =>
                {
                    match response.bytes().await {
                        Ok(bytes) => return Some(bytes.to_vec()),
                        Err(error) => warn!(%error, attempt, "aerial image download failed"),
                    }
                }
                Ok(response) => {
                    warn!(status = %response.status(), attempt, "aerial image refused");
                }
                Err(error) => warn!(%error, attempt, "aerial image download failed"),
            }
        }
        None
    }
}

/// A WMS 1.3.0 `GetMap` request; with EPSG:4326 the bounding box is latitude first.
fn wms_url(request: Request) -> String {
    let b = request.bounds;
    format!(
        "{WMS}?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=ch.swisstopo.swissimage\
         &STYLES=&CRS=EPSG:4326&BBOX={:.7},{:.7},{:.7},{:.7}&WIDTH={size}&HEIGHT={size}\
         &FORMAT=image/jpeg",
        b.south,
        b.west,
        b.north,
        b.east,
        size = request.size
    )
}

/// A stable file name for a request.
fn cache_name(request: Request) -> String {
    let b = request.bounds;
    format!(
        "{:.6}_{:.6}_{:.6}_{:.6}_{}.jpg",
        b.south, b.west, b.north, b.east, request.size
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WABERN: Bounds = Bounds {
        south: 46.9275,
        west: 7.448,
        north: 46.9318,
        east: 7.4543,
    };

    #[test]
    fn requests_the_area_latitude_first() {
        let url = wms_url(Request {
            bounds: WABERN,
            size: 512,
        });

        assert!(url.contains("CRS=EPSG:4326"));
        assert!(url.contains("BBOX=46.9275000,7.4480000,46.9318000,7.4543000"));
        assert!(url.contains("WIDTH=512&HEIGHT=512"));
    }

    #[test]
    fn only_switzerland_is_covered() {
        assert!(WABERN.in_switzerland());
        let paris = Bounds {
            south: 48.85,
            west: 2.34,
            north: 48.86,
            east: 2.35,
        };
        assert!(!paris.in_switzerland());
    }

    #[tokio::test]
    async fn offline_uses_the_cache_only() {
        let cache = std::env::temp_dir().join(format!("torqa-imagery-{}", std::process::id()));
        let request = Request {
            bounds: WABERN,
            size: 64,
        };
        let path = cache.join(CACHE_VERSION).join(cache_name(request));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"cached jpeg").unwrap();
        let imagery = Imagery::new(cache.clone()).offline();
        let uncached = Request {
            size: 128,
            ..request
        };

        let photos = imagery.photos(&[request, uncached], &mut |_, _| {}).await;

        assert_eq!(photos, [Some(b"cached jpeg".to_vec()), None]);
        std::fs::remove_dir_all(cache).unwrap();
    }

    /// Downloads a real image; run with `cargo test -p torqa-imagery -- --ignored`.
    #[tokio::test]
    #[ignore = "needs network"]
    async fn live_swissimage() {
        let cache = std::env::temp_dir().join(format!("torqa-imagery-live-{}", std::process::id()));
        let imagery = Imagery::new(cache);

        let photos = imagery
            .photos(
                &[Request {
                    bounds: WABERN,
                    size: 256,
                }],
                &mut |_, _| {},
            )
            .await;

        let jpeg = photos[0].as_ref().expect("an image");
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "JPEG start marker");
    }
}
