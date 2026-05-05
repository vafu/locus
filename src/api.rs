use std::collections::HashMap;

use zbus::proxy;

pub const BUS_NAME: &str = "io.github.Locus";
pub const ROOT_PATH: &str = "/io/github/Locus";
pub const GRAPH_INTERFACE: &str = "io.github.Locus.Graph";
pub const NONE_STRING: &str = "";

pub type LinkTuple = (String, String, String);

#[proxy(
    default_service = "io.github.Locus",
    default_path = "/io/github/Locus",
    interface = "io.github.Locus.Graph"
)]
pub trait Graph {
    fn add_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> zbus::Result<()>;

    fn remove_link(&self, source: &str, relation: &str, target: &str) -> zbus::Result<()>;

    fn remove_links(&self, source: &str, relation: &str) -> zbus::Result<()>;

    fn get_targets(&self, source: &str, relation: &str) -> zbus::Result<Vec<String>>;

    fn get_sources(&self, target: &str, relation: &str) -> zbus::Result<Vec<String>>;

    fn get_links(&self, subject: &str) -> zbus::Result<Vec<LinkTuple>>;

    fn get_all_links(&self) -> zbus::Result<Vec<LinkTuple>>;

    fn set_property(
        &self,
        subject: &str,
        key: &str,
        value: &str,
        durable: bool,
    ) -> zbus::Result<()>;

    fn remove_property(&self, subject: &str, key: &str) -> zbus::Result<()>;

    fn get_property(&self, subject: &str, key: &str) -> zbus::Result<String>;

    fn get_properties(&self, subject: &str) -> zbus::Result<HashMap<String, String>>;

    fn ensure_project(
        &self,
        path: &str,
        name: &str,
        icon: &str,
        durable: bool,
    ) -> zbus::Result<String>;

    fn list_projects(&self) -> zbus::Result<Vec<String>>;

    fn set_context_link(
        &self,
        context: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> zbus::Result<()>;

    fn get_context_targets(&self, context: &str, relation: &str) -> zbus::Result<Vec<String>>;

    #[zbus(signal)]
    fn link_added(&self, source: String, relation: String, target: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn link_removed(&self, source: String, relation: String, target: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn property_changed(&self, subject: String, key: String, value: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn property_removed(&self, subject: String, key: String) -> zbus::Result<()>;
}

pub struct LocusClient<'a> {
    proxy: GraphProxy<'a>,
}

pub type Client<'a> = LocusClient<'a>;

#[derive(Debug, Clone)]
pub struct ProjectSpec<'a> {
    pub path: &'a str,
    pub name: Option<&'a str>,
    pub icon: Option<&'a str>,
    pub durable: bool,
}

impl<'a> ProjectSpec<'a> {
    pub fn new(path: &'a str) -> Self {
        Self {
            path,
            name: None,
            icon: None,
            durable: false,
        }
    }

    pub fn name(mut self, name: &'a str) -> Self {
        self.name = Some(name);
        self
    }

    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn durable(mut self) -> Self {
        self.durable = true;
        self
    }
}

impl<'a> LocusClient<'a> {
    pub async fn new(connection: &'a zbus::Connection) -> zbus::Result<Self> {
        Ok(Self {
            proxy: GraphProxy::new(connection).await?,
        })
    }

    pub async fn add_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> zbus::Result<()> {
        self.proxy.add_link(source, relation, target, durable).await
    }

    pub async fn remove_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::Result<()> {
        self.proxy.remove_link(source, relation, target).await
    }

    pub async fn remove_links(&self, source: &str, relation: &str) -> zbus::Result<()> {
        self.proxy.remove_links(source, relation).await
    }

    pub async fn targets(&self, source: &str, relation: &str) -> zbus::Result<Vec<String>> {
        self.proxy.get_targets(source, relation).await
    }

    pub async fn sources(&self, target: &str, relation: &str) -> zbus::Result<Vec<String>> {
        self.proxy.get_sources(target, relation).await
    }

    pub async fn links(&self, subject: &str) -> zbus::Result<Vec<LinkTuple>> {
        self.proxy.get_links(subject).await
    }

    pub async fn all_links(&self) -> zbus::Result<Vec<LinkTuple>> {
        self.proxy.get_all_links().await
    }

    pub async fn set_property(
        &self,
        subject: &str,
        key: &str,
        value: &str,
        durable: bool,
    ) -> zbus::Result<()> {
        self.proxy.set_property(subject, key, value, durable).await
    }

    pub async fn remove_property(&self, subject: &str, key: &str) -> zbus::Result<()> {
        self.proxy.remove_property(subject, key).await
    }

    pub async fn property(&self, subject: &str, key: &str) -> zbus::Result<Option<String>> {
        let value = self.proxy.get_property(subject, key).await?;
        Ok((value != NONE_STRING).then_some(value))
    }

    pub async fn properties(&self, subject: &str) -> zbus::Result<HashMap<String, String>> {
        self.proxy.get_properties(subject).await
    }

    pub async fn ensure_project(
        &self,
        path: &str,
        name: Option<&str>,
        icon: Option<&str>,
        durable: bool,
    ) -> zbus::Result<String> {
        self.proxy
            .ensure_project(
                path,
                name.unwrap_or(NONE_STRING),
                icon.unwrap_or(NONE_STRING),
                durable,
            )
            .await
    }

    pub async fn ensure_project_spec(&self, spec: ProjectSpec<'_>) -> zbus::Result<String> {
        self.ensure_project(spec.path, spec.name, spec.icon, spec.durable)
            .await
    }

    pub async fn list_projects(&self) -> zbus::Result<Vec<String>> {
        self.proxy.list_projects().await
    }

    pub async fn set_context_link(
        &self,
        context: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> zbus::Result<()> {
        self.proxy
            .set_context_link(context, relation, target, durable)
            .await
    }

    pub async fn context_targets(
        &self,
        context: &str,
        relation: &str,
    ) -> zbus::Result<Vec<String>> {
        self.proxy.get_context_targets(context, relation).await
    }
}
