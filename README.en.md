# Delta Bypass for Rust

English · [简体中文](README.md)

A high-performance automated verification bypass service for Platoboost written in Rust. Automatically solves CAPTCHA image challenges and processes verification checkpoints to extract access keys, providing both a Command Line Interface (CLI) and an HTTP REST API.

---

## Overview

Delta Bypass for Rust automates the entire Platoboost verification workflow. Given a target authorization URL or credential string, the service automatically solves image CAPTCHAs, navigates sequential checkpoints, and retrieves the final access key.

Developed by **Hasl_Team**, this project is a high-performance Rust rewrite of Hasl_Team's original Python implementation ([Delta-bypass Source Repository](https://github.com/AbabaHnb/Delta-bypass)). While maintaining **100% pixel-perfect accuracy alignment** with the original solving algorithms, it significantly improves execution speed and concurrent throughput.

## System Requirements & Prerequisites

* **Zero Language Runtimes Required**: No Python, Node.js, .NET, or interpreter installation required.
* **No External Shared Library Dependencies**: OpenSSL is not required (cryptography routines are written in pure Rust and statically linked).
* **Network & Storage Requirements**:
  * Internet access is required to reach `captcha.platorelay.com` and `auth.platorelay.com`.
  * The working directory must be **writable**. The service automatically creates and maintains `.key_cache.json` (deleting this file is safe; it automatically rebuilds).

---

## Building and Installation

Rust 1.75 or higher is required.

### Standard Build from Source

```bash
# Clone repository and build release binary
cargo build --release
```

The output binary is placed at `target/release/delta-bypass` (`delta-bypass.exe` on Windows). The single binary can be copied and deployed independently.

### Automated Cloud Build via GitHub Actions (Recommended)

