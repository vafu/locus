use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "locus-codegen")]
#[command(about = "Generate helpers from a Locus schema")]
pub struct Args {
    #[arg(long, default_value = "schema.yaml")]
    pub schema: PathBuf,
    #[arg(long, value_enum, default_value_t = Language::Ts)]
    pub language: Language,
    #[arg(long, value_enum)]
    pub adapter: Option<Adapter>,
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum Language {
    #[value(alias = "typescript")]
    Ts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum Adapter {
    Rx,
}
