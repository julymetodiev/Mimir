//! Read command: the source of a symbol by name.

use anyhow::Result;
use latt_client::Client;
use serde::Serialize;
use std::path::Path;

/// Output structure for JSON mode.
#[derive(Serialize)]
struct ReadOutput<'a> {
    symbol: &'a str,
    kind: String,
    file: String,
    start_line: u32,
    end_line: u32,
    lines: Vec<LineOutput<'a>>,
}

#[derive(Serialize)]
struct LineOutput<'a> {
    number: u32,
    content: &'a str,
    is_symbol: bool,
}

/// Execute the read command.
pub async fn execute(
    symbol_name: &str,
    file: Option<&Path>,
    before: usize,
    after: usize,
    show_numbers: bool,
    json: bool,
) -> Result<()> {
    let mut client = Client::open(&std::env::current_dir()?).await?;
    let source = client
        .read(
            symbol_name,
            file.map(Path::canonicalize).transpose()?,
            before as u32,
            after as u32,
        )
        .await?;
    let symbol = &source.symbol;
    let numbered = source
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| (source.first_line + i as u32, line.as_str()));
    let in_symbol = |number: u32| number >= symbol.start_line && number <= symbol.end_line;

    if json {
        let output = ReadOutput {
            symbol: &symbol.name,
            kind: symbol.kind.to_string(),
            file: symbol.file_path.display().to_string(),
            start_line: symbol.start_line,
            end_line: symbol.end_line,
            lines: numbered
                .map(|(number, content)| LineOutput {
                    number,
                    content,
                    is_symbol: in_symbol(number),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!(
        "{}  [{}]  {}:{}-{}\n",
        symbol.name,
        symbol.kind,
        symbol.file_path.display(),
        symbol.start_line,
        symbol.end_line
    );
    for (number, line) in numbered {
        let marker = if in_symbol(number) { " " } else { "·" };
        if show_numbers {
            println!("{marker}{number:4}│ {line}");
        } else {
            println!("{marker} {line}");
        }
    }
    Ok(())
}
