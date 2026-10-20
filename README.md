# volt-plane

Modular Rust/C++ hardware orchestration server and HAL for Keithley SMUs using Tokio actor patterns, CXX FFI, and structured telemetry.

The web console is the Rust operator interface. It talks to an in-process bench that models GPIB, VISA, CAN, Ethernet, USB, and a USB GPIO fixture. That bench is the seam the C++ HAL will replace; the browser never opens a hardware session itself.

The server binds to `127.0.0.1` only.

## Run

Install Rust, then from this directory:

```powershell
cargo run
```

Open <http://127.0.0.1:4173>. Set `PORT` to use another port.

```powershell
cargo test
```

## Console

- **Bench** — instrument list, live voltage/current/resistance, source and compliance, output enable, SCPI session, bench log.
- **Buses** — GPIB, VISA, CAN, Ethernet, and USB resource lists. Scan walks the simulated bus.
- **Sequences** — power-rail check on the Keithley 2450, continuity on the Keysight 34461A, and a GPIO fixture power cycle. Abort still runs cleanup steps.

Simulated loads: 2450 channel A is 100 Ω, 2636B channels are 220 Ω and 1 kΩ, the supply is 10 Ω. `All outputs off` drops every source.

SCPI understood by the simulated SMUs, DMM, and supply: `*IDN?`, `*RST`, `*CLS`, `SYST:ERR?`, `OUTP ON|OFF`, `SOUR:VOLT`, `SOUR:VOLT:ILIM`, `SOUR:CURR`, `SOUR:CURR:VLIM`, `MEAS:VOLT?`, `MEAS:CURR?`, `MEAS:RES?`, plus an `A:` / `B:` channel prefix.

## Layout

| Path | Role |
|---|---|
| `src/bench.rs` | Simulated instruments, buses, sequences, and the HAL seam |
| `src/api.rs` | JSON and WebSocket API |
| `src/main.rs` | Local Axum server |
| `web/` | Operator console served by the binary |