The project includes an automated GitHub Actions CI/CD workflow ([.github/workflows/build.yml](file:///.github/workflows/build.yml)).
- **On Push / PR**: Binaries are automatically built for Windows (x64), Linux (GNU), and Linux (Musl static). Artifacts can be downloaded directly from the **Actions** tab on GitHub.
- **On Version Tag Push**: Pushing a version tag (e.g., `git tag v1.0.0 && git push origin v1.0.0`) automatically publishes a GitHub Release with pre-compiled binaries attached.

### Linux C Toolchain Prerequisites

If a C compiler is missing on your Linux distribution, install standard build utilities:

```bash
# Debian / Ubuntu
sudo apt install build-essential

# CentOS / RHEL / Rocky Linux
sudo dnf groupinstall "Development Tools"

# Alpine Linux
apk add build-base
```

### Static Binary Compilation with Musl (Linux)

To prevent `GLIBC_2.xx not found` errors on older Linux distributions, build a fully static binary independent of system `glibc`:

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt install musl-tools          # Debian/Ubuntu (or musl-gcc on other distros)
cargo build --release --target x86_64-unknown-linux-musl
```

The resulting binary at `target/x86_64-unknown-linux-musl/release/delta-bypass` depends on no shared libraries and runs seamlessly across any Linux distribution (e.g., CentOS 7, Alpine, Arch Linux).

---

## Performance Benchmark

Measured on an 8-core CPU system running Debian Linux:

| Benchmark Metric | Python Original | Rust Rewrite | Improvement / Comparison |
|---|---|---|---|
| End-to-End Single Bypass | ~6.8 seconds | **~5.5 seconds** | ~20% overall speedup |
| `coherence` CAPTCHA Solving | 72 ms | **28 ms** | 2.57x computation speedup |
| `driftodd` CAPTCHA Solving | 227 ms | **38 ms** | 5.97x computation speedup |
| Algorithmic Accuracy | Baseline | **24/24 Identical** | Max coordinate offset: 0.000 px |

> **Note**: Out of the ~5.5-second total elapsed time, approximately 5.0 seconds consist of mandatory upstream checkpoint cooldown intervals (enforced server delays). Local CPU processing and algorithms take only ~0.5 seconds.

---

## Command Line Interface (CLI) Guide

### Usage Examples

> **Warning (Command Line Quotes)**: Authorization URLs contain special characters like `&`. Always enclose URLs in **double quotes** on Windows CMD/PowerShell and Linux Shells to avoid parameter truncation.

```bash
# Bypass a specific Platoboost URL
delta-bypass "https://auth.platorelay.com/a?d=<pass_credential>"

# Pass a credential string directly or specify a file containing credentials
delta-bypass "<pass_credential>"
delta-bypass tickets.txt

# Generate 3 test URLs and automatically execute bypass
delta-bypass --generate 3

# Generate test URLs only (without executing bypass)
delta-bypass --generate 5 --no-auto

# Start the HTTP REST API server
delta-bypass --serve --port 2233
```

### Windows Terminal Encoding Setup

Legacy Windows Command Prompt (`cmd.exe`) defaults to non-UTF-8 encodings, which may render Chinese log characters as corrupted symbols.
1. **Temporary UTF-8 Session**: Run `chcp 65001` in CMD before executing the binary.
2. **Recommended Environment**: Use **Windows Terminal** or **PowerShell 7+**, which natively support UTF-8 encoding.

### CLI Options Reference

| Option | Default | Description |
|---|---|---|
| `<link>` | — | Target URL, credential string, or file path containing credentials |
| `--generate N` / `-g` | 0 | Generate N test verification URLs |
| `--quiet` / `-q` | Off | Silent mode: output final key only and suppress progress logs |
| `--max-rounds N` | 3 | Maximum round count (automatically adjusted to server checkpoint count) |
| `--no-auto` | Off | Used with `--generate` to generate URLs without running bypass |
| `--serve` | Off | Run in HTTP API server mode |
| `--host` | 0.0.0.0 | Server listen address (use `127.0.0.1` for local access only) |
| `--port` / `-p` | 2233 | Server listen port |
| `--prepared N` | 30 | Puzzle store pre-fill capacity (set to 0 to disable) |
| `--img <FILE> --img-type <KIND>` | — | Debug: test local image solving (KIND: `driftodd` or `coherence`) |
| `--bench N` | 1 | Debug: repeatedly evaluate a single image N times for benchmarking |
| `--pool-stats` | Off | Debug: display real-time puzzle store statistics only |
| `--pool-watch-secs N` | 60 | Debug: duration in seconds to monitor the puzzle store |

---

## HTTP REST API Specification

### Starting the Server

```bash
delta-bypass --serve --port 2233 --prepared 30
```

### Request Example

```bash
curl -G http://127.0.0.1:2233/delta \
     --data-urlencode "url=https://auth.platorelay.com/a?d=<pass_credential>"
```

### Response Example

```json
{
  "key": "FREE_xxxxxxxx",
  "cached": false,
  "error": null,
  "made_by": "Hasl_Team",
  "qq_group": "277707901",
  "times": "5.512340000000s"
}
```

### Response Schema

| Field | Type | Description |
|---|---|---|
| `key` | String \| null | Retrieved access key (`null` on failure) |
| `cached` | Boolean | `true` indicates a hit in the 24-hour result cache |
| `error` | String \| null | Error message (`null` on success) |
| `times` | String | Actual bypass execution time (returns original duration on cache hit) |

### Error Code Dictionary

| Error Message | Cause & Resolution |
|---|---|
| `链接格式无效 / Malformed link` | Invalid URL format or failure to extract pass credentials |
| `链接无效 / Invalid link: <Server Message>` | Credential is invalid or expired (rejected by upstream, non-retryable) |
| `绕过失败 / Bypass failed` | Key extraction failed after two attempts (includes one retry) |
| `内部执行异常 / Internal execution error` | Unexpected internal runtime exception |
| `未获得结果 / No result returned` | Channel closed unexpectedly during execution |

---

## Performance Optimizations

The application incorporates three key architectural optimizations to minimize latency:

1. **Pre-solved CAPTCHA Store (saves ~1.1s)**  
   Empirical testing confirmed that the upstream CAPTCHA service is shared globally and independent of specific link sessions. The service asynchronously fetches CAPTCHAs, downloads 45KB images, and calculates answers in the background to build a credential store. When a client request arrives, only a single ~180ms credential exchange is performed. Credentials strictly adhere to one-time usage and are **never reused or cached long-term**.

2. **Send-Time Cooldown Tracking (saves ~0.2s)**  
   Upstream evaluates the 5-second checkpoint cooldown based on server request arrival timestamps. Tracking is calculated from client **request dispatch** rather than response receipt, saving one full Network Round-Trip Time (RTT). An adaptive self-tuning margin (60ms to 1500ms) absorbs network jitter while preventing rate limits.

3. **HTTP Connection Pooling (saves ~0.6s)**  
   Persistent TCP/TLS connections are maintained with upstream servers. Fetching a 45KB image over an established connection requires ~25ms compared to ~660ms for new TLS handshakes.

---

## Puzzle Store & Rate Limiting Mechanics

The puzzle store is bound by **request rate constraints** rather than server memory capacity.

Maintaining 30 ready puzzles with a 30-second TTL mathematically requires a replenishment rate of **1 puzzle per second** (equivalent to 2 HTTP requests per second continuously).

To prevent upstream rate limiting, strict concurrency controls are enforced:
* **Global Single Ticket Window**: All background threads queue sequentially for token acquisition.
* **In-Flight Concurrency Limit**: Maximum of 2 CAPTCHAs being solved concurrently worldwide.
* **Global Circuit Breaker**: Any single rate limit or refusal pauses all replenishment threads (starting at 5s, exponentially backing off to 60s, halving upon success).

Under typical operation, the store maintains a stable 30/30 level with zero rate limits, cold-starting in ~30 seconds while continuing to serve inbound API requests.

> **Tuning Guidance**: To expand capacity, increase `POOL_MAX_AGE` in `src/config.rs` (server TTL supports up to 60s). Avoid increasing target capacity alone, as the rate limiter will clamp replenishment speed below target levels.

---

## Algorithm Accuracy Alignment

The Rust implementation guarantees pixel-level equivalence with the original Python solvers. To preserve precision, three critical implementation details are strictly maintained:

1. **Exact Floating-Point Division for Grayscale**  
   Grayscale calculation must explicitly divide by `3.0` rather than multiplying by `(1.0 / 3.0)`. For RGB `(170, 170, 170)`, exact division yields `170.0`, whereas `510 * (1/3)` produces `169.99998`. Because the threshold for dark pixel classification is set at exactly `170`, floating-point variance flips pixel binary states, altering connected components and centroid locations.

2. **Full GIF Canvas Compositing**  
   To optimize bandwidth, GIF frames frequently store only delta rectangular regions. The decoder maintains a full-frame canvas and compositing rules (handling transparency and disposal modes). Reading partial frame patches directly results in corrupted black frames following the first frame.

3. **Deterministic Tie-Breaking Strategy**  
   Rust's native `max_by` returns the last occurrence on equal values, whereas Python returns the first. To prevent coordinate drift on ties, custom `pick_max` and `pick_min` functions are explicitly implemented.

4. **Integer Grid Division**  
   All grid dimension calculations utilize integer division to match Python NumPy's `//` operator behavior.

---

## Production Deployment & Service Management

### 1. Windows Service Setup

#### Option A: Windows Task Scheduler (Native)
1. Open Task Scheduler and select "Create Task".
2. **General**: Select "Run whether user is logged on or not" and "Run with highest privileges".
3. **Triggers**: Add a new trigger set to "At startup".
4. **Actions**: Add a new action. Program: absolute path to `delta-bypass.exe`. Arguments: `--serve --host 127.0.0.1 --port 2233 --prepared 30`. **Set "Start in" to the directory containing the executable** (omitting this causes working directory redirection to `C:\Windows\System32`).
5. **Settings**: Check "If the task fails, restart every...".

#### Option B: NSSM Service Manager (Recommended with Auto-Restart & Logs)
Download NSSM from [nssm.cc](https://nssm.cc), then execute in an Administrator Command Prompt:

```cmd
nssm install DeltaBypass C:\delta-bypass\delta-bypass.exe
nssm set DeltaBypass AppParameters "--serve --host 127.0.0.1 --port 2233 --prepared 30"
nssm set DeltaBypass AppDirectory C:\delta-bypass
nssm set DeltaBypass AppStdout C:\delta-bypass\out.log
nssm set DeltaBypass AppStderr C:\delta-bypass\err.log
nssm start DeltaBypass
```

### 2. Linux Systemd Service

Deploy using the included service configuration at `deploy/delta-bypass.service`:

```bash
# 1. Create installation directory and copy binary
sudo mkdir -p /opt/delta-bypass
sudo cp target/release/delta-bypass /opt/delta-bypass/
sudo chown -R www:www /opt/delta-bypass

# 2. Install and activate Systemd service
sudo cp deploy/delta-bypass.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now delta-bypass

# 3. Check service status and live logs
systemctl status delta-bypass
journalctl -u delta-bypass -f
```

*Create the non-login system user `www` if it does not exist*:
```bash
sudo useradd -r -s /usr/sbin/nologin www
```

### 3. Docker Container Deployment

#### Multi-Stage Dockerfile

```dockerfile
# Build stage
FROM rust:1-alpine AS builder
RUN apk add --no-cache build-base
WORKDIR /build
COPY . .
RUN cargo build --release

# Runtime stage
FROM alpine:latest
RUN apk add --no-cache ca-certificates && \
    adduser -D -H app
WORKDIR /app
COPY --from=builder /build/target/release/delta-bypass /app/
RUN chown -R app:app /app
USER app
EXPOSE 2233
CMD ["/app/delta-bypass", "--serve", "--host", "0.0.0.0", "--port", "2233", "--prepared", "30"]
```

#### Build & Run Commands

```bash
docker build -t delta-bypass .

# Run container with volume mount for key cache persistence
docker run -d --name delta-bypass \
  -p 127.0.0.1:2233:2233 \
  -v delta-keys:/app \
  --restart unless-stopped \
  delta-bypass
```

> **Notes**:
> 1. Container image must include `ca-certificates` for outbound HTTPS connections.
> 2. Set `--host 0.0.0.0` inside the container; constrain public exposure via host port binding `-p 127.0.0.1:2233:2233`.

### 4. Nginx Reverse Proxy & Security Hardening

**Security Warning**: The HTTP API features no authentication mechanisms by default. Anyone with network access to the port can consume bypass capacity. **Do not expose the API unauthenticated to the public Internet.**

Configure Nginx authentication and rate limiting:

```nginx
limit_req_zone $binary_remote_addr zone=delta:10m rate=10r/s;

location /delta {
    limit_req zone=delta burst=20 nodelay;
    auth_basic "Delta Bypass for Rust API Authorization";
    auth_basic_user_file /etc/nginx/.htpasswd;
    proxy_pass http://127.0.0.1:2233;
    proxy_read_timeout 120s;
}
```

---

## Directory Structure

```
src/
├── main.rs              CLI entry point, argument parsing, and command execution
├── lib.rs               Library entry point and module declarations
├── config.rs            Global system configuration parameters and constants
├── api.rs               HTTP REST API server, key caching, and request deduplication
├── chain.rs             Core bypass workflow (puzzle -> submit -> checkpoint -> key)
├── pool.rs              Asynchronous puzzle store and replenishment queue
├── auth.rs              Authentication server communication protocols
├── crypto.rs            Upstream custom encryption routines
├── net.rs               HTTP connection pooling and socket reuse
├── useragent.rs         User-Agent spoofing and mobile browser fingerprinting
├── link.rs              Test URL generator
├── timing.rs            High-precision benchmark and step diagnostics logger
├── image/               Image processing algorithms
│   ├── mod.rs           GIF decoding, grayscale conversion, and circle fitting
│   ├── patches.rs       Dark pixel connected component segmentation
│   └── nearest.rs       Fast nearest-neighbor search algorithms
└── solver/              CAPTCHA solvers
    ├── mod.rs           Solver dispatch interface
    ├── driftodd.rs      Reverse rotation CAPTCHA solver
    ├── coherence.rs     Static patch / coherence CAPTCHA solver
    └── tracking.rs      Motion trajectory tracking algorithm
```

---

## Configuration Parameters

All tunable constants are declared in `src/config.rs`. Key options include:

| Parameter | Default | Notes & Constraints |
|---|---|---|
| `MIN_STEP_GAP` | 5s | Upstream hard cooldown interval; **do not lower** |
| `GAP_MARGIN_START` | 250ms | Initial delay margin for adaptive tuning |
| `POOL_MAX_AGE` | 30s | Puzzle store TTL (must remain below server's 60s limit) |
| `POOL_MIN_SLOT_INTERVAL` | 950ms | Minimum replenishment slot interval; **do not lower** |
| `POOL_MAX_INFLIGHT` | 2 | Maximum concurrent puzzles in flight; higher values risk rate limits |
| `POLL_MAX_ATTEMPTS` | 10 | Maximum key polling attempts |
| `MAX_ROUNDS_HARD_CAP` | 12 | Maximum round ceiling to prevent infinite loops |

---

## Troubleshooting Guide

| Symptom | Root Cause | Solution |
|---|---|---|
| `链接无效 / Invalid link` | Expired credential or upstream rejection | Generate a new Platoboost link (upstream rejection behavior) |
| `绕过失败 / Bypass failed` | Workflow failed at a specific stage | Remove `--quiet` and check the `终止于:` line in logs |
| Frequent rate limiting logs | Cooldown interval too short | Verify `MIN_STEP_GAP` setting; increase interval if upstream rules tightened |
| Puzzle store will not replenish | Rate limited by upstream | Run `--pool-stats`; non-zero values in `被拒` indicate rate limits |
| High-concurrency connection failures | File descriptor limits (Linux) | Run `ulimit -n 65535` or add `LimitNOFILE=65535` in Systemd unit |
| Windows CMD Chinese garbled | Terminal encoding is not UTF-8 | Run `chcp 65001` or switch to Windows Terminal / PowerShell 7 |
| Linux `GLIBC_2.xx not found` | Build GLIBC version higher than target system | Build static target `x86_64-unknown-linux-musl` |
| Linux `Permission denied` | Binary lacks execution permissions | Run `chmod +x delta-bypass` |
| Key cache file missing | Working directory misconfigured | Check "Start in" in Task Scheduler or `WorkingDirectory` in Systemd |
| Docker TLS Certificate Error | Missing root certificates | Install `ca-certificates` in runtime container stage |
| Port binding conflict | Target port bound by another process | Change `--port` or identify and terminate conflicting process |

---

## License

Distributed under the [MIT License](LICENSE).
