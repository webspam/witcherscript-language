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
            self.top_level
                .entry(name.clone())
                .or_default()
                .insert(subscriber_uri.to_string());
        }
        for key in &observations.members {
            self.members
                .entry(key.clone())
                .or_default()
                .insert(subscriber_uri.to_string());
        }
        for name in &observations.enum_members {
            self.enum_members
                .entry(name.clone())
                .or_default()
                .insert(subscriber_uri.to_string());
        }
        self.keys_by_subscriber
            .insert(subscriber_uri.to_string(), observations);
    }

    pub fn unregister(&mut self, subscriber_uri: &str) {
        let Some(prev) = self.keys_by_subscriber.remove(subscriber_uri) else {
            return;
        };
        for name in prev.top_level {
            if let Some(set) = self.top_level.get_mut(&name) {
                set.remove(subscriber_uri);
                if set.is_empty() {
                    self.top_level.remove(&name);
                }
            }
        }
        for key in prev.members {
            if let Some(set) = self.members.get_mut(&key) {
                set.remove(subscriber_uri);
                if set.is_empty() {
                    self.members.remove(&key);
                }
            }
        }
        for name in prev.enum_members {
            if let Some(set) = self.enum_members.get_mut(&name) {
                set.remove(subscriber_uri);
                if set.is_empty() {
                    self.enum_members.remove(&name);
                }
            }
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
