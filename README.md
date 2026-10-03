# Cloak

Cloak is an oblivious storage system. A trusted proxy sits between clients and an
untrusted storage server. It encrypts every element and sends the server only fixed-size
batches whose accesses follow a fixed, recentness-biased distribution over
*reuse-distance sets*. Real client queries fill as many batch slots as possible, and
dummy accesses fill the rest. When the workload has temporal locality, most slots are
real queries and Cloak's throughput comes close to that of a non-oblivious baseline.
The budget distribution affects only performance, not security.

This repository contains the implementation and the scripts for every experiment in
the paper:

> **Cloak: Heuristic ORAM Optimization Through Fixed Temporal Distribution**
> Onur Eren Arpaci, Florian Kerschbaum, Sujaya Maiyya (University of Waterloo)

Cloak is a research prototype. Among other things, the encryption key is hardcoded and
only one client connection is supported. Do not use it to protect real data.

## Repository layout

```
src/                 Rust implementation (crate `cloak`)
  bin/server.rs        storage server (holds encrypted elements in memory)
  bin/proxy.rs         Cloak proxy (batching, reuse-distance sets, cache, encryption)
  bin/client.rs        workload driver; writes per-run throughput/latency stats
  bin/budget_ratio.rs  turns the proxy's budget log into batch-utilization stats
  bin/gen_workload.rs  synthetic workload generator (Zipf-distributed reuse distances)
local-test/          one tiny end-to-end run on localhost
scripts/run-local.sh one repetition of any experiment on localhost
ansible/             cluster deployment and per-run orchestration (3 machines)
experiments/         one folder per experiment in the paper (README, run script,
                     configs and workloads where needed, and the raw results behind
                     the paper's numbers)
plots/               scripts that draw the paper's figures and table
```

| Experiment | Paper | Cluster? | Full run (paper hardware) |
|---|---|---|---|
| [`experiments/datasets`](experiments/datasets/) | Table 2: Cloak vs. the unsafe baseline on 8 workloads | yes | ~2 h |
| [`experiments/zipf-exponent`](experiments/zipf-exponent/) | Figure 3, column 1 (workload skew) | yes | ~1 h |
| [`experiments/batch-size`](experiments/batch-size/) | Figure 3, column 2 (batch size) | yes | ~25 min |
| [`experiments/cache-size`](experiments/cache-size/) | Figure 3, column 3 (cache size) | yes | ~20 min |
| [`experiments/element-size`](experiments/element-size/) | Figure 3, column 4 (element size) | yes | ~1.5 h |
| [`experiments/temporal-locality`](experiments/temporal-locality/) | Figures 2 and 4 (reuse-interval histograms) | no | ~1 min |

The cluster times are for 5 repetitions with the Ansible controller in the same
cloud region as the machines: the client's run time plus about 20 s of fixed
cost per run (orchestration, server start, storage initialization). With a
remote controller, e.g. a laptop on a home connection, the fixed cost is
higher, and the first run of each Table 2 dataset also copies its workload to
the client (~430 MB decompressed for all eight), which can take minutes; later
runs skip the copy.

## Building

Requirements:

