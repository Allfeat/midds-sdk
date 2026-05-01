//! Subxt bindings generator for the MIDDS client.
//!
//! Wraps [`subxt_codegen::CodegenBuilder`] behind a small CLI: point it at a
//! running node (`--metadata ws://…`) or a SCALE-encoded metadata file, and it
//! emits a single Rust module ready to be `include!`d (or copied) by
//! `midds-client`.

use std::{path::PathBuf, str::FromStr};

use anyhow::{Context, Result};
use clap::Parser;
use parity_scale_codec::Decode;
use subxt_codegen::{CodegenBuilder, Metadata};
use subxt_utils_fetchmetadata::{MetadataVersion, Url, from_file_blocking, from_url_blocking};

#[derive(Parser, Debug)]
#[command(
    name = "midds-codegen",
    about = "Generate subxt bindings for midds-client from runtime metadata.",
    version
)]
struct Cli {
    /// WS/HTTP URL of an Allfeat node (`ws://localhost:9944`) or path to a
    /// SCALE-encoded metadata file (typically produced by
    /// `subxt metadata -f bytes`).
    #[arg(long, value_name = "URL_OR_PATH")]
    metadata: String,

    /// Output file. Defaults to `crates/midds-client/src/generated.rs`, which
    /// is where `midds-client` expects the generated bindings.
    #[arg(
        long,
        value_name = "PATH",
        default_value = "crates/midds-client/src/generated.rs"
    )]
    out: PathBuf,

    /// Metadata version to request when fetching over the network: `latest`
    /// (default), `unstable`, or an integer (e.g. `15`). Ignored when reading
    /// from a file.
    #[arg(long, default_value = "latest")]
    metadata_version: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let bytes = read_metadata_bytes(&cli.metadata, &cli.metadata_version)?;
    let metadata = Metadata::decode(&mut &bytes[..]).context("decode SCALE-encoded metadata")?;

    let tokens = CodegenBuilder::new()
        .generate(metadata)
        .map_err(|e| anyhow::anyhow!("generate subxt bindings: {e}"))?;

    if let Some(parent) = cli.out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    std::fs::write(&cli.out, tokens.to_string())
        .with_context(|| format!("write {}", cli.out.display()))?;

    let _ = std::process::Command::new("rustfmt").arg(&cli.out).status();

    eprintln!("Generated {}", cli.out.display());
    Ok(())
}

fn read_metadata_bytes(source: &str, version: &str) -> Result<Vec<u8>> {
    if is_url(source) {
        let url = Url::from_str(source).with_context(|| format!("parse url {source}"))?;
        let version = MetadataVersion::from_str(version)
            .map_err(|e| anyhow::anyhow!("invalid metadata version `{version}`: {e}"))?;
        from_url_blocking(url, version, None)
            .with_context(|| format!("fetch metadata from {source}"))
    } else {
        let path = PathBuf::from(source);
        from_file_blocking(&path).with_context(|| format!("read {}", path.display()))
    }
}

fn is_url(s: &str) -> bool {
    s.starts_with("ws://")
        || s.starts_with("wss://")
        || s.starts_with("http://")
        || s.starts_with("https://")
}
