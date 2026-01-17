use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use tofu::process_graph;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the graph.dot file
    #[arg(long, default_value = "graph.dot")]
    graph: PathBuf,

    /// Path to the planfile.json file
    #[arg(long, default_value = "planfile.json")]
    plan: PathBuf,

    /// Path to the output json file
    #[arg(long, default_value = "output.json")]
    out: PathBuf,

    /// Path to the source directory containing .tf files
    #[arg(long)]
    dir: Option<PathBuf>,

    /// Include values in the output
    #[arg(long)]
    include_values: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // 1. Parse Plan File
    let plan_content = fs::read_to_string(&cli.plan).context("Failed to read plan file")?;

    // 2. Parse DOT File
    let dot_content = fs::read_to_string(&cli.graph).context("Failed to read graph file")?;

    // 3. Process
    let graph = process_graph(&plan_content, &dot_content, cli.include_values, cli.dir)?;

    let json_output = serde_json::to_string_pretty(&graph)?;
    fs::write(&cli.out, json_output).context("Failed to write output file")?;

    println!("Successfully generated {}", cli.out.display());

    Ok(())
}