- A stable Rust toolchain from [rustup](https://rustup.rs) (tested with 1.91).
- OpenSSL development headers and `pkg-config`:
  - Ubuntu/Debian: `sudo apt-get install build-essential pkg-config libssl-dev`
  - macOS: `brew install openssl@3 pkg-config`. If the build can't find OpenSSL, set
    `OPENSSL_DIR="$(brew --prefix openssl@3)"`.

```bash
cargo build --release
```

```bash
cargo test
```

The binaries end up in `target/release/`: `server`, `proxy`, `client`, `budget_ratio`
and `gen_workload`. `server`, `proxy` and `client` take `--config <file.toml>`, and every
config key can be overridden on the command line (e.g. `--proxy-cache-size 4000`,
`--client-req-per-sec 100000`). See `<binary> --help` and the annotated config in
[`local-test/config.toml`](local-test/config.toml).

## Quick local test

```bash
local-test/run.sh
```

This builds the project and then runs a storage server, a proxy and a client on
`127.0.0.1` over a 30,000-request workload. The run takes a few seconds and ends by
printing the throughput, latency and batch-utilization results.
`local-test/run.sh --proxy-is-unsafe true` runs the same workload through the
non-oblivious baseline. [`local-test/README.md`](local-test/README.md) shows the
expected output and how to start the three binaries by hand.

You can also run any experiment's configurations locally, on short workloads, without
a cluster. Results go to `results-smoke/`:

```bash
LOCAL=1 SMOKE=1 SMOKE_REQUESTS=20000 ONLY='^s1\.0$' experiments/zipf-exponent/run.sh
```

Locally the server and proxy each allocate the whole store (1M elements × element
size, about 1 GB at the default 1 KB elements), and the numbers say nothing about
performance.

## Running the experiments on a cluster

The paper's experiments run the server, the proxy and the client on three separate
machines. Ansible, running on a separate *controller* (your laptop or another
machine), builds Cloak on the machines and drives every run.

### 1. Machines

- Three machines running Ubuntu 22.04 or 24.04, reachable over SSH from the controller.
  The SSH user needs passwordless `sudo`.
- The paper used three AWS **m6a.4xlarge** instances (16 vCPUs, 64 GB RAM each) in the
  same availability zone. To reproduce the absolute numbers you need comparable
  machines and network. Memory needed on the server and the proxy is roughly
  1M × element size: about 1 GB for most experiments, and about 33 GB for the 32 KB
  point of the element-size sweep.
- The machines must accept TCP connections from each other on ports **4000** (server)
  and **5050** (proxy), on the addresses they use to reach each other (see below).
- The machines need internet access during deployment (apt, rustup and crates.io).
  They should be dedicated to Cloak: every run first kills whatever listens on ports
  4000/5050.

To reproduce the paper's numbers on AWS:

- set `ansible_host` to each instance's public IP and leave `private_ip` unset;
- in the security group, allow TCP 4000 and 5050 from the three instances' public IPs

### 2. Controller

- `ansible-core` 2.12 or newer (e.g. `pipx install ansible-core`,
  `brew install ansible`, or `sudo apt-get install ansible`), `bash`, and `xz`.
- For the figures: [uv](https://docs.astral.sh/uv/) (or Python ≥ 3.11 with
  matplotlib, numpy, pandas and tol-colors).

### 3. Inventory

```bash
cp ansible/inventory.example ansible/inventory
```

Edit `ansible/inventory` and put one machine in each of the groups `server`, `proxy`
and `client`:

- `ansible_host` is the address the controller uses for SSH.
- `private_ip` (optional) is the address the machines use to reach each other, if
  different from `ansible_host` (its default). On AWS, leave it unset (see
  "Network path" above).
- Set `ansible_user` and `ansible_ssh_private_key_file` under `[all:vars]`.

Check connectivity from the `ansible/` directory:

```bash
cd ansible && ansible all -m ping
```

### 4. Deploy

```bash
cd ansible && ansible-playbook deploy.yml
```

`deploy.yml` waits for cloud-init and the apt lock on fresh VMs and installs the build
dependencies and a Rust toolchain. It also masks Ubuntu's `apt-daily` timers, because
on fresh cloud VMs they otherwise fire during measurements and restart services. Then
it copies `src/`, `Cargo.toml` and `Cargo.lock` to `~/cloak` on every machine and runs
`cargo build --release` there. On fresh m6a.4xlarge instances this takes about a
minute (a rerun about 25 s). After changing the sources, rebuild with
`ansible-playbook deploy.yml --tags build`.

### 5. Run

Do a quick end-to-end check of the cluster first, with short workloads and one
repetition (about a minute):

```bash
SMOKE=1 ONLY='^s(0\.0|1\.0)$' experiments/zipf-exponent/run.sh
```

Then run whole experiments, for example:

```bash
experiments/batch-size/run.sh
```

Each `run.sh` loops over repetitions (5 by default) and configurations. Every run
appends one row to `results/<label>-stats.csv` (and `results/<label>-ratio.csv`) in the
experiment folder, and the scripts can be run from any directory. Useful variables:

| Variable | Effect |
|---|---|
| `REPS=n` | repetitions (default 5) |
| `ONLY=<regex>` | run only the matching configuration labels |
| `SMOKE=1` | cut workloads to `SMOKE_REQUESTS` (default 200,000) requests; results go to `results-smoke/` |
| `DRY_RUN=1` | print what would run |
| `INVENTORY=<file>` | use another inventory |

A failed repetition is logged in `results/runs.log` and `results/logs/`, and the sweep
moves on. If you interrupt a sweep, the next run cleans up leftover processes. To clean
up right away:

```bash
cd ansible && ansible-playbook stop.yml
```

See [`experiments/README.md`](experiments/README.md) for the full list of variables
and the result file formats, and each experiment's README for its parameter grid,
client rates and expected runtime.

## Figures and tables

```bash
uv run python plots/fig3_ablations.py               # Figure 3, from experiments/*/results/
uv run python plots/fig3_ablations.py --paper       # Figure 3, from the paper's raw results
uv run python plots/fig3_ablations.py --smoke       # Figure 3, from experiments/*/results-smoke/
uv run python plots/table2_datasets.py --paper      # Table 2 (Cloak and unsafe columns)
experiments/temporal-locality/run.sh                # Figures 2 and 4 (computes the histograms, then plots)
uv run python plots/fig2_fig4_temporal_locality.py --paper
```

Output goes to `plots/out/` (`--out-dir` changes it): figures as PDF and PNG (`--pgf`
also writes PGF, which needs a LaTeX installation), and the table as CSV plus Markdown
on stdout. With `--paper` the scripts read each experiment's `paper-results/` folder,
which holds the raw per-repetition rows behind the numbers in the paper. Without it
they read your own `results/`; `--smoke` reads `results-smoke/`. For results written
elsewhere (`RESULTS_DIR=...`), see
[`experiments/README.md`](experiments/README.md#plotting).

## Datasets

The workloads used in the paper ship xz-compressed in each experiment's `workloads/`
folder, except MSR prxy (below), and are decompressed on first use into `.cache/`. The
eight Table 2 workloads come from public sources: Netflix, Ethereum, Twitter, Meta,
Wikimedia, MSR Cambridge and Alibaba traces, plus one synthetic workload.
[`experiments/datasets/prep/README.md`](experiments/datasets/prep/README.md) gives each
one's source, license and attribution, and describes how it was sampled. It also has
scripts that regenerate the workloads from the raw traces.

The MSR prxy workload is not included, because the MSR Cambridge traces are not
redistributed. Download `msr-cambridge1.tar` (3.3 GB) from SNIA IOTTA into
`experiments/datasets/prep/data/msr/` and run
`experiments/datasets/prep/make_workloads.sh msr-prxy` (about 1.5 minutes). It
regenerates the paper's file byte for byte, checks its SHA-256 and installs it where the
harness expects it ([details](experiments/datasets/prep/README.md#msr-prxy)). Until
then, `experiments/datasets/run.sh` and `experiments/temporal-locality/run.sh` skip MSR
prxy with a warning.

## License

The code and scripts in this repository are released under the [MIT License](LICENSE).
The workloads derived from third-party traces remain subject to their sources'
terms; see [`experiments/datasets/prep/README.md`](experiments/datasets/prep/README.md).
In particular, `experiments/datasets/workloads/netflix.xz` is licensed under
CC BY-NC-SA 3.0 IGO ([notice](experiments/datasets/workloads/netflix.LICENSE.txt)).
