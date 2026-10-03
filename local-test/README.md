# Local smoke test

Builds Cloak and runs one tiny repetition (storage server, Cloak proxy and client, all
on `127.0.0.1`) to check that everything builds and works end to end. It takes a few
seconds and says nothing about performance; the paper's numbers come from three
separate machines (see `../experiments/`).

## Prerequisites

The build requirements in the top-level README's
[Building](../README.md#building) section, plus `bash` (3.2 or newer). Ports
4000 and 5050 must be free.

## Run

```bash
local-test/run.sh                          # Cloak
local-test/run.sh --proxy-is-unsafe true   # non-oblivious baseline
```

The script runs `cargo build --release`, then `scripts/run-local.sh` with
`config.toml` and `workload.txt`, and prints the result files written to
`local-test/results/`. Extra arguments are passed to the binaries as config overrides.

The workload has 30,000 requests over 10,000 keys (synthetic, Zipf-distributed reuse
distances with s = 1.0, 50% writes), sent at 10,000 requests/s, so a run takes about
3 seconds. Expected output (abbreviated: build and run-log lines omitted, CSV values
rounded; numbers vary between machines and runs):

```
== .../local-test/results/stats.csv
request_count,throughput,avg_latency,avg_latency_write,avg_latency_read,std_latency,p99_latency
30000,9785.7,3.96,0.005,7.87,7.69,28
== .../local-test/results/ratio.csv
batch_count,mean_batch_util,median_batch_util,calculated_batch_size
124,0.232,0.235,540

OK: 30000/30000 requests answered, throughput 9786 req/s, mean latency 4.0 ms (std 7.7, p99 28 ms), 124 batches of size 540, mean batch utilization 0.23
```

Throughput is close to the configured send rate because this setup is far from
saturated. With `--proxy-is-unsafe true` there is no `ratio.csv`: the baseline does not
write a budget log.

### Output files

`stats.csv` is written by the client and `ratio.csv` by `budget_ratio` from the
proxy's budget log; their columns are defined in
[`../experiments/README.md`](../experiments/README.md#result-files). Requests
still queued in the proxy when the client finishes sending are not answered, so
`request_count` can be slightly below the number of requests in the workload,
mostly when the proxy is saturated.

## Running the binaries by hand

The binaries write their files (stats, budget log, client log) relative to their
working directory and do not create missing directories, so start them from a scratch
directory. Use three terminals:

```bash
CLOAK=/path/to/Cloak            # this repository
mkdir -p /tmp/cloak-run && cd /tmp/cloak-run

# terminal 1: storage server (wait for "Starting server on ...")
$CLOAK/target/release/server --config $CLOAK/local-test/config.toml

# terminal 2: proxy; initializes the storage, then prints "listening for clients on ..."
$CLOAK/target/release/proxy --config $CLOAK/local-test/config.toml

# terminal 3: client
$CLOAK/target/release/client --config $CLOAK/local-test/config.toml \
    --client-input-file $CLOAK/local-test/workload.txt

# afterwards, in /tmp/cloak-run:
$CLOAK/target/release/budget_ratio budget_log.txt ratio.csv
cat stats.csv ratio.csv
```

The client exits when the workload is done; the proxy exits once the client has
disconnected. The server never exits on its own (it waits for the next proxy
connection); stop it with Ctrl-C. It holds no state that needs saving, so stopping it
is always safe. The config binds to `0.0.0.0`; add
`--common-server-addr-bind 127.0.0.1:4000 --common-proxy-addr-bind 127.0.0.1:5050`
to all three commands to listen on localhost only (as `scripts/run-local.sh` does).

Do not check whether the server or proxy is up by connecting to ports 4000/5050
(`nc -z`, `/dev/tcp`, Ansible `wait_for`): the server treats the first connection as
the proxy and exits if it does not receive the proxy's handshake, and the proxy serves
only the first connection it accepts. Wait for the log lines above, or look for a
listening socket (`lsof -iTCP:5050 -sTCP:LISTEN`, `ss -lnt`).

## `scripts/run-local.sh`

```
scripts/run-local.sh <config.toml> <workload> <out_dir> [extra binary args...]
```

Runs one repetition as above in a temporary directory, waits for the client and the
proxy, runs `budget_ratio` if a budget log was written, copies `stats.csv` (and
`ratio.csv`) to `<out_dir>`, and stops the server. Nothing is left running, also on
failure or Ctrl-C. On failure the exit status is non-zero, nothing is copied, and the
run directory with the binaries' logs is kept. The config must use the bare file
names `stats_file = "stats.csv"` and `budget_log_file = "budget_log.txt"`. Environment
overrides: `CLOAK_BIN_DIR`, `CLOAK_SERVER_PORT`/`CLOAK_PROXY_PORT` (4000/5050),
`CLOAK_START_TIMEOUT` (600 s), `CLOAK_CLIENT_TIMEOUT` (3600 s),
`CLOAK_PROXY_EXIT_TIMEOUT` (120 s), and `CLOAK_KEEP_RUN_DIR=1`.

## Configuration

All binaries take `--config <file.toml>`, and every key can be overridden on the
command line as `--<section>-<key with dashes>`, e.g. `--proxy-cache-size 1000` or
`--client-req-per-sec 50000`.

| Key | Meaning |
|---|---|
| `common.server_addr` / `proxy_addr` | Addresses the proxy uses to reach the server and the client uses to reach the proxy |
| `common.server_addr_bind` / `proxy_addr_bind` | Listen addresses of the server and the proxy |
| `common.object_size` | Object (value) size in bytes |
| `common.crypto_overhead` | Bytes added per object by AES-GCM encryption (16-byte IV + 16-byte tag); must be 32 |
| `common.storage_size` | Number of objects; workload keys must be in `0..storage_size`. The proxy rounds it up to the size covered by its budget schedule |
| `server.max_read_size` | Maximum number of objects per storage request; must be at least the (rounded-up) storage size, since the proxy initializes the whole storage in one request |
| `proxy.first_batch_size` | Budget of the first time-set (objects last accessed in the previous batch); time-set *i* gets budget ceil(`first_batch_size` / *i*). The batch size is the sum of the budgets |
| `proxy.queue_coefficient` | Back-pressure (Cloak mode only): when more than `queue_coefficient` x batch size distinct keys are pending, the proxy stops taking new requests until the next batch |
| `proxy.cache_size` | Number of objects in the proxy's cache of recently read or written objects |
| `proxy.num_threads` | Threads used to encrypt/decrypt each batch |
| `proxy.batch_interval_ms` | The proxy sends one batch to the server per interval |
| `proxy.is_unsafe` | `true` runs the non-oblivious baseline: same batching, cache and encryption, but only the requested objects are accessed (no dummy accesses, no reshuffling) |
| `proxy.enable_budget_log` / `budget_log_file` | Per-batch log of the budget used in each time-set, summarized by `budget_ratio` |
| `client.num_clients` | Number of client connections; must be 1 (the proxy serves a single connection) |
| `client.req_per_sec` | Request send rate (open loop); set it above the system's capacity to measure maximum throughput |
| `client.input_file` | Workload file (scripts pass `--client-input-file` instead) |
| `client.stats_file` | Summary CSV written at the end of the run |
| `client.enable_output_log` / `output_file` | Optional per-request log (client id, request id, type, key, latency, first bytes of the response) |

## Workloads and the generator

A workload file has one request per line: `key` (read) or `key value` (write; the
client writes an object filled with the byte `value % 256`).

`workload.txt` was generated with the synthetic workload generator (the one used for
the paper's synthetic Zipf workloads), then renamed from `s1.0`:

```bash
target/release/gen_workload --out-dir <dir> --num-keys 10000 --num-requests 30000 \
    --s-start 1.0 --s-end 1.0 --seed 1
```

The generator keeps all keys in a recency list; for every request it samples a rank
from a Zipf distribution with exponent `s`, queries the key at that position (the key
with that reuse distance) and moves it to the front. It writes one file `s<exponent>`
per exponent in `--s-start..=--s-end`. Without options it generates the paper's
Zipf-exponent sweep `s0.0` ... `s2.0` (1M keys, 1M requests, 50% writes); without
`--seed` every run produces a different, statistically equivalent workload. See
`gen_workload --help`.
