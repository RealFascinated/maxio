use std::collections::BTreeMap;
use std::sync::Arc;

use crate::cache::MetricsLruCache;
use crate::metrics::{MetricsRegistry, cache_name};
use crate::storage::{MultipartUploadMeta, PartMeta};

#[derive(Debug, Clone)]
struct MultipartSession {
    meta: MultipartUploadMeta,
    parts: BTreeMap<u32, PartMeta>,
}

/// In-memory multipart upload sessions (upload meta + uploaded parts).
pub struct MultipartCache {
    sessions: MetricsLruCache<String, MultipartSession>,
}

impl MultipartCache {
    pub fn new(metrics: Option<Arc<MetricsRegistry>>, max_entries: usize) -> Self {
        Self {
            sessions: MetricsLruCache::new(metrics, cache_name::MULTIPART_SESSION, max_entries),
        }
    }

    pub fn get_upload(&self, upload_id: &str) -> Option<MultipartUploadMeta> {
        self.sessions
            .get(upload_id)
            .map(|session| session.meta.clone())
    }

    pub fn record_upload_miss(&self) {
        self.sessions.record_miss();
    }

    pub fn insert_upload(&self, meta: MultipartUploadMeta) {
        let upload_id = meta.upload_id.clone();
        self.sessions.insert(
            upload_id,
            MultipartSession {
                meta,
                parts: BTreeMap::new(),
            },
        );
    }

    pub fn upsert_part(&self, upload_id: &str, part: PartMeta) {
        self.sessions.get_mut(upload_id, |session| {
            session.parts.insert(part.part_number, part);
        });
    }

    pub fn list_parts(&self, upload_id: &str) -> Option<Vec<PartMeta>> {
        self.sessions.get(upload_id).and_then(|session| {
            if session.parts.is_empty() {
                None
            } else {
                Some(session.parts.values().cloned().collect())
            }
        })
    }

    pub fn record_parts_miss(&self) {
        self.sessions.record_miss();
    }

    pub fn install_session(&self, meta: MultipartUploadMeta, parts: Vec<PartMeta>) {
        let upload_id = meta.upload_id.clone();
        let mut part_map = BTreeMap::new();
        for part in parts {
            part_map.insert(part.part_number, part);
        }
        self.sessions.insert(
            upload_id,
            MultipartSession {
                meta,
                parts: part_map,
            },
        );
    }

    pub fn remove(&self, upload_id: &str) {
        self.sessions.remove(upload_id);
    }

    pub fn remove_many(&self, upload_ids: &[String]) {
        for upload_id in upload_ids {
            self.sessions.remove(upload_id);
        }
    }
}
