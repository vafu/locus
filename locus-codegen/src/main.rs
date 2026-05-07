mod args;
mod producer;
mod shell;
mod typescript;

use std::fs::File;
use std::io::{self, Write};

use anyhow::Context;
use args::Args;
use clap::Parser;
use locus_schema::GraphSchema;
use producer::producer;

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let schema = GraphSchema::load(&args.schema)
        .with_context(|| format!("load schema {}", args.schema.display()))?;
    let producer = producer(args.language, args.adapter);
    debug_assert!(!producer.file_extension().is_empty());

    if let Some(out) = args.out {
        let mut file = File::create(&out).with_context(|| format!("write {}", out.display()))?;
        producer
            .generate(&schema, &mut file)
            .with_context(|| format!("generate {} output", producer.language()))?;
        file.flush()
            .with_context(|| format!("flush {}", out.display()))?;
    } else {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        producer
            .generate(&schema, &mut stdout)
            .with_context(|| format!("generate {} output", producer.language()))?;
        stdout.flush().context("flush stdout")?;
    }

    Ok(())
}
