use log::{error, info};
use reqwest::StatusCode;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use teloxide::types::UserId;
use tokio::sync::RwLock;
use tokio::time::Instant;

#[derive(Debug, Clone)]
struct CachedResponse {
    ids: HashSet<UserId>,
    etag: Option<String>,
    last_updated: Instant,
}

#[derive(Debug, Clone)]
pub struct Countersign {
    state: Arc<RwLock<Option<CachedResponse>>>,
    client: reqwest::Client,
}

impl Countersign {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(None)),
            client: reqwest::Client::new(),
        }
    }

    pub async fn is_known_scammer(&self, user_id: UserId) -> bool {
        let stale_after = Duration::from_secs(15 * 60);

        let (cached_result, etag) = {
            let state = self.state.read().await;

            match state.as_ref() {
                Some(state) => {
                    let result = state.ids.contains(&user_id);

                    if state.last_updated.elapsed() < stale_after {
                        return result;
                    }

                    (result, state.etag.clone())
                }
                None => (false, None),
            }
        };

        let state = self.state.clone();

        match Self::fetch(&self.client, etag).await {
            Ok(Some(updated)) => {
                info!("countersign list updated");
                let mut state = state.write().await;
                let result = updated.ids.contains(&user_id);
                *state = Some(updated);
                result
            }
            Ok(None) => {
                info!("countersign list not modified");
                cached_result
            }
            Err(err) => {
                error!("failed to fetch countersign list: {err}");
                cached_result
            }
        }
    }

    async fn fetch(
        client: &reqwest::Client,
        etag: Option<String>,
    ) -> Result<Option<CachedResponse>, reqwest::Error> {
        let mut req = client.get("https://countersign.chat/api/scammer_ids.json");

        if let Some(tag) = etag {
            req = req.header("If-None-Match", tag);
        }

        let resp = req.send().await?;

        if resp.status() == StatusCode::NOT_MODIFIED {
            return Ok(None);
        }

        let etag = resp
            .headers()
            .get("ETag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let json = resp.json::<Vec<String>>().await?;

        let parsed: Result<HashSet<UserId>, _> = json
            .into_iter()
            .map(|s| s.parse::<u64>().map(UserId))
            .collect();

        let ids = match parsed {
            Ok(ids) => ids,
            Err(err) => {
                error!("failed to parse countersign list: {err}");
                return Ok(None);
            }
        };

        Ok(Some(CachedResponse {
            ids,
            etag,
            last_updated: Instant::now(),
        }))
    }
}
