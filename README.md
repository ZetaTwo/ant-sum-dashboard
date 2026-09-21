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
- To cross-compile a Windows binary (`make build-backend-windows`): the
  `x86_64-pc-windows-gnu` rustup target and a mingw-w64 toolchain
  (`x86_64-w64-mingw32-gcc`). `rusb` vendors and builds libusb from source
  via mingw, so no separate Windows libusb install is needed.

## Usage

CLI flags (all backend, also settable via env var — see `--help`):

| Flag | Env var | Default | Meaning |
|---|---|---|---|
| `--port` | `PORT` | `8080` | HTTP/WebSocket port |
| `--simulate` | `SIMULATE` | off | Use synthetic data instead of real hardware |
| `--inactivity-timeout-secs` | `INACTIVITY_TIMEOUT_SECS` | `30` | Idle time before the displayed total resets |
| `--static-dir` | `STATIC_DIR` | `../frontend/dist` | Where to serve the built frontend from |

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

`make build` produces the release backend binary and the built frontend
assets; `make run` serves both from a single process (backend serves
`/ws` and the static frontend together — no separate frontend server in
production).

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
