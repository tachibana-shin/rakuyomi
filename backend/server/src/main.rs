use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
struct Args {
    home_path: PathBuf,
}

fn main() -> Result<()> {
    server::log_startup();

    let args = Args::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("rakuyomi-main")
        .build()?;

    runtime.block_on(server::run(args.home_path))
}
