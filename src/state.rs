use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Link {
    pub source: String,
    pub relation: String,
    pub target: String,
}

impl Link {
    pub fn new(source: &str, relation: &str, target: &str) -> Self {
        Self {
            source: source.to_string(),
            relation: relation.to_string(),
            target: target.to_string(),
        }
    }

    pub fn to_tuple(&self) -> (String, String, String) {
        (
            self.source.clone(),
            self.relation.clone(),
            self.target.clone(),
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeState {
    pub links: BTreeSet<Link>,
    pub properties: BTreeMap<(String, String), String>,
}

impl RuntimeState {
    pub fn links(&self) -> BTreeSet<Link> {
        self.links.clone()
    }

    pub fn properties_for(&self, subject: &str) -> BTreeMap<String, String> {
        let mut properties = BTreeMap::new();
        for ((property_subject, key), value) in &self.properties {
            if property_subject == subject {
                properties.insert(key.clone(), value.clone());
            }
        }
        properties
    }

    pub fn property(&self, subject: &str, key: &str) -> Option<String> {
        self.properties
            .get(&(subject.to_string(), key.to_string()))
            .cloned()
    }
}
