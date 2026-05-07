use std::io::Write;

use anyhow::Result;
use locus_schema::GraphSchema;

use crate::args::{Adapter, Language};
use crate::shell::ShellProducer;
use crate::typescript::TypeScriptProducer;

pub trait CodeProducer {
    fn language(&self) -> &'static str;
    fn file_extension(&self) -> &'static str;
    fn generate(&self, schema: &GraphSchema, out: &mut dyn Write) -> Result<()>;
}

pub fn producer(language: Language, adapter: Option<Adapter>) -> Box<dyn CodeProducer> {
    match language {
        Language::Shell => Box::new(ShellProducer::new(adapter)),
        Language::Ts => Box::new(TypeScriptProducer::new(adapter)),
    }
}
