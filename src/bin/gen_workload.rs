//! Synthetic workload generator with Zipf-distributed reuse distances.
//!
//! All keys `0..num_keys` are kept in a recency list (most recently queried first).
//! For every request, a rank `r` is sampled from a Zipf distribution over
//! `1..=num_keys` with exponent `s`, and the key at position `r - 1` of the list
//! (i.e. the key whose current reuse distance is `r - 1`) is queried and moved to the
//! front, which updates the reuse distances of all other keys. Each request is a write
//! with probability `--write-ratio` (the written value is the request index), otherwise
//! a read. `s = 0` gives uniformly random reuse distances.
//!
//! One workload file is written per exponent, named `s<exponent>` (one decimal), with
//! one request per line: `key` (read) or `key value` (write).
//!
//! The defaults generate the paper's Zipf-exponent sweep (`s0.0` ... `s2.0`, 1M keys,
//! 1M requests, 50% writes); `s1.0` is also the synthetic workload used by the other
//! experiments. Example (a single small workload):
//!
//! ```text
//! gen_workload --out-dir /tmp/wl --num-keys 10000 --num-requests 30000 \
//!     --s-start 1.0 --s-end 1.0 --seed 1
//! ```
use clap::Parser;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rand_distr::{Distribution, Zipf};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about = "Generate synthetic workloads with Zipf-distributed reuse distances")]
struct Args {
    /// Directory the workload files are written to (must exist)
    #[arg(long, default_value = ".")]
    out_dir: PathBuf,
    /// Number of distinct keys (keys are 0..num_keys)
    #[arg(long, default_value_t = 1_000_000)]
    num_keys: usize,
    /// Number of requests per workload
    #[arg(long, default_value_t = 1_000_000)]
    num_requests: usize,
    /// Probability that a request is a write
    #[arg(long, default_value_t = 0.5)]
    write_ratio: f64,
    /// First Zipf exponent of the sweep
    #[arg(long, default_value_t = 0.0)]
    s_start: f32,
    /// Last Zipf exponent of the sweep (inclusive)
    #[arg(long, default_value_t = 2.0)]
    s_end: f32,
    /// Step between Zipf exponents
    #[arg(long, default_value_t = 0.1)]
    s_step: f32,
    /// RNG seed (default: seeded from the OS, i.e. a different workload every run)
    #[arg(long)]
    seed: Option<u64>,
}

/// Moves the element at `idx` to the front of the list and returns it.
fn move_to_front(id_list: &mut [usize], idx: usize) -> usize {
    let selected = id_list[idx];
    id_list.copy_within(0..idx, 1); // shift the prefix right by 1
    id_list[0] = selected;
    selected
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    assert!(args.num_keys >= 1, "--num-keys must be at least 1");
    assert!(args.s_start >= 0.0, "Zipf exponents must be >= 0");
    assert!(args.s_end >= args.s_start, "--s-end must be >= --s-start");
    assert!((0.0..=1.0).contains(&args.write_ratio), "--write-ratio must be in [0, 1]");

    let mut rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_os_rng(),
    };

    let mut s = args.s_start;
    loop {
        let path = args.out_dir.join(format!("s{:.1}", s));
        let mut file = BufWriter::new(File::create(&path)?);
        let mut id_list: Vec<usize> = (0..args.num_keys).collect();
        let zipf = Zipf::new(args.num_keys as f32, s).unwrap();

        for i in 0..args.num_requests {
            let id = zipf.sample(&mut rng) as usize - 1;
            assert!(id < args.num_keys);
            let selected_id = move_to_front(&mut id_list, id);
            if rng.random_bool(args.write_ratio) {
                writeln!(file, "{} {}", selected_id, i)?;
            } else {
                writeln!(file, "{}", selected_id)?;
            }
        }
        file.flush()?;
        println!("wrote {}", path.display());

        if args.s_step <= 0.0 {
            break;
        }
        s += args.s_step;
        if s > args.s_end + args.s_step / 2.0 {
            break;
        }
    }
    Ok(())
}
