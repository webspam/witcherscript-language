use std::collections::{HashMap, HashSet};

use super::workspace_index::ObservedKey;
use super::ObservationSet;

#[derive(Debug, Clone, Default)]
pub struct SubscriptionRegistry {
    top_level: HashMap<String, HashSet<String>>,
    members: HashMap<(String, String), HashSet<String>>,
    enum_members: HashMap<String, HashSet<String>>,
    keys_by_subscriber: HashMap<String, ObservationSet>,
}

impl SubscriptionRegistry {
    pub fn register(&mut self, subscriber_uri: &str, observations: ObservationSet) {
        self.unregister(subscriber_uri);
        for name in &observations.top_level {
            bucket_insert(&mut self.top_level, name.clone(), subscriber_uri);
        }
        for key in &observations.members {
            bucket_insert(&mut self.members, key.clone(), subscriber_uri);
        }
        for name in &observations.enum_members {
            bucket_insert(&mut self.enum_members, name.clone(), subscriber_uri);
        }
        self.keys_by_subscriber
            .insert(subscriber_uri.to_string(), observations);
    }

    pub fn unregister(&mut self, subscriber_uri: &str) {
        let Some(prev) = self.keys_by_subscriber.remove(subscriber_uri) else {
            return;
        };
        for name in prev.top_level {
            bucket_remove(&mut self.top_level, &name, subscriber_uri);
        }
        for key in prev.members {
            bucket_remove(&mut self.members, &key, subscriber_uri);
        }
        for name in prev.enum_members {
            bucket_remove(&mut self.enum_members, &name, subscriber_uri);
        }
    }

    pub fn subscribers_of(&self, keys: &[ObservedKey]) -> HashSet<String> {
        let mut out = HashSet::new();
        for key in keys {
            let bucket = match key {
                ObservedKey::TopLevel(n) => self.top_level.get(n),
                ObservedKey::Member(c, n) => self.members.get(&(c.clone(), n.clone())),
                ObservedKey::EnumMember(n) => self.enum_members.get(n),
            };
            if let Some(set) = bucket {
                for uri in set {
                    out.insert(uri.clone());
                }
            }
        }
        out
    }
}

fn bucket_insert<K: Eq + std::hash::Hash>(
    bucket: &mut HashMap<K, HashSet<String>>,
    key: K,
    subscriber_uri: &str,
) {
    bucket
        .entry(key)
        .or_default()
        .insert(subscriber_uri.to_string());
}

fn bucket_remove<K: Eq + std::hash::Hash>(
    bucket: &mut HashMap<K, HashSet<String>>,
    key: &K,
    subscriber_uri: &str,
) {
    let Some(set) = bucket.get_mut(key) else {
        return;
    };
    set.remove(subscriber_uri);
    if set.is_empty() {
        bucket.remove(key);
    }
}
