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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkSetChange {
    Unchanged,
    Changed { removed: Vec<Link>, added: Link },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyChange {
    Unchanged,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteNodeChange {
    pub removed_links: Vec<Link>,
    pub removed_properties: Vec<(String, String)>,
}

impl DeleteNodeChange {
    pub fn is_empty(&self) -> bool {
        self.removed_links.is_empty() && self.removed_properties.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub source: String,
    pub path: Vec<String>,
    pub target: Option<String>,
}
