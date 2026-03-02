use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::RwLock;

use style::context::QuirksMode;
use style::media_queries::MediaList;
use style::servo_arc::Arc;
use style::shared_lock::SharedRwLock;
use style::stylesheets::{AllowImportRules, Origin, Stylesheet, StylesheetContents, UrlExtraData};

pub struct StylesheetCache {
    cache: RwLock<HashMap<String, Arc<StylesheetContents>>>,
    lock: SharedRwLock,
}

impl StylesheetCache {
    pub fn new(lock: SharedRwLock) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            lock,
        }
    }

    pub fn get_or_parse(&self, css: &str) -> Arc<Stylesheet> {
        self.load_cached(css, css)
    }

    pub fn load_cached(&self, key: &str, css: &str) -> Arc<Stylesheet> {
        // 1. Try Read Lock
        {
            let cache = self.cache.read().unwrap();
            if let Some(existing) = cache.get(key) {
                let sheet = Stylesheet {
                    contents: self.lock.wrap(existing.clone()),
                    shared_lock: self.lock.clone(),
                    media: Arc::new(self.lock.wrap(MediaList::empty())),
                    disabled: AtomicBool::new(false),
                };
                return Arc::new(sheet);
            }
        }

        // 2. Entry missing, take Write Lock
        let mut cache = self.cache.write().unwrap();

        let contents = if let Some(existing) = cache.get(key) {
            existing.clone()
        } else {
            let url_data = UrlExtraData::from(url::Url::parse("about:blank").unwrap());

            let new_contents = StylesheetContents::from_str(
                css,
                url_data,
                Origin::Author,
                &self.lock,
                None, // loader
                None, // error_reporter
                QuirksMode::NoQuirks,
                AllowImportRules::Yes,
                None, // sanitization_data
            );
            cache.insert(key.to_string(), new_contents.clone());
            new_contents
        };

        let sheet = Stylesheet {
            contents: self.lock.wrap(contents),
            shared_lock: self.lock.clone(),
            media: Arc::new(self.lock.wrap(MediaList::empty())),
            disabled: AtomicBool::new(false),
        };

        Arc::new(sheet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use style::servo_arc::Arc;

    #[test]
    fn test_cache_deduplication() {
        let lock = SharedRwLock::new();
        let cache = StylesheetCache::new(lock);

        let css = "div { color: red; }";
        let sheet1 = cache.get_or_parse(css);
        let sheet2 = cache.get_or_parse(css);

        // Stylo Arc pointers should be equal if cached
        // contents is Locked<Arc<StylesheetContents>>
        let guard1 = sheet1.shared_lock.read();
        let contents1 = sheet1.contents.read_with(&guard1);

        let guard2 = sheet2.shared_lock.read();
        let contents2 = sheet2.contents.read_with(&guard2);

        assert!(
            Arc::ptr_eq(contents1, contents2),
            "Stylesheet contents should be shared"
        );
    }
}
