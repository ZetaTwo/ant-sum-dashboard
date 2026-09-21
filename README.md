# ANT+ Sum Dashboard

A live dashboard that sums the distance covered on multiple ANT+ training
devices (bikes, rowers, ski ergs) into one cumulative number, with a
per-device breakdown, streamed to a browser in real time.

- **Backend**: Rust. Reads ANT+ FE-C (Fitness Equipment) broadcasts from a
  USB ANT+ dongle, aggregates per-device distance in memory, and streams
  state to connected browsers over a WebSocket.
- **Frontend**: plain TypeScript, no UI framework, bundled with Vite.
- **Storage**: in-memory only. State does not survive a backend restart.

## How it works

- Devices are received via ANT+ continuous scan mode on a single channel,
  so multiple simultaneous devices (bike, rower, ski erg, ...) are picked up
  without pre-pairing — each broadcast carries its own originating device
  number.
- Each device's raw distance counter (a rolling 1-byte, 0-255m value) is
  unrolled into a monotonically increasing total for the life of the
  process.
- If no device reports *new* distance for `--inactivity-timeout-secs`
  (default 30), the dashboard's displayed totals reset to zero. This is a
  presentation-layer reset only — the real per-device totals in memory are
  never wiped, so the reset just changes what's currently shown, not what's
  tracked. Devices keep broadcasting on a fixed schedule even while idle, so
  activity is defined as distance actually increasing, not merely a message
  arriving.
- `fe_c.rs` parses ANT+ FE-C page 16 ("General FE Data") itself — the `ant`
  crate this project depends on has no built-in FE-C profile support.

## Prerequisites

- Rust (stable, 2024 edition)
- Node.js + [pnpm](https://pnpm.io/)
- `libusb-1.0-0-dev` — needed to build `rusb` (the USB backend), required
  even in `--simulate` mode since it's a compile-time dependency.
- To use real hardware: an ANT+ USB dongle. On WSL2, attach it from the
  Windows host with `usbipd attach --wsl`; if `lsusb` stops showing the
  dongle, the passthrough dropped and needs reattaching.
- To cross-compile a Windows binary (`make build-windows`): the
  `x86_64-pc-windows-gnu` rustup target and a mingw-w64 toolchain
  (`x86_64-w64-mingw32-gcc`). `rusb` vendors and builds libusb from source
  via mingw, so no separate Windows libusb install is needed.

## Windows hardware setup

Running against the real dongle on native Windows (not WSL2) needs one
manual driver step first. `rusb` (the USB backend) uses `libusb`, which on
Windows can only open a device through a generic passthrough driver like
WinUSB — not through the vendor driver Windows assigns by default, which
only exposes a narrow, vendor-defined API. Symptom if this hasn't been done:

```
WARN ant_sum_dashboard::ant_source::usb: ANT+ USB source failed, retrying err=failed to open ANT+ USB driver: FailedToOpenDevice(Access) retry_in_secs=5
```

Fix, one-time per machine:

1. Install [Zadig](https://zadig.akeo.ie/).
2. Options → List All Devices, then select the ANT+ USB stick (VID `0FCF`,
   PID `1009` for the ANTUSB-m).
3. Set the target driver to **WinUSB** (not libusb-win32 — that backend is
   known to be flakier with `libusb-1.0`, which is what this project uses)
   and click "Replace Driver".
4. Unplug and replug the dongle so the new driver binding takes effect.

This rebinds only the selected device, not USB globally. If the error
persists after this, check for another process holding the dongle open
(Garmin ANT+ Agent/USB service, Garmin Express, Zwift, TrainerRoad, or a
previous instance of this app) and close it before retrying.

## Usage

CLI flags (all backend, also settable via env var — see `--help`):

| Flag | Env var | Default | Meaning |
|---|---|---|---|
| `--port` | `PORT` | `8080` | HTTP/WebSocket port |
| `--simulate` | `SIMULATE` | off | Use synthetic data instead of real hardware |
| `--inactivity-timeout-secs` | `INACTIVITY_TIMEOUT_SECS` | `30` | Idle time before the displayed total resets |

### Dev

With a `Makefile` shortcut:

```sh
make install   # frontend deps, once
make dev       # backend (--simulate) + Vite dev server together, Ctrl-C stops both
```

Or by hand:

```sh
# terminal 1
cd backend && cargo run -- --simulate --port 8080

# terminal 2
cd frontend && pnpm install && pnpm run dev
```

Open the URL Vite prints (default `http://localhost:5173`) — its dev server
proxies `/ws` through to the backend.

To run against real hardware instead of the simulator: `make dev-hardware`,
or `cargo run -- --port 8080` in `backend/`.

### Testing

```sh
make test
```

Runs the backend's Rust unit tests (`cargo test`) and the frontend
typecheck (`tsc --noEmit`).

### Production

```sh
make build
make run
```

`make build` produces a single self-contained release binary: `ws.rs` uses
`rust-embed` (via `axum-embed`) on `frontend/dist`, and in release builds
this bakes the files into the executable at compile time, so nothing
needs to ship alongside it — no `frontend/dist` folder required at
runtime. In debug builds (`cargo run`, `make dev`/`dev-hardware`) the same
code instead reads those files from disk on every request, so frontend
changes show up without a backend rebuild. Since embedding happens at
backend compile time, the frontend must already be built first — `make
build` depends on `build-frontend` to guarantee that ordering. `make
build-windows` cross-compiles the same self-contained binary for Windows
(see Prerequisites). `make run` serves the embedded frontend and `/ws`
from a single process.

## ANT+ network key

Real device reception requires the ANT+ Managed Network key, which Garmin
formally gates behind their ANT+ Adopter program registration. This value
is hardcoded in `ant_source/usb.rs` (`ANT_PLUS_NETWORK_KEY`) rather than
kept in a config file: the exact byte value is openly published as an
ordinary constant in mainstream open-source ANT+ projects (openant,
GoldenCheetah — see the citation on the constant), so there's nothing
secret to keep out of version control. Whether using it without going
through Garmin's own Adopter registration complies with their license terms
is a separate question from the technical one this project answers.

## Dependency notes

- Real ANT+ radio access uses the `ant` crate from
  [`cujomalainey/ant-rs`](https://github.com/cujomalainey/ant-rs)'s
  `development` branch via a git dependency pinned to a specific commit.
  The versions published on crates.io under `ant`/`ant-usb`/`ant-plus` are
  stale 2017 placeholders and must not be used.
