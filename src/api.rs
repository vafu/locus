use std::collections::HashMap;

#[cfg(feature = "server")]
use crate::dbus::GraphIfaceProxy;
#[cfg(not(feature = "server"))]
use zbus::proxy;

pub const BUS_NAME: &str = "io.github.Locus";
pub const ROOT_PATH: &str = "/io/github/Locus";
pub const GRAPH_INTERFACE: &str = "io.github.Locus.Graph";
pub const NONE_STRING: &str = "";

pub type LinkTuple = (String, String, String);
#[cfg(feature = "server")]
pub type GraphProxy<'a> = GraphIfaceProxy<'a>;

#[cfg(not(feature = "server"))]
#[proxy(
    default_service = "io.github.Locus",
    default_path = "/io/github/Locus",
    interface = "io.github.Locus.Graph",
    gen_blocking = false,
    async_name = "GraphProxy"
)]
pub trait Graph {
    fn add_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> zbus::fdo::Result<()>;

    fn set_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> zbus::fdo::Result<()>;

    fn remove_link(&self, source: &str, relation: &str, target: &str) -> zbus::fdo::Result<()>;

    fn remove_links(&self, source: &str, relation: &str) -> zbus::fdo::Result<()>;

    fn get_targets(&self, source: &str, relation: &str) -> zbus::fdo::Result<Vec<String>>;

    fn get_sources(&self, target: &str, relation: &str) -> zbus::fdo::Result<Vec<String>>;

    fn get_links(&self, subject: &str) -> zbus::fdo::Result<Vec<LinkTuple>>;

    fn get_all_links(&self) -> zbus::fdo::Result<Vec<LinkTuple>>;

    fn set_property(
        &self,
        subject: &str,
        key: &str,
        value: &str,
        durable: bool,
    ) -> zbus::fdo::Result<()>;

    fn remove_property(&self, subject: &str, key: &str) -> zbus::fdo::Result<()>;

    fn get_property(&self, subject: &str, key: &str) -> zbus::fdo::Result<String>;

    fn get_properties(&self, subject: &str) -> zbus::fdo::Result<HashMap<String, String>>;

    fn get_subjects(&self) -> zbus::fdo::Result<Vec<String>>;

    fn find_subjects(&self, key: &str, value: &str) -> zbus::fdo::Result<Vec<String>>;

    #[zbus(signal)]
    fn link_added(&self, source: String, relation: String, target: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn link_removed(&self, source: String, relation: String, target: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn link_set(
        &self,
        source: String,
        relation: String,
        old_targets: Vec<String>,
        target: String,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    fn property_changed(&self, subject: String, key: String, value: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn property_removed(&self, subject: String, key: String) -> zbus::Result<()>;
}

pub struct LocusClient<'a> {
    proxy: GraphProxy<'a>,
}

pub type Client<'a> = LocusClient<'a>;

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
    ) -> zbus::fdo::Result<()> {
        self.proxy.add_link(source, relation, target, durable).await
    }

    pub async fn set_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> zbus::fdo::Result<()> {
        self.proxy.set_link(source, relation, target, durable).await
    }

    pub async fn remove_link(
        &self,
        source: &str,
        relation: &str,
        target: &str,
    ) -> zbus::fdo::Result<()> {
        self.proxy.remove_link(source, relation, target).await
    }

    pub async fn remove_links(&self, source: &str, relation: &str) -> zbus::fdo::Result<()> {
        self.proxy.remove_links(source, relation).await
    }

    pub async fn targets(&self, source: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        self.proxy.get_targets(source, relation).await
    }

    pub async fn sources(&self, target: &str, relation: &str) -> zbus::fdo::Result<Vec<String>> {
        self.proxy.get_sources(target, relation).await
    }

    pub async fn links(&self, subject: &str) -> zbus::fdo::Result<Vec<LinkTuple>> {
        self.proxy.get_links(subject).await
    }

    pub async fn all_links(&self) -> zbus::fdo::Result<Vec<LinkTuple>> {
        self.proxy.get_all_links().await
    }

    pub async fn set_property(
        &self,
        subject: &str,
        key: &str,
        value: &str,
        durable: bool,
    ) -> zbus::fdo::Result<()> {
        self.proxy.set_property(subject, key, value, durable).await
    }

    pub async fn remove_property(&self, subject: &str, key: &str) -> zbus::fdo::Result<()> {
        self.proxy.remove_property(subject, key).await
    }

    pub async fn property(&self, subject: &str, key: &str) -> zbus::fdo::Result<Option<String>> {
        let value = self.proxy.get_property(subject, key).await?;
        Ok((value != NONE_STRING).then_some(value))
    }

    pub async fn properties(&self, subject: &str) -> zbus::fdo::Result<HashMap<String, String>> {
        self.proxy.get_properties(subject).await
    }

    pub async fn subjects(&self) -> zbus::fdo::Result<Vec<String>> {
        self.proxy.get_subjects().await
    }

    pub async fn find_subjects(
        &self,
        key: &str,
        value: Option<&str>,
    ) -> zbus::fdo::Result<Vec<String>> {
        self.proxy
            .find_subjects(key, value.unwrap_or(NONE_STRING))
            .await
    }

    pub async fn set_context_link(
        &self,
        context: &str,
        relation: &str,
        target: &str,
        durable: bool,
    ) -> zbus::fdo::Result<()> {
        self.proxy
            .set_link(&context_subject(context), relation, target, durable)
            .await
    }

    pub async fn context_targets(
        &self,
        context: &str,
        relation: &str,
    ) -> zbus::fdo::Result<Vec<String>> {
        self.proxy
            .get_targets(&context_subject(context), relation)
            .await
    }
}

fn context_subject(context: &str) -> String {
    format!("context:{context}")
}
