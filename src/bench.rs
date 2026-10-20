//! In-process bench used by the web console.
//!
//! `Bench` is the seam the C++ HAL (GPIB, VISA, CAN, Ethernet, USB, GPIO)
//! will replace. The HTTP layer never talks to hardware directly.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::{broadcast, Mutex};

const HISTORY: usize = 60;

#[derive(Clone)]
pub struct AppState {
    bench: std::sync::Arc<Mutex<Bench>>,
    tx: broadcast::Sender<String>,
}

impl AppState {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(64);
        Self {
            bench: std::sync::Arc::new(Mutex::new(Bench::demo())),
            tx,
        }
    }

    pub fn spawn_ticker(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(250));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                state.commit(|bench| bench.tick()).await;
            }
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    pub async fn snapshot(&self) -> Snapshot {
        self.bench.lock().await.snapshot()
    }

    pub async fn with_bench<R>(&self, f: impl FnOnce(&Bench) -> R) -> R {
        f(&*self.bench.lock().await)
    }

    pub async fn commit<R>(&self, f: impl FnOnce(&mut Bench) -> R) -> R {
        let (result, json) = {
            let mut bench = self.bench.lock().await;
            let result = f(&mut bench);
            bench.rev = bench.rev.wrapping_add(1);
            let json = serde_json::to_string(&bench.snapshot()).unwrap_or_else(|_| "{}".into());
            (result, json)
        };
        let _ = self.tx.send(json);
        result
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub rev: u64,
    pub now_ms: u64,
    pub station: Station,
    pub buses: Vec<Bus>,
    pub instruments: Vec<Instrument>,
    pub log: Vec<LogLine>,
    pub sequences: Vec<SequenceInfo>,
    pub run: RunState,
}

