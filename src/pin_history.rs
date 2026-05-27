use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use log::error;
use teloxide::types::MessageId;
use tokio::sync::Mutex;

pub struct PinHistory {
    entries: Mutex<VecDeque<MessageId>>,
    file_path: Option<PathBuf>,
    max: Option<usize>,
}

impl PinHistory {
    pub fn disabled() -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            file_path: None,
            max: None,
        }
    }

    pub fn load(storage_path: &Path, max: usize) -> io::Result<Self> {
        let file_path = storage_path.join("pinned_messages.json");

        let entries: VecDeque<MessageId> = match fs::read(&file_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => VecDeque::new(),
            Err(err) => return Err(err),
        };

        Ok(Self {
            entries: Mutex::new(entries),
            file_path: Some(file_path),
            max: Some(max),
        })
    }

    pub async fn push(&self, new_id: MessageId) -> Vec<MessageId> {
        let Some(max) = self.max else {
            return Vec::new();
        };

        let mut entries = self.entries.lock().await;
        entries.push_back(new_id);

        let mut evicted = Vec::new();

        while entries.len() > max {
            let oldest = entries
                .pop_front()
                .expect("entries non-empty while over capacity");
            evicted.push(oldest);
        }

        if let Some(file_path) = &self.file_path {
            let result = serde_json::to_vec(&*entries)
                .map_err(io::Error::other)
                .and_then(|bytes| fs::write(file_path, bytes));

            if let Err(err) = result {
                error!("failed to persist pin history: {err}");
            }
        }

        evicted
    }
}
