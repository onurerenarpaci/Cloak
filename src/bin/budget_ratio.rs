use clap::Parser;
use csv::Trim;
use std::error::Error;
use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::fs::File;
use std::io::Write;

/// Summarize the proxy's budget log into batch-utilization statistics.
///
/// The budget log (written by the proxy when `enable_budget_log = true`) is a CSV
/// whose header row holds the per-time-set budgets and whose every following row
/// holds the number of real requests served from each time-set in one batch.
/// For each batch, utilization = sum(row) / sum(header). The output CSV has the
/// header `batch_count,mean_batch_util,median_batch_util,calculated_batch_size`
/// and one data row (`calculated_batch_size` is the sum of the budgets).
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Path to the budget log CSV written by the proxy.
    #[arg(value_name = "BUDGET_LOG")]
    input: PathBuf,
    /// Output CSV file (relative paths are relative to the working directory).
    #[arg(value_name = "OUT_FILE", default_value = "ratio.csv")]
    output: PathBuf,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(Trim::All)
        .from_path(&args.input)?;

    let headers = reader
        .headers()?
        .iter()
        .enumerate()
        .map(|(idx, field)| parse_i128(field, "header", 1, idx + 1))
        .collect::<Result<Vec<_>, _>>()?;

    let header_sum: i128 = headers.iter().sum();
    if header_sum == 0 {
        return Err(io_error("Header sum is zero; cannot compute ratios"));
    }

    let mut ratios = Vec::new();
    for (row_idx, record) in reader.records().enumerate() {
        let record = record?;
        if record.len() != headers.len() {
            return Err(io_error(&format!(
                "Row {} has {} columns; expected {}",
                row_idx + 2,
                record.len(),
                headers.len()
            )));
        }

        let values = record
            .iter()
            .enumerate()
            .map(|(col_idx, field)| parse_i128(field, "row", row_idx + 2, col_idx + 1))
            .collect::<Result<Vec<_>, _>>()?;

        let row_sum: i128 = values.iter().sum();
        let ratio = row_sum as f64 / header_sum as f64;
        ratios.push(ratio);
    }

    if ratios.is_empty() {
        return Err(io_error("CSV file contains no data rows"));
    }

    let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;

    let median = {
        let mut sorted = ratios.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mid = sorted.len() / 2;
        if sorted.len() % 2 == 0 {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        }
    };

    let mut file = File::create(&args.output)?;
    writeln!(file, "batch_count,mean_batch_util,median_batch_util,calculated_batch_size")?;
    writeln!(file, "{},{},{},{}", ratios.len(), mean, median, header_sum)?;

    Ok(())
}

fn parse_i128(
    field: &str,
    kind: &str,
    row: usize,
    column: usize,
) -> Result<i128, Box<dyn Error>> {
    field.parse::<i128>().map_err(|_| {
        io_error(&format!(
            "Failed to parse {kind} value '{}' at row {}, column {} as integer",
            field, row, column
        ))
    })
}

fn io_error(message: &str) -> Box<dyn Error> {
    io::Error::new(ErrorKind::InvalidData, message.to_string()).into()
}