#[derive(Debug, Clone, Serialize)]
pub struct Station {
    pub name: String,
    pub station_id: String,
    pub operator_id: String,
    pub simulation: bool,
    pub hal: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bus {
    pub id: String,
    pub name: String,
    pub state: String,
    pub detail: String,
    pub resources: Vec<String>,
    pub last_scan_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Instrument {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub role: String,
    pub kind: String,
    pub bus: String,
    pub resource: String,
    pub visa: bool,
    pub connected: bool,
    pub output_on: bool,
    pub rtt_ms: f64,
    pub last_error: Option<String>,
    pub max_volts: f64,
    pub max_amps: f64,
    pub channels: Vec<Channel>,
    pub lines: Vec<GpioLine>,
    pub frames: Vec<CanFrame>,
    pub bus_load: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Channel {
    pub name: String,
    pub source_mode: String,
    pub source_level: f64,
    pub compliance: f64,
    pub output_on: bool,
    pub voltage: f64,
    pub current: f64,
    pub resistance: Option<f64>,
    pub limited: bool,
    pub load_ohms: f64,
    pub history_v: Vec<f64>,
    pub history_i: Vec<f64>,
    #[serde(skip)]
    default_compliance: f64,
    #[serde(skip)]
    limit_logged: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpioLine {
    pub name: String,
    pub on: bool,
    pub direction: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanFrame {
    pub id: u32,
    pub dlc: u8,
    pub data: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub ts_ms: u64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SequenceInfo {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunState {
    pub sequence_id: String,
    pub status: String,
    pub step: Option<usize>,
    pub outcomes: Vec<String>,
    pub detail: String,
}

struct Sequence {
    id: String,
    name: String,
    summary: String,
    steps: Vec<StepDef>,
}

struct StepDef {
    name: String,
    cleanup: bool,
    action: Action,
}

enum Action {
    Connect(String),
    Source {
        id: String,
        channel: String,
        mode: String,
        level: f64,
        compliance: f64,
    },
    Output {
        id: String,
        channel: String,
        on: bool,
    },
    AssertCurrent {
        id: String,
        channel: String,
        lo: f64,
        hi: f64,
    },
    AssertResistance {
        id: String,
        lo: f64,
        hi: f64,
    },
    Gpio {
        id: String,
        line: String,
        on: bool,
    },
    AssertGpio {
        id: String,
        line: String,
        on: bool,
    },
    Settle(u64),
}

pub struct Bench {
    rev: u64,
    seed: u32,
    frame_seq: u32,
    abort: bool,
    station: Station,
    buses: Vec<Bus>,
    instruments: Vec<Instrument>,
    log: VecDeque<LogLine>,
    run: RunState,
}

impl Bench {
    fn demo() -> Self {
        let mut bench = Self {
            rev: 0,
            seed: 0x00c0_ffee,
            frame_seq: 0,
            abort: false,
            station: Station {
                name: "Volt Plane".into(),
                station_id: "BENCH-01".into(),
                operator_id: "ENG".into(),
                simulation: true,
                hal: "In-process simulation. GPIB, VISA, CAN, Ethernet, USB, and GPIO are modeled until the C++ HAL is attached.".into(),
            },
            buses: vec![
                bus("gpib", "GPIB", &["GPIB0::24::INSTR", "GPIB0::16::INSTR"]),
                bus(
                    "visa",
                    "VISA",
                    &[
                        "GPIB0::24::INSTR",
                        "TCPIP0::192.168.1.50::inst0::INSTR",
                        "USB0::0x2A8D::0x1601::MY59001234::INSTR",
                        "TCPIP0::192.168.1.61::inst0::INSTR",
                    ],
                ),
                bus("can", "CAN", &["can0:0x21 ECU", "can0:0x30 SENSOR"]),
                bus(
                    "lan",
                    "Ethernet",
                    &[
                        "TCPIP0::192.168.1.50::inst0::INSTR",
                        "TCPIP0::192.168.1.61::inst0::INSTR",
                    ],
                ),
                bus(
                    "usb",
                    "USB",
                    &[
                        "USB0::0x2A8D::0x1601::MY59001234::INSTR",
                        "USB0::0x09DB::0x0001::GPIO01",
                    ],
                ),
            ],
            instruments: vec![
                instrument(
                    "smu-2450",
                    "SMU 2450",
                    "Keithley Instruments",
                    "2450",
                    "04501234",
                    "1.7.12b",
                    "Source measure unit",
                    "smu",
                    "gpib",
                    "GPIB0::24::INSTR",
                    true,
                    200.0,
                    1.0,
                    vec![channel("A", 100.0, 0.1)],
                    vec![],
                ),
                instrument(
                    "smu-2636",
                    "SMU 2636B",
                    "Keithley Instruments",
                    "2636B",
                    "44509821",
                    "3.4.2",
                    "Dual-channel source measure unit",
                    "smu",
                    "lan",
                    "TCPIP0::192.168.1.50::inst0::INSTR",
                    true,
                    200.0,
                    1.5,
                    vec![channel("A", 220.0, 0.1), channel("B", 1000.0, 0.1)],
                    vec![],
                ),
                instrument(
                    "dmm-34461",
                    "DMM 34461A",
                    "Keysight Technologies",
                    "34461A",
                    "MY59001234",
                    "A.03.10",
                    "Digital multimeter",
                    "dmm",
                    "usb",
                    "USB0::0x2A8D::0x1601::MY59001234::INSTR",
                    true,
                    0.0,
                    0.0,
                    vec![channel("MEAS", 0.0, 0.0)],
                    vec![],
                ),
                instrument(
                    "psu-e36312",
                    "PSU E36312A",
                    "Keysight Technologies",
                    "E36312A",
                    "MY61004521",
                    "2.05",
                    "DC power supply",
                    "psu",
                    "lan",
                    "TCPIP0::192.168.1.61::inst0::INSTR",
                    true,
                    25.0,
                    2.0,
                    vec![channel("OUT1", 10.0, 1.0)],
                    vec![],
                ),
                instrument(
                    "can-ecu",
                    "Fixture ECU",
                    "Volt Plane",
                    "ECU-21",
                    "CAN21-0007",
                    "0.9.1",
                    "CAN node",
                    "can",
                    "can",
                    "can0:0x21",
                    false,
                    0.0,
                    0.0,
                    vec![],
                    vec![],
                ),
                instrument(
                    "gpio-fixture",
                    "GPIO fixture",
                    "Volt Plane",
                    "GPIO-8",
                    "GPIO01",
                    "1.0.0",
                    "USB GPIO fixture",
                    "gpio",
                    "usb",
                    "USB0::0x09DB::0x0001::GPIO01",
                    false,
                    0.0,
                    0.0,
                    vec![],
                    vec![
                        line("INTERLOCK", false),
                        line("DUT_PWR", false),
                        line("RESET_N", true),
                        line("SCOPE_TRIG", false),
                    ],
                ),
            ],
            log: VecDeque::new(),
            run: idle_run(),
        };
        bench.push_log("info", "bench ready — simulation".into());
        bench.refresh_buses();
        bench
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            rev: self.rev,
            now_ms: now_ms(),
            station: self.station.clone(),
            buses: self.buses.clone(),
            instruments: self.instruments.clone(),
            log: self.log.iter().cloned().collect(),
            sequences: describe(),
            run: self.run.clone(),
        }
    }

    pub fn tick(&mut self) {
        let mut seed = self.seed;
        let mut frame_seq = self.frame_seq;
        let mut notes = Vec::new();

        for inst in &mut self.instruments {
            if inst.connected {
                let base = match inst.bus.as_str() {
                    "lan" => 0.85,
                    "usb" => 0.28,
                    "gpib" => 1.45,
                    "can" => 0.42,
                    _ => 0.6,
                };
                inst.rtt_ms = (base + noise(&mut seed) * 0.12).max(0.05);
            } else {
                inst.rtt_ms = 0.0;
                continue;
            }

            match inst.kind.as_str() {
                "smu" | "psu" => {
                    let inst_id = inst.id.clone();
                    for ch in &mut inst.channels {
                        update_sourced(ch, noise(&mut seed));
                        if ch.limited && !ch.limit_logged {
                            ch.limit_logged = true;
                            notes.push(format!("{inst_id} {} compliance active", ch.name));
                        } else if !ch.limited {
                            ch.limit_logged = false;
                        }
                    }
                }
                "dmm" => {
                    if let Some(ch) = inst.channels.first_mut() {
                        ch.voltage = 3.3012 + noise(&mut seed) * 0.0004;
                        ch.current = 0.0;
                        ch.resistance = Some(100.37 + noise(&mut seed) * 0.03);
                        push_sample(&mut ch.history_v, ch.voltage);
                        push_sample(&mut ch.history_i, ch.resistance.unwrap_or(0.0));
                    }
                }
                "can" => {
                    inst.bus_load = (14.0 + noise(&mut seed) * 5.0).clamp(2.0, 40.0);
                    frame_seq = frame_seq.wrapping_add(1);
                    let id = if frame_seq % 3 == 0 { 0x30 } else { 0x21 };
                    let b0 = (frame_seq & 0xff) as u8;
                    let b1 = ((frame_seq >> 2) & 0xff) as u8;
                    inst.frames.push(CanFrame {
                        id,
                        dlc: 2,
                        data: format!("{b0:02X} {b1:02X}"),
                    });
                    if inst.frames.len() > 8 {
                        inst.frames.remove(0);
                    }
                }
                _ => {}
            }
        }

        self.seed = seed;
        self.frame_seq = frame_seq;
        for note in notes {
            self.push_log("warn", note);
        }
        self.refresh_buses();
    }

    pub fn connect(&mut self, id: &str) -> Result<(), String> {
        let pos = self.require(id)?;
        self.instruments[pos].connected = true;
        self.instruments[pos].last_error = None;
        self.push_log("info", format!("connected {id}"));
        self.refresh_buses();
        Ok(())
    }

    pub fn disconnect(&mut self, id: &str) -> Result<(), String> {
        let pos = self.require(id)?;
        let inst = &mut self.instruments[pos];
        inst.connected = false;
        inst.output_on = false;
        inst.rtt_ms = 0.0;
        inst.bus_load = 0.0;
        for ch in &mut inst.channels {
            ch.output_on = false;
            ch.limited = false;
            ch.limit_logged = false;
            ch.voltage = 0.0;
            ch.current = 0.0;
            ch.resistance = None;
        }
        self.push_log("info", format!("disconnected {id}"));
        self.refresh_buses();
        Ok(())
    }

    pub fn connect_all(&mut self) {
        for inst in &mut self.instruments {
            inst.connected = true;
            inst.last_error = None;
        }
        self.push_log("info", "connected every instrument".into());
        self.refresh_buses();
    }

    pub fn outputs_off(&mut self) {
        let mut any = false;
        for inst in &mut self.instruments {
            for ch in &mut inst.channels {
                if ch.output_on {
                    any = true;
                    ch.output_on = false;
                    ch.limited = false;
                    ch.limit_logged = false;
                }
            }
            inst.output_on = false;
        }
        self.push_log(
            "warn",
            if any {
                "all outputs off".into()
            } else {
                "outputs already off".into()
            },
        );
    }

    pub fn set_source(
        &mut self,
        id: &str,
        channel: &str,
        mode: &str,
        level: f64,
        compliance: f64,
    ) -> Result<(), String> {
        let mode = mode.to_ascii_lowercase();
        if mode != "voltage" && mode != "current" {
            return Err(format!("unknown source mode {mode}"));
        }
        let pos = self.require(id)?;
        let kind = self.instruments[pos].kind.clone();
        if kind != "smu" && kind != "psu" {
            return Err("this instrument cannot source".into());
        }
        if kind == "psu" && mode != "voltage" {
            return Err("supply sources voltage only".into());
        }
        if !self.instruments[pos].connected {
            return Err("not connected".into());
        }
        if !level.is_finite() || !compliance.is_finite() {
            return Err("level and compliance must be finite".into());
        }
        let max_v = self.instruments[pos].max_volts;
        let max_i = self.instruments[pos].max_amps;
        let level_limit = if mode == "voltage" { max_v } else { max_i };
        let comp_limit = if mode == "voltage" { max_i } else { max_v };
        if level.abs() > level_limit {
            return Err(format!("level {level} is outside the instrument range"));
        }
        if compliance <= 0.0 || compliance > comp_limit {
            return Err(format!(
                "compliance {compliance} is outside the instrument range"
            ));
        }
        let idx = channel_index(&self.instruments[pos], channel)?;
        let ch = &mut self.instruments[pos].channels[idx];
        ch.source_mode = mode.clone();
        ch.source_level = level;
        ch.compliance = compliance;
        self.push_log(
            "info",
            format!("{id} {channel} {mode} {level} compliance {compliance}"),
        );
        Ok(())
    }

    pub fn set_output(&mut self, id: &str, channel: &str, on: bool) -> Result<(), String> {
        let pos = self.require(id)?;
        let kind = self.instruments[pos].kind.clone();
        if kind != "smu" && kind != "psu" {
            return Err("this instrument has no output".into());
        }
        if !self.instruments[pos].connected {
            return Err("not connected".into());
        }
        let idx = channel_index(&self.instruments[pos], channel)?;
        self.instruments[pos].channels[idx].output_on = on;
        if !on {
            self.instruments[pos].channels[idx].limited = false;
            self.instruments[pos].channels[idx].limit_logged = false;
        }
        self.instruments[pos].output_on = self.instruments[pos].channels.iter().any(|ch| ch.output_on);
        self.push_log(
            if on { "warn" } else { "info" },
            format!("{id} {channel} output {}", if on { "ON" } else { "OFF" }),
        );
        Ok(())
    }

    pub fn set_gpio(&mut self, id: &str, line: &str, on: bool) -> Result<(), String> {
        let pos = self.require(id)?;
        if self.instruments[pos].kind != "gpio" {
            return Err("this instrument has no gpio".into());
        }
        if !self.instruments[pos].connected {
            return Err("not connected".into());
        }
        let line_pos = self.instruments[pos]
            .lines
            .iter()
            .position(|item| item.name.eq_ignore_ascii_case(line))
            .ok_or_else(|| format!("no line {line}"))?;
        self.instruments[pos].lines[line_pos].on = on;
        self.push_log(
            "info",
            format!("{id} {line} {}", if on { "high" } else { "low" }),
        );
        Ok(())
    }

    pub fn command(&mut self, id: &str, line: &str) -> Result<String, String> {
        let pos = self.require(id)?;
        if !self.instruments[pos].connected {
            let message = "not connected".to_string();
            self.instruments[pos].last_error = Some(message.clone());
            self.push_log("error", format!("{id} {message}"));
            return Err(message);
        }
        let raw = line.trim().to_string();
        if raw.is_empty() {
            return Err("empty command".into());
        }
        let result = self.dispatch(id, &raw);
        match &result {
            Ok(response) => {
                if let Some(inst) = self.instruments.iter_mut().find(|inst| inst.id == id) {
                    inst.last_error = None;
                }
                self.push_log("info", format!("{id} >> {raw}"));
                self.push_log("info", format!("{id} << {response}"));
            }
            Err(message) => {
                if let Some(inst) = self.instruments.iter_mut().find(|inst| inst.id == id) {
                    inst.last_error = Some(message.clone());
                }
                self.push_log("error", format!("{id} >> {raw}"));
                self.push_log("error", format!("{id} {message}"));
            }
        }
        result
    }

    pub fn begin_scan(&mut self, bus: Option<&str>) -> Result<(), String> {
        let ids = self.resolve_buses(bus)?;
        for id in ids {
            let name = self
                .buses
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.name.clone())
                .unwrap_or_else(|| id.clone());
            if let Some(item) = self.buses.iter_mut().find(|item| item.id == id) {
                item.state = "scanning".into();
                item.detail = "scanning…".into();
            }
            self.push_log("info", format!("scan {name} started"));
        }
        Ok(())
    }

    pub fn finish_scan(&mut self, bus: Option<&str>) -> Result<(), String> {
        let ids = self.resolve_buses(bus)?;
        for id in ids {
            let (name, count) = self
                .buses
                .iter()
                .find(|item| item.id == id)
                .map(|item| (item.name.clone(), item.resources.len()))
                .unwrap_or_else(|| (id.clone(), 0));
            if let Some(item) = self.buses.iter_mut().find(|item| item.id == id) {
                item.state = "up".into();
                item.last_scan_ms = Some(now_ms());
            }
            self.push_log("info", format!("scan {name} complete · {count} resources"));
        }
        self.refresh_buses();
        Ok(())
    }

    pub fn prepare_sequence(&mut self, id: &str) -> Result<(), String> {
        if self.run.status == "running" {
            return Err("a sequence is already running".into());
        }
        let steps = catalog_steps(id).ok_or_else(|| format!("unknown sequence {id}"))?;
        self.abort = false;
        self.run = RunState {
            sequence_id: id.to_string(),
            status: "running".into(),
            step: Some(0),
            outcomes: vec!["pending".into(); steps.len()],
            detail: String::new(),
        };
        self.push_log("info", format!("sequence {id} started"));
        Ok(())
    }

    pub fn request_abort(&mut self) -> Result<(), String> {
        if self.run.status != "running" {
            return Err("no sequence is running".into());
        }
        self.abort = true;
        self.push_log("warn", "sequence abort requested".into());
        Ok(())
    }

    pub fn abort_requested(&self) -> bool {
        self.abort
    }

    fn mark_step(&mut self, index: usize, outcome: &str) {
        if index < self.run.outcomes.len() {
            self.run.outcomes[index] = outcome.to_string();
        }
        self.run.step = Some(index);
    }

    fn finish_run(&mut self, status: &str, detail: &str) {
        self.run.status = status.to_string();
        self.run.detail = detail.to_string();
        self.run.step = None;
        self.abort = false;
        let level = if status == "pass" { "info" } else { "error" };
        self.push_log(
            level,
            format!("sequence {} {status}: {detail}", self.run.sequence_id),
        );
    }

    fn exec(&mut self, action: &Action) -> Result<(), String> {
        match action {
            Action::Connect(id) => self.connect(id),
            Action::Source {
                id,
                channel,
                mode,
                level,
                compliance,
            } => self.set_source(id, channel, mode, *level, *compliance),
            Action::Output { id, channel, on } => self.set_output(id, channel, *on),
            Action::AssertCurrent {
                id,
                channel,
                lo,
                hi,
            } => {
                self.tick();
                let pos = self.require(id)?;
                if !self.instruments[pos].connected {
                    return Err("not connected".into());
                }
                let idx = channel_index(&self.instruments[pos], channel)?;
                let current = self.instruments[pos].channels[idx].current;
                if current >= *lo && current <= *hi {
                    Ok(())
                } else {
                    Err(format!("current {current:.4} A outside {lo}..{hi} A"))
                }
            }
            Action::AssertResistance { id, lo, hi } => {
                self.tick();
                let pos = self.require(id)?;
                if !self.instruments[pos].connected {
                    return Err("not connected".into());
                }
                let reading = self.instruments[pos]
                    .channels
                    .first()
                    .and_then(|ch| ch.resistance)
                    .unwrap_or(0.0);
                if reading >= *lo && reading <= *hi {
                    Ok(())
                } else {
                    Err(format!("{reading:.2} ohm outside {lo:.0}..{hi:.0}"))
                }
            }
            Action::Gpio { id, line, on } => self.set_gpio(id, line, *on),
            Action::AssertGpio { id, line, on } => {
                let pos = self.require(id)?;
                if !self.instruments[pos].connected {
                    return Err("not connected".into());
                }
                match self.instruments[pos]
                    .lines
                    .iter()
                    .find(|item| item.name.eq_ignore_ascii_case(line))
                {
                    Some(item) if item.on == *on => Ok(()),
                    Some(item) => Err(format!(
                        "{line} is {}",
                        if item.on { "high" } else { "low" }
                    )),
                    None => Err(format!("no line {line}")),
                }
            }
            Action::Settle(_) => Ok(()),
        }
    }

    fn dispatch(&mut self, id: &str, raw: &str) -> Result<String, String> {
        let pos = self.require(id)?;
        let kind = self.instruments[pos].kind.clone();
        let (channel, rest) = parse_prefix(&self.instruments[pos], raw)?;
        let cmd = normalize(rest);
        if cmd.is_empty() {
            return Err("empty command".into());
        }

        if cmd == "*IDN?" {
            let inst = &self.instruments[pos];
            return Ok(format!(
                "{},{},{},{}",
                inst.manufacturer, inst.model, inst.serial, inst.firmware
            ));
        }
        if cmd == "*CLS" {
            self.instruments[pos].last_error = None;
            return Ok("OK".into());
        }
        if cmd == "SYST:ERR?" {
            return Ok(self.instruments[pos]
                .last_error
                .clone()
                .unwrap_or_else(|| "0,\"No error\"".into()));
        }
        if kind == "can" || kind == "gpio" {
            return Err("command not supported on this interface".into());
        }
        if cmd == "*RST" {
            self.reset_channels(id)?;
            return Ok("OK".into());
        }
        if self.instruments[pos].channels.is_empty() {
            return Err("no channels".into());
        }
        let idx = if channel.is_empty() {
            0
        } else {
            channel_index(&self.instruments[pos], &channel)?
        };
        let ch_name = self.instruments[pos].channels[idx].name.clone();

        if cmd == "OUTP?" || cmd == "OUTP:STAT?" {
            let on = self.instruments[pos].channels[idx].output_on;
            return Ok(if on { "1".into() } else { "0".into() });
        }
        if let Some(on) = parse_outp(&cmd) {
            self.set_output(id, &ch_name, on)?;
            return Ok("OK".into());
        }
        if cmd == "MEAS:VOLT?" || cmd == "SENS:VOLT?" || cmd == "VOLT?" {
            self.tick();
            let v = self.instruments[self.require(id)?].channels[idx].voltage;
            return Ok(format!("{v:.6}"));
        }
        if cmd == "MEAS:CURR?" || cmd == "SENS:CURR?" || cmd == "CURR?" {
            self.tick();
            let current = self.instruments[self.require(id)?].channels[idx].current;
            return Ok(format!("{current:.6}"));
        }
        if cmd == "MEAS:RES?" || cmd == "RES?" {
            self.tick();
            let reading = self.instruments[self.require(id)?].channels[idx]
                .resistance
                .unwrap_or(0.0);
            return Ok(format!("{reading:.6}"));
        }
        if let Some(rest) = cmd.strip_prefix("SOUR:VOLT") {
            return self.apply_source_header(id, &ch_name, "voltage", rest);
        }
        if let Some(rest) = cmd.strip_prefix("SOUR:CURR") {
            return self.apply_source_header(id, &ch_name, "current", rest);
        }
        Err(format!("undefined header: {cmd}"))
    }

    fn apply_source_header(
        &mut self,
        id: &str,
        channel: &str,
        mode: &str,
        rest: &str,
    ) -> Result<String, String> {
        let rest = rest.trim().trim_start_matches(':').trim();
        let pos = self.require(id)?;
        let idx = channel_index(&self.instruments[pos], channel)?;
        if rest.is_empty() || rest == "?" {
            let level = self.instruments[pos].channels[idx].source_level;
            return Ok(format!("{level:.6}"));
        }
        if rest.starts_with("ILIM") || rest.starts_with("VLIM") || rest.starts_with("PROT") {
            self.set_compliance(id, channel, trailing_f64(rest)?)?;
            return Ok("OK".into());
        }
        let level = rest
            .parse::<f64>()
            .map_err(|_| format!("bad numeric '{rest}'"))?;
        let compliance = self.instruments[pos].channels[idx].compliance;
        self.set_source(id, channel, mode, level, compliance)?;
        Ok("OK".into())
    }

    fn set_compliance(&mut self, id: &str, channel: &str, compliance: f64) -> Result<(), String> {
        let pos = self.require(id)?;
        if !self.instruments[pos].connected {
            return Err("not connected".into());
        }
        let idx = channel_index(&self.instruments[pos], channel)?;
        let mode = self.instruments[pos].channels[idx].source_mode.clone();
        let max = if mode == "current" {
            self.instruments[pos].max_volts
        } else {
            self.instruments[pos].max_amps
        };
        if !compliance.is_finite() || compliance <= 0.0 || compliance > max {
            return Err(format!(
                "compliance {compliance} is outside the instrument range"
            ));
        }
        self.instruments[pos].channels[idx].compliance = compliance;
        Ok(())
    }

    fn reset_channels(&mut self, id: &str) -> Result<(), String> {
        let pos = self.require(id)?;
        let inst = &mut self.instruments[pos];
        inst.output_on = false;
        inst.last_error = None;
        for ch in &mut inst.channels {
            ch.output_on = false;
            ch.limited = false;
            ch.limit_logged = false;
            ch.source_mode = "voltage".into();
            ch.source_level = 0.0;
            ch.compliance = ch.default_compliance;
            ch.voltage = 0.0;
            ch.current = 0.0;
            ch.resistance = None;
        }
        self.push_log("info", format!("{id} reset"));
        Ok(())
    }

    fn require(&self, id: &str) -> Result<usize, String> {
        self.instruments
            .iter()
            .position(|inst| inst.id == id)
            .ok_or_else(|| format!("unknown instrument {id}"))
    }

    fn resolve_buses(&self, bus: Option<&str>) -> Result<Vec<String>, String> {
        match bus.map(str::trim).filter(|item| !item.is_empty()) {
            None => Ok(self.buses.iter().map(|item| item.id.clone()).collect()),
            Some(id) => {
                if self.buses.iter().any(|item| item.id == id) {
                    Ok(vec![id.to_string()])
                } else {
                    Err(format!("unknown bus {id}"))
                }
            }
        }
    }

    fn refresh_buses(&mut self) {
        let mut physical: HashMap<String, u32> = HashMap::new();
        let mut visa_open = 0u32;
        for inst in &self.instruments {
            if !inst.connected {
                continue;
            }
            *physical.entry(inst.bus.clone()).or_insert(0) += 1;
            if inst.visa {
                visa_open += 1;
            }
        }
        for bus in &mut self.buses {
            if bus.state == "scanning" {
                continue;
            }
            bus.state = "up".into();
            let open = if bus.id == "visa" {
                visa_open
            } else {
                physical.get(&bus.id).copied().unwrap_or(0)
            };
            bus.detail = match bus.id.as_str() {
                "gpib" => format!("GPIB0 · IEEE-488 · {open} open"),
                "visa" => format!("resource manager · {}", count_label(open, "session", "sessions")),
                "can" => format!("can0 · 500 kbit/s · {}", count_label(open, "node", "nodes")),
                "lan" => format!(
                    "eth0 · 1000 Mb/s · 192.168.1.10 · {}",
                    count_label(open, "socket", "sockets")
                ),
                "usb" => format!("USB hub · {}", count_label(open, "device open", "devices open")),
                _ => bus.detail.clone(),
            };
        }
    }

    fn push_log(&mut self, level: &str, message: String) {
        self.log.push_back(LogLine {
            ts_ms: now_ms(),
            level: level.into(),
            message,
        });
        while self.log.len() > 120 {
            self.log.pop_front();
        }
    }
}

pub fn spawn_sequence(state: AppState, id: String) {
    let Some(steps) = catalog_steps(&id) else {
        return;
    };
    tokio::spawn(async move {
        let mut failed = false;
        let mut aborted = false;
        let mut detail = String::new();
        for (index, step) in steps.iter().enumerate() {
            if (failed || aborted) && !step.cleanup {
                state.commit(|bench| bench.mark_step(index, "skip")).await;
                continue;
            }
            state.commit(|bench| bench.mark_step(index, "running")).await;
            let wait = match step.action {
                Action::Settle(ms) => ms,
                _ => 160,
            };
            if pause(&state, wait).await {
                aborted = true;
                if detail.is_empty() {
                    detail = "aborted by operator".into();
                }
            }
            if aborted && !step.cleanup {
                state.commit(|bench| bench.mark_step(index, "skip")).await;
                continue;
            }
            if matches!(step.action, Action::Settle(_)) {
                state.commit(|bench| bench.mark_step(index, "pass")).await;
                continue;
            }
            match state.commit(|bench| bench.exec(&step.action)).await {
                Ok(()) => state.commit(|bench| bench.mark_step(index, "pass")).await,
                Err(error) => {
                    failed = true;
                    detail = error;
                    state
                        .commit(|bench| bench.mark_step(index, "fail"))
                        .await;
                }
            }
        }
        let status = if aborted {
            "abort"
        } else if failed {
            "fail"
        } else {
            "pass"
        };
        if status == "pass" {
            detail = "all steps passed".into();
        }
        state
            .commit(move |bench| bench.finish_run(status, &detail))
            .await;
    });
}

async fn pause(state: &AppState, ms: u64) -> bool {
    let mut left = ms;
    while left > 0 {
        let slice = left.min(80);
        tokio::time::sleep(Duration::from_millis(slice)).await;
        left -= slice;
        if state.with_bench(|bench| bench.abort_requested()).await {
            return true;
        }
    }
    false
}

fn catalog() -> Vec<Sequence> {
    vec![
        Sequence {
            id: "power-rail".into(),
            name: "Power rail".into(),
            summary: "Source 3.300 V from the Keithley 2450 into the 100 Ω fixture and check the current.".into(),
            steps: vec![
                step("Connect Keithley 2450", Action::Connect("smu-2450".into())),
                step(
                    "Program 3.300 V, 100 mA compliance",
                    Action::Source {
                        id: "smu-2450".into(),
                        channel: "A".into(),
                        mode: "voltage".into(),
                        level: 3.3,
                        compliance: 0.1,
                    },
                ),
                step(
                    "Enable output",
                    Action::Output {
                        id: "smu-2450".into(),
                        channel: "A".into(),
                        on: true,
                    },
                ),
                step("Settle 400 ms", Action::Settle(400)),
                step(
                    "Assert channel A current is 10–80 mA",
                    Action::AssertCurrent {
                        id: "smu-2450".into(),
                        channel: "A".into(),
                        lo: 0.01,
                        hi: 0.08,
                    },
                ),
                cleanup_step(
                    "Disable output",
                    Action::Output {
                        id: "smu-2450".into(),
                        channel: "A".into(),
                        on: false,
                    },
                ),
            ],
        },
        Sequence {
            id: "continuity".into(),
            name: "Continuity".into(),
            summary: "Read the fixture resistance on the Keysight 34461A and require 95–105 Ω.".into(),
            steps: vec![
                step(
                    "Connect Keysight 34461A",
                    Action::Connect("dmm-34461".into()),
                ),
                step(
                    "Assert 4-wire resistance is 95–105 Ω",
                    Action::AssertResistance {
                        id: "dmm-34461".into(),
                        lo: 95.0,
                        hi: 105.0,
                    },
                ),
            ],
        },
        Sequence {
            id: "fixture-power".into(),
            name: "Fixture power".into(),
            summary: "Close the DUT power GPIO, confirm it, then open it again.".into(),
            steps: vec![
                step(
                    "Connect GPIO fixture",
                    Action::Connect("gpio-fixture".into()),
                ),
                step(
                    "Drive DUT_PWR high",
                    Action::Gpio {
                        id: "gpio-fixture".into(),
                        line: "DUT_PWR".into(),
                        on: true,
                    },
                ),
                step(
                    "Assert DUT_PWR is high",
                    Action::AssertGpio {
                        id: "gpio-fixture".into(),
                        line: "DUT_PWR".into(),
                        on: true,
                    },
                ),
                cleanup_step(
                    "Drive DUT_PWR low",
                    Action::Gpio {
                        id: "gpio-fixture".into(),
                        line: "DUT_PWR".into(),
                        on: false,
                    },
                ),
            ],
        },
    ]
}

fn describe() -> Vec<SequenceInfo> {
    catalog()
        .into_iter()
        .map(|seq| SequenceInfo {
            steps: seq.steps.into_iter().map(|step| step.name).collect(),
            id: seq.id,
            name: seq.name,
            summary: seq.summary,
        })
        .collect()
}

fn catalog_steps(id: &str) -> Option<Vec<StepDef>> {
    catalog()
        .into_iter()
        .find(|seq| seq.id == id)
        .map(|seq| seq.steps)
}

fn step(name: &str, action: Action) -> StepDef {
    StepDef {
        name: name.into(),
        cleanup: false,
        action,
    }
}

fn cleanup_step(name: &str, action: Action) -> StepDef {
    StepDef {
        name: name.into(),
        cleanup: true,
        action,
    }
}

fn idle_run() -> RunState {
    RunState {
        sequence_id: String::new(),
        status: "idle".into(),
        step: None,
        outcomes: Vec::new(),
        detail: String::new(),
    }
}

fn bus(id: &str, name: &str, resources: &[&str]) -> Bus {
    Bus {
        id: id.into(),
        name: name.into(),
        state: "up".into(),
        detail: String::new(),
        resources: resources.iter().map(|item| (*item).to_string()).collect(),
        last_scan_ms: None,
    }
}

fn instrument(
    id: &str,
    name: &str,
    manufacturer: &str,
    model: &str,
    serial: &str,
    firmware: &str,
    role: &str,
    kind: &str,
    bus_id: &str,
    resource: &str,
    visa: bool,
    max_volts: f64,
    max_amps: f64,
    channels: Vec<Channel>,
    lines: Vec<GpioLine>,
) -> Instrument {
    Instrument {
        id: id.into(),
        name: name.into(),
        manufacturer: manufacturer.into(),
        model: model.into(),
        serial: serial.into(),
        firmware: firmware.into(),
        role: role.into(),
        kind: kind.into(),
        bus: bus_id.into(),
        resource: resource.into(),
        visa,
        connected: false,
        output_on: false,
        rtt_ms: 0.0,
        last_error: None,
        max_volts,
        max_amps,
        channels,
        lines,
        frames: Vec::new(),
        bus_load: 0.0,
    }
}

fn channel(name: &str, load: f64, compliance: f64) -> Channel {
    Channel {
        name: name.into(),
        source_mode: "voltage".into(),
        source_level: 0.0,
        compliance,
        output_on: false,
        voltage: 0.0,
        current: 0.0,
        resistance: None,
        limited: false,
        load_ohms: load,
        history_v: Vec::new(),
        history_i: Vec::new(),
        default_compliance: compliance,
        limit_logged: false,
    }
}

fn line(name: &str, on: bool) -> GpioLine {
    GpioLine {
        name: name.into(),
        on,
        direction: "out".into(),
    }
}

fn update_sourced(ch: &mut Channel, sample: f64) {
    if !ch.output_on {
        ch.limited = false;
        ch.voltage = sample * 50e-6;
        ch.current = sample * 20e-9;
        ch.resistance = None;
        push_sample(&mut ch.history_v, ch.voltage);
        push_sample(&mut ch.history_i, ch.current);
        return;
    }

    let load = ch.load_ohms.max(1e-6);
    let (voltage, current, limited) = if ch.source_mode == "current" {
        let wanted = ch.source_level * load;
        if wanted.abs() > ch.compliance {
            let voltage = ch.compliance.copysign(wanted);
            (voltage, voltage / load, true)
        } else {
            (wanted, ch.source_level, false)
        }
    } else {
        let wanted = ch.source_level / load;
        if wanted.abs() > ch.compliance {
            let current = ch.compliance.copysign(wanted);
            (current * load, current, true)
        } else {
            (ch.source_level, wanted, false)
        }
    };
    ch.limited = limited;
    ch.voltage = voltage + sample * 80e-6;
    ch.current = current + sample * 40e-9;
    ch.resistance = Some(if ch.current.abs() > 1e-12 {
        ch.voltage / ch.current
    } else {
        load
    });
    push_sample(&mut ch.history_v, ch.voltage);
    push_sample(&mut ch.history_i, ch.current);
}

fn push_sample(history: &mut Vec<f64>, value: f64) {
    history.push(value);
    if history.len() > HISTORY {
        let extra = history.len() - HISTORY;
        history.drain(0..extra);
    }
}

fn channel_index(inst: &Instrument, name: &str) -> Result<usize, String> {
    inst.channels
        .iter()
        .position(|ch| ch.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("no channel {name}"))
}

fn parse_prefix<'a>(inst: &Instrument, line: &'a str) -> Result<(String, &'a str), String> {
    let line = line.trim();
    let bytes = line.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        let mark = bytes[0].to_ascii_uppercase();
        if mark == b'A' || mark == b'B' {
            let name = if mark == b'A' { "A" } else { "B" };
            if !inst
                .channels
                .iter()
                .any(|ch| ch.name.eq_ignore_ascii_case(name))
            {
                return Err(format!("no channel {name}"));
            }
            return Ok((name.to_string(), line[2..].trim()));
        }
    }
    Ok((String::new(), line))
}

fn normalize(line: &str) -> String {
    line.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .trim_start_matches(':')
        .to_ascii_uppercase()
}

fn parse_outp(cmd: &str) -> Option<bool> {
    let mut rest = cmd.strip_prefix("OUTP")?.trim();
    rest = rest.trim_start_matches(':').trim();
    if let Some(after) = rest.strip_prefix("STAT") {
        rest = after.trim().trim_start_matches(':').trim();
    }
    match rest {
        "ON" | "1" => Some(true),
        "OFF" | "0" => Some(false),
        _ => None,
    }
}

fn trailing_f64(text: &str) -> Result<f64, String> {
    let token = text.split_whitespace().last().unwrap_or(text);
    let token = token.trim_start_matches(':');
    token
        .parse::<f64>()
        .map_err(|_| format!("bad numeric '{token}'"))
}

fn noise(seed: &mut u32) -> f64 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let unit = ((*seed >> 8) & 0x00ff_ffff) as f64 / 16_777_216.0;
    unit * 2.0 - 1.0
}

fn count_label(count: u32, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smu_current(bench: &Bench) -> f64 {
        bench
            .instruments
            .iter()
            .find(|inst| inst.id == "smu-2450")
            .unwrap()
            .channels[0]
            .current
    }

    #[test]
    fn sources_3v3_into_100_ohm_fixture() {
        let mut bench = Bench::demo();
        bench.connect("smu-2450").unwrap();
        bench
            .command("smu-2450", "SOUR:VOLT 3.3")
            .unwrap();
        bench.command("smu-2450", "SOUR:VOLT:ILIM 0.1").unwrap();
        bench.command("smu-2450", "OUTP ON").unwrap();
        let reading: f64 = bench.command("smu-2450", "MEAS:CURR?").unwrap().parse().unwrap();
        assert!((reading - 0.033).abs() < 0.002, "{reading}");
        let idn = bench.command("smu-2450", "*IDN?").unwrap();
        assert!(idn.contains("2450"), "{idn}");
    }

    #[test]
    fn compliance_clamps_current() {
        let mut bench = Bench::demo();
        bench.connect("smu-2450").unwrap();
        bench
            .set_source("smu-2450", "A", "voltage", 5.0, 0.01)
            .unwrap();
        bench.set_output("smu-2450", "A", true).unwrap();
        bench.tick();
        let ch = &bench
            .instruments
            .iter()
            .find(|inst| inst.id == "smu-2450")
            .unwrap()
            .channels[0];
        assert!(ch.limited);
        assert!((ch.current - 0.01).abs() < 0.000_1, "{}", ch.current);
        bench.disconnect("smu-2450").unwrap();
        assert!(!bench.instruments.iter().find(|inst| inst.id == "smu-2450").unwrap().output_on);
        assert!(smu_current(&bench).abs() < 1e-6);
    }

    #[test]
    fn rejects_missing_session_and_channel() {
        let mut bench = Bench::demo();
        assert!(bench.command("smu-2450", "*IDN?").is_err());
        bench.connect("smu-2450").unwrap();
        assert!(bench.command("smu-2450", "B:MEAS:VOLT?").is_err());
        assert!(bench.begin_scan(Some("spacewire")).is_err());
    }

    #[test]
    fn continuity_and_gpio_actions() {
        let mut bench = Bench::demo();
        bench
            .exec(&Action::Connect("dmm-34461".into()))
            .unwrap();
        bench
            .exec(&Action::AssertResistance {
                id: "dmm-34461".into(),
                lo: 95.0,
                hi: 105.0,
            })
            .unwrap();
        bench
            .exec(&Action::Connect("gpio-fixture".into()))
            .unwrap();
        bench
            .exec(&Action::Gpio {
                id: "gpio-fixture".into(),
                line: "DUT_PWR".into(),
                on: true,
            })
            .unwrap();
        bench
            .exec(&Action::AssertGpio {
                id: "gpio-fixture".into(),
                line: "DUT_PWR".into(),
                on: true,
            })
            .unwrap();
    }
}
