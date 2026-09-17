# ANT+ Sum Dashboard

Sums the distance covered on multiple ANT+ training devices (bikes, rowers,
ski ergs) into one cumulative number, broken down per device, streamed live
to a browser over a WebSocket. Rust backend, plain TypeScript frontend
(Vite, no UI framework). In-memory only — state does not survive a restart.

See [the implementation plan](/home/zetatwo/.claude/plans/we-are-going-to-harmonic-pony.md)
for the full design and open risks.

## Status

- Backend pipeline (`--simulate` mode), session reset logic, and the
  WebSocket stream work end-to-end.
- Frontend renders the total + per-device breakdown against the simulator.
- Real ANT+ USB hardware support (`ant_source/usb.rs`) is wired up and
  validated against the attached ANTUSB-m stick: USB open, the full
  reset/capabilities handshake, channel configuration, and continuous scan
  mode (`OpenRxScanMode`) all work. It uses the `ant` crate (git dependency,
  `cujomalainey/ant-rs`, pinned to commit `4426dc64b060ad511931644cb419ebd7cb6b8769`
  on the `development` branch — crates.io's `ant`/`ant-usb`/`ant-plus` are
  stale 2017 placeholders and must not be used). `ant-rs` has no FE-C
  profile, so `ant_source/usb.rs` decodes broadcast payloads with the same
  `fe_c::parse_page16` used in tests.
- The ANT+ Managed Network key is hardcoded in `ant_source/usb.rs`
  (`ANT_PLUS_NETWORK_KEY`). It's formally gated behind Garmin's ANT+ Adopter
  registration, but this exact value is openly published as an ordinary
  constant in mainstream open-source ANT+ projects (openant, GoldenCheetah —
  see the comment on the constant for exact citations), so it's committed
  directly rather than kept in a git-ignored config file. Verified against
  the attached hardware: radio initializes and scan mode opens with no
  errors. Actual device reception (e.g. a PM5) hasn't been confirmed yet —
  needs a real ANT+ device powered on and transmitting nearby to test.

## Prerequisites

- Rust (stable, 2024 edition)
- Node.js + npm
- `libusb-1.0-0-dev` (installed) — needed to build `rusb`.
- On WSL2: the ANT+ USB dongle attached via `usbipd attach --wsl` from the
  Windows host. If `lsusb` stops showing `ID 0fcf:1009 Dynastream
  Innovations, Inc. ANTUSB-m Stick`, the passthrough dropped — reattach it.

## Dev (no hardware — primary path)

```sh
# terminal 1
cd backend && cargo run -- --simulate --port 8080

# terminal 2
cd frontend && npm install && npm run dev
```

## Dev (real hardware)

```sh
cd backend && cargo run -- --port 8080
```

Open the URL Vite prints (default `http://localhost:5173`). The dev server
proxies `/ws` to the backend on port 8080.

Useful flags: `--inactivity-timeout-secs <N>` (default 30) controls how long
the dashboard waits with no data from any device before the displayed totals
reset to zero (the real per-device totals are never wiped in memory — see
the plan's "Reset-after-inactivity semantics").

## Testing

```sh
cd backend && cargo test
```

Covers FE-C distance-rollover accumulation and the session baseline/reset
logic — the two trickiest pieces of pure logic in the backend.

## Production build

```sh
cd frontend && npm run build
cd backend && cargo build --release
./target/release/ant-sum-dashboard --port 8080 --static-dir ../frontend/dist
```

Serves the built frontend and the `/ws` endpoint from a single binary.
