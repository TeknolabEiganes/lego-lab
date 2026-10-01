//! Real LEGO Education Computer Science & AI hardware over Bluetooth LE.
//!
//! Every device (Single Motor, Double Motor, Colour Sensor, Controller) is
//! its own BLE peripheral. On connect we scan for the LEGO service, connect
//! to one device of each kind (the strongest signal) and keep a [`Link`] per
//! device. Wire formats live in [`protocol`]; see docs/lego-hardware.md.

pub mod protocol;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use btleplug::api::{
    Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use futures::StreamExt;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::{
    AttachedDevice, ConnectionState, DeviceKind, DeviceSnapshot, Direction, HardwareError,
    HwResult, LegoDevice, Lever, Motor, SensorQuery, SensorValue, Speed,
};
use protocol::{Advert, CommandStatus, Notification, ResponseKey};

/// GATT service shared by all Computer Science & AI devices.
pub const SERVICE_UUID: Uuid = Uuid::from_u128(0x0000fd02_0000_1000_8000_00805f9b34fb);
/// Characteristic the device receives commands on (write without response).
pub const WRITE_UUID: Uuid = Uuid::from_u128(0x0000fd02_0001_1000_8000_00805f9b34fb);
/// Characteristic the device sends responses/notifications on.
pub const NOTIFY_UUID: Uuid = Uuid::from_u128(0x0000fd02_0002_1000_8000_00805f9b34fb);
/// LEGO's Bluetooth SIG company identifier (manufacturer data).
pub const LEGO_COMPANY_ID: u16 = 0x0397;

const SCAN_TIME: Duration = Duration::from_secs(4);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);
const NOTIFY_INTERVAL_MS: u16 = 50;
/// Re-sending an identical "run" within this window is skipped, so a
/// Forever loop doesn't flood the radio with the same command.
const DEDUPE_WINDOW: Duration = Duration::from_millis(500);

fn comm(e: impl std::fmt::Display) -> HardwareError {
    HardwareError::Communication(e.to_string())
}

#[derive(Default)]
struct Live {
    battery: Option<u8>,
    button: bool,
    /// Relative positions per motor bit (index 0 = mask 1, 1 = mask 2).
    motor_position: [Option<i32>; 2],
    /// Angle of the motor shaft, 0–359°.
    motor_angle: [Option<u16>; 2],
    motor_speed: [Option<i8>; 2],
    color: Option<(i8, u8)>,
    levers: Option<(i8, i8)>,
    /// Card scanned onto the device while connected (colour, serial).
    card: Option<(i8, u16)>,
    got_first: bool,
}

/// One connected device.
struct Link {
    kind: DeviceKind,
    advert: Advert,
    peripheral: Peripheral,
    write_char: Characteristic,
    pending: Mutex<HashMap<ResponseKey, VecDeque<oneshot::Sender<Vec<u8>>>>>,
    live: Mutex<Live>,
    alive: AtomicBool,
    last_run: Mutex<HashMap<u8, (u8, Direction, Instant)>>,
}

impl Link {
    /// Current connection card: the latest one scanned while connected,
    /// otherwise the one in the advertisement.
    fn card(&self) -> String {
        match self.live.lock().unwrap_or_else(|e| e.into_inner()).card {
            Some((color, serial)) => protocol::card_label(color, serial),
            None => self.advert.card(),
        }
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn mark_dead(&self) {
        self.alive.store(false, Ordering::SeqCst);
        // Dropping the senders fails every waiting request.
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    fn handle(&self, message: &[u8]) {
        if message.first() == Some(&protocol::DEVICE_NOTIFICATION) {
            let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
            for n in protocol::parse_notification(message) {
                match n {
                    Notification::Info { battery, .. } => live.battery = Some(battery),
                    Notification::Button { pressed } => live.button = pressed,
                    Notification::Motor { mask, angle, position, speed, .. } => {
                        let i = if mask == 2 { 1 } else { 0 };
                        live.motor_position[i] = Some(position);
                        live.motor_angle[i] = Some(angle);
                        live.motor_speed[i] = Some(speed);
                    }
                    Notification::Color { color, reflection } => live.color = Some((color, reflection)),
                    Notification::Controller { left, right } => live.levers = Some((left, right)),
                    Notification::Card { color, serial } => live.card = Some((color, serial)),
                    Notification::Imu { .. } | Notification::Gesture(_) => {}
                }
            }
            live.got_first = true;
            return;
        }
        let Some(key) = protocol::response_key(message) else { return };
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(queue) = pending.get_mut(&key) {
            // Skip requests that gave up waiting (timed out or cancelled).
            while let Some(tx) = queue.pop_front() {
                if tx.send(message.to_vec()).is_ok() {
                    break;
                }
            }
            if queue.is_empty() {
                pending.remove(&key);
            }
        }
    }

    async fn write(&self, data: &[u8]) -> HwResult<()> {
        if !self.is_alive() {
            return Err(HardwareError::DeviceMissing(self.kind));
        }
        self.peripheral
            .write(&self.write_char, data, WriteType::WithoutResponse)
            .await
            .map_err(comm)
    }

    /// Send `data` and wait for the reply matching `key`.
    async fn request(&self, data: &[u8], key: ResponseKey, timeout: Duration) -> HwResult<Vec<u8>> {
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key)
            .or_default()
            .push_back(tx);
        self.write(data).await?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => Err(HardwareError::DeviceMissing(self.kind)),
            Err(_) => Err(comm(format!("the {} did not answer", self.kind.label()))),
        }
    }

    /// Send a motor command and wait for its result.
    async fn motor_command(&self, data: &[u8], result_id: u8, mask: u8, timeout: Duration) -> HwResult<()> {
        let reply = self.request(data, (result_id, Some(mask)), timeout).await?;
        match protocol::parse_status(&reply) {
            Some(CommandStatus::Refused) => Err(comm(format!("the {} refused the command", self.kind.label()))),
            // Interrupted means a newer command (or STOP) took over: done.
            _ => Ok(()),
        }
    }
}

async fn adapter() -> HwResult<Adapter> {
    let manager = Manager::new()
        .await
        .map_err(|e| comm(format!("cannot reach the Bluetooth service (is bluetooth.service running?): {e}")))?;
    manager
        .adapters()
        .await
        .map_err(comm)?
        .into_iter()
        .next()
        .ok_or_else(|| comm("no Bluetooth adapter found"))
}

/// A LEGO device seen during a scan.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundDevice {
    /// Bluetooth address; identifies the device for `connect_device`.
    pub address: String,
    pub kind: DeviceKind,
    /// Connection card, e.g. "blue 3683".
    pub card: String,
    pub rssi: Option<i16>,
}

/// Scan for LEGO devices, strongest signal first within each kind.
async fn scan_all(adapter: &Adapter) -> HwResult<Vec<(Peripheral, Advert, Option<i16>)>> {
    // No BlueZ service filter: with a UUID filter BlueZ (5.8x) silently
    // doesn't start discovery and nothing is found. We filter below instead.
    if let Err(e) = adapter.start_scan(ScanFilter::default()).await {
        // Another app (or a previous scan) may already be discovering; the
        // results are shared, so carry on.
        if !e.to_string().contains("InProgress") {
            return Err(comm(format!("cannot scan (is Bluetooth switched on?): {e}")));
        }
    }
    tokio::time::sleep(SCAN_TIME).await;
    let _ = adapter.stop_scan().await;

    let mut found = Vec::new();
    for p in adapter.peripherals().await.map_err(comm)? {
        let Ok(Some(props)) = p.properties().await else { continue };
        if !props.services.contains(&SERVICE_UUID) {
            continue;
        }
        if let Some(advert) = props.manufacturer_data.get(&LEGO_COMPANY_ID).and_then(|d| protocol::parse_advert(d)) {
            found.push((p, advert, props.rssi));
        }
    }
    found.sort_by_key(|(_, a, rssi)| (a.kind, std::cmp::Reverse(rssi.unwrap_or(i16::MIN))));
    Ok(found)
}

/// One line per LEGO device in range (for `lego-lab --scan`).
pub async fn scan_report() -> HwResult<Vec<String>> {
    let adapter = adapter().await?;
    Ok(scan_all(&adapter)
        .await?
        .into_iter()
        .map(|(p, a, rssi)| {
            let signal = rssi.map_or("?".to_string(), |r| format!("{r} dBm"));
            format!("{:<14} card {:<14} signal {:<8} address {}", a.kind.label(), a.card(), signal, p.address())
        })
        .collect())
}

/// Connect to everything in range, print live readings, disconnect
/// (for `lego-lab --probe`). Never moves a motor.
pub async fn probe() -> HwResult<()> {
    let dev = LegoBleDevice::new();
    dev.connect().await?;
    for d in dev.discover().await? {
        println!("  connected: {} ({})", d.kind.label(), d.name);
    }
    let show = |when: &str| {
        for s in dev.snapshot() {
            let readings: Vec<String> = s.readings.iter().map(|(k, v)| format!("{k}={v}")).collect();
            let battery = s.battery.map_or("?".into(), |b| format!("{b}%"));
            println!("  {when:<12} {:<14} battery {:<5} {}", s.kind.label(), battery, readings.join("  "));
        }
    };
    tokio::time::sleep(Duration::from_secs(1)).await;
    show("now");
    // Zeroing the position counters doesn't move anything.
    dev.reset_motor_positions().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    show("after reset");
    dev.disconnect().await
}

async fn connect_link(peripheral: Peripheral, advert: Advert) -> HwResult<Arc<Link>> {
    let name = format!("{} ({})", advert.kind.label(), advert.card());
    tokio::time::timeout(CONNECT_TIMEOUT, peripheral.connect())
        .await
        .map_err(|_| comm(format!("{name}: connection timed out")))?
        .map_err(|e| comm(format!("{name}: {e}")))?;
    peripheral.discover_services().await.map_err(comm)?;
    let chars = peripheral.characteristics();
    let find = |uuid| chars.iter().find(|c| c.uuid == uuid).cloned();
    let (Some(write_char), Some(notify_char)) = (find(WRITE_UUID), find(NOTIFY_UUID)) else {
        let _ = peripheral.disconnect().await;
        return Err(comm(format!("{name} is not a LEGO Education device")));
    };
    peripheral.subscribe(&notify_char).await.map_err(comm)?;
    let mut stream = peripheral.notifications().await.map_err(comm)?;

    let link = Arc::new(Link {
        kind: advert.kind,
        advert,
        peripheral: peripheral.clone(),
        write_char,
        pending: Mutex::default(),
        live: Mutex::default(),
        alive: AtomicBool::new(true),
        last_run: Mutex::default(),
    });
    let reader = link.clone();
    tokio::spawn(async move {
        while let Some(n) = stream.next().await {
            if n.uuid == NOTIFY_UUID {
                reader.handle(&n.value);
            }
        }
        reader.mark_dead();
    });

    let setup = async {
        let reply = link.request(&protocol::info_request(), (protocol::INFO_RESPONSE, None), REPLY_TIMEOUT).await?;
        let info = protocol::parse_info(&reply).ok_or_else(|| comm(format!("{name} sent a bad info reply")))?;
        if !info.rpc_compatible() {
            let (major, minor, build) = info.rpc;
            return Err(comm(format!(
                "{name} uses protocol version {major}.{minor}.{build}, which LEGO Lab doesn't know yet. \
                 Updating the firmware at https://code.legoeducation.com may help"
            )));
        }
        let key = (protocol::DEVICE_NOTIFICATION_RESPONSE, None);
        link.request(&protocol::device_notification_request(NOTIFY_INTERVAL_MS), key, REPLY_TIMEOUT).await?;
        // Wait (briefly) for the first sensor reading.
        for _ in 0..50 {
            if link.live.lock().unwrap_or_else(|e| e.into_inner()).got_first {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok(())
    };
    if let Err(e) = setup.await {
        link.mark_dead();
        let _ = peripheral.disconnect().await;
        return Err(e);
    }
    Ok(link)
}

#[derive(Default)]
struct Shared {
    /// Number of connects in progress.
    busy: AtomicUsize,
    /// Number of scans in progress (a scan alone isn't "connecting").
    scanning: AtomicUsize,
    links: Mutex<Vec<Arc<Link>>>,
    /// Devices seen in the last scan, by address.
    found: Mutex<Vec<(Peripheral, Advert)>>,
    found_list: Mutex<Vec<FoundDevice>>,
    adapter: tokio::sync::Mutex<Option<Adapter>>,
}

/// All connected LEGO devices. Cheap to clone; clones share state.
#[derive(Clone, Default)]
pub struct LegoBleDevice {
    shared: Arc<Shared>,
}

/// Counts a scan/connect as in progress until dropped, even on error.
struct BusyGuard<'a>(&'a AtomicUsize);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl LegoBleDevice {
    pub fn new() -> Self {
        Self::default()
    }

    fn busy(&self) -> BusyGuard<'_> {
        self.shared.busy.fetch_add(1, Ordering::SeqCst);
        BusyGuard(&self.shared.busy)
    }

    fn scanning(&self) -> BusyGuard<'_> {
        self.shared.scanning.fetch_add(1, Ordering::SeqCst);
        BusyGuard(&self.shared.scanning)
    }

    fn is_connecting(&self) -> bool {
        self.shared.busy.load(Ordering::SeqCst) > 0
    }

    /// A scan or connect is in progress.
    pub fn is_busy(&self) -> bool {
        self.is_connecting() || self.shared.scanning.load(Ordering::SeqCst) > 0
    }

    fn links(&self) -> Vec<Arc<Link>> {
        self.shared.links.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|l| l.is_alive()).cloned().collect()
    }

    fn link(&self, kind: DeviceKind) -> HwResult<Arc<Link>> {
        if self.connection() != ConnectionState::Connected {
            return Err(HardwareError::NotConnected);
        }
        self.links().into_iter().find(|l| l.kind == kind).ok_or(HardwareError::DeviceMissing(kind))
    }

    /// Bluetooth addresses of the connected devices.
    pub fn connected_addresses(&self) -> Vec<String> {
        self.links().iter().map(|l| l.peripheral.address().to_string()).collect()
    }

    /// The adapter, created once; also starts watching for disconnects.
    async fn adapter(&self) -> HwResult<Adapter> {
        let mut slot = self.shared.adapter.lock().await;
        if let Some(a) = slot.as_ref() {
            return Ok(a.clone());
        }
        let a = adapter().await?;
        self.watch_disconnects(a.clone());
        *slot = Some(a.clone());
        Ok(a)
    }

    /// Watch for devices dropping off (switched off, out of range).
    fn watch_disconnects(&self, adapter: Adapter) {
        let shared = Arc::downgrade(&self.shared);
        tokio::spawn(async move {
            let Ok(mut events) = adapter.events().await else { return };
            while let Some(event) = events.next().await {
                let CentralEvent::DeviceDisconnected(id) = event else { continue };
                let Some(shared) = shared.upgrade() else { return };
                let id: PeripheralId = id;
                let links = shared.links.lock().unwrap_or_else(|e| e.into_inner()).clone();
                for l in links.iter().filter(|l| l.peripheral.id() == id) {
                    l.mark_dead();
                }
            }
        });
    }

    /// Add a connected device, replacing any other device of the same kind
    /// (blocks address devices by kind).
    async fn add_link(&self, link: Arc<Link>) {
        let replaced: Vec<Arc<Link>> = {
            let mut links = self.shared.links.lock().unwrap_or_else(|e| e.into_inner());
            let (old, keep): (Vec<_>, Vec<_>) =
                links.drain(..).partition(|l| l.kind == link.kind || !l.is_alive());
            *links = keep;
            links.push(link);
            old
        };
        for old in replaced {
            old.mark_dead();
            let _ = old.peripheral.disconnect().await;
        }
    }

    /// Scan for every LEGO device in range.
    pub async fn scan_devices(&self) -> HwResult<Vec<FoundDevice>> {
        let _scanning = self.scanning();
        let adapter = self.adapter().await?;
        let found = scan_all(&adapter).await?;
        let list: Vec<FoundDevice> = found
            .iter()
            .map(|(p, a, rssi)| FoundDevice {
                address: p.address().to_string(),
                kind: a.kind,
                card: a.card(),
                rssi: *rssi,
            })
            .collect();
        *self.shared.found.lock().unwrap_or_else(|e| e.into_inner()) =
            found.into_iter().map(|(p, a, _)| (p, a)).collect();
        *self.shared.found_list.lock().unwrap_or_else(|e| e.into_inner()) = list.clone();
        Ok(list)
    }

    /// Result of the most recent scan.
    pub fn last_scan(&self) -> Vec<FoundDevice> {
        self.shared.found_list.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Connect one device from the last scan.
    pub async fn connect_device(&self, address: &str) -> HwResult<()> {
        let entry = self
            .shared
            .found
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(p, _)| p.address().to_string() == address)
            .cloned();
        let Some((peripheral, advert)) = entry else {
            return Err(comm("that device is no longer in range; scan again"));
        };
        let _busy = self.busy();
        self.adapter().await?;
        let link = connect_link(peripheral, advert).await?;
        self.add_link(link).await;
        Ok(())
    }

    /// Disconnect one device.
    pub async fn disconnect_device(&self, address: &str) -> HwResult<()> {
        let link = self.links().into_iter().find(|l| l.peripheral.address().to_string() == address);
        if let Some(link) = link {
            if let Some(mask) = motor_stop_mask(link.kind) {
                let _ = link.write(&protocol::motor_stop(mask)).await;
            }
            link.mark_dead();
            let _ = link.peripheral.disconnect().await;
        }
        Ok(())
    }

    async fn run_with_speed(&self, motor: Motor, speed: Speed, command: Vec<u8>, result: u8, timeout: Duration) -> HwResult<()> {
        let link = self.link(motor.device())?;
        let mask = protocol::motor_mask(motor);
        link.last_run.lock().unwrap_or_else(|e| e.into_inner()).remove(&mask);
        // Speed is a separate message, sent in the same write (as LEGO's package does).
        let mut data = protocol::motor_set_speed(mask, speed.percent() as i8);
        data.extend(command);
        link.motor_command(&data, result, mask, timeout).await
    }
}

/// Mask that stops every motor on a device, if it has motors.
fn motor_stop_mask(kind: DeviceKind) -> Option<u8> {
    match kind {
        DeviceKind::SingleMotor => Some(1),
        DeviceKind::DoubleMotor => Some(3),
        _ => None,
    }
}

fn no_reading(kind: DeviceKind) -> HardwareError {
    comm(format!("no reading from the {} yet", kind.label()))
}

/// Degrees turned since the last reset, with turns for big numbers:
/// "120°" or "10005° (27.8 turns)".
fn format_position(degrees: i32) -> String {
    if degrees.abs() < 720 {
        format!("{degrees}°")
    } else {
        format!("{degrees}° ({:.1} turns)", degrees as f64 / 360.0)
    }
}

fn motor_index(motor: Motor) -> usize {
    if motor == Motor::DoubleRight { 1 } else { 0 }
}

#[async_trait]
impl LegoDevice for LegoBleDevice {
    fn backend_name(&self) -> &'static str {
        "LEGO Bluetooth"
    }

    fn connection(&self) -> ConnectionState {
        if !self.links().is_empty() {
            ConnectionState::Connected
        } else if self.is_connecting() {
            ConnectionState::Connecting
        } else {
            ConnectionState::Disconnected
        }
    }

    /// Scan and connect the strongest-signal device of each kind.
    async fn connect(&self) -> HwResult<()> {
        let _busy = self.busy();
        let found = self.scan_devices().await?;
        if found.is_empty() {
            return Err(comm(
                "no LEGO devices found. Switch them on and check their light is blinking (ready to connect)",
            ));
        }
        let mut best: Vec<&FoundDevice> = Vec::new();
        for d in &found {
            // `found` is sorted strongest-first within each kind.
            if !best.iter().any(|b| b.kind == d.kind) {
                best.push(d);
            }
        }
        let attempts = futures::future::join_all(best.iter().map(|d| self.connect_device(&d.address))).await;
        let mut first_error = None;
        for e in attempts.into_iter().filter_map(Result::err) {
            eprintln!("LEGO Lab: {e}");
            first_error.get_or_insert(e);
        }
        match first_error {
            Some(e) if self.links().is_empty() => Err(e),
            _ => Ok(()),
        }
    }

    async fn disconnect(&self) -> HwResult<()> {
        let _ = self.stop_all().await;
        let links = std::mem::take(&mut *self.shared.links.lock().unwrap_or_else(|e| e.into_inner()));
        for link in links {
            link.mark_dead();
            let _ = link.peripheral.disconnect().await;
        }
        Ok(())
    }

    async fn discover(&self) -> HwResult<Vec<AttachedDevice>> {
        if self.connection() != ConnectionState::Connected {
            return Err(HardwareError::NotConnected);
        }
        Ok(self.links().iter().map(|l| AttachedDevice { kind: l.kind, name: l.card() }).collect())
    }

    async fn motor_run(&self, motor: Motor, direction: Direction, speed: Speed) -> HwResult<()> {
        let link = self.link(motor.device())?;
        let mask = protocol::motor_mask(motor);
        let pct = speed.percent();
        let repeated = link
            .last_run
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&mask)
            .is_some_and(|(s, d, t)| *s == pct && *d == direction && t.elapsed() < DEDUPE_WINDOW);
        if repeated {
            return Ok(());
        }
        let mut data = protocol::motor_set_speed(mask, pct as i8);
        data.extend(protocol::motor_run(mask, direction));
        link.motor_command(&data, protocol::MOTOR_RUN_RESULT, mask, REPLY_TIMEOUT).await?;
        link.last_run.lock().unwrap_or_else(|e| e.into_inner()).insert(mask, (pct, direction, Instant::now()));
        Ok(())
    }

    async fn motor_run_for_time(&self, motor: Motor, direction: Direction, speed: Speed, duration: Duration) -> HwResult<()> {
        let ms = duration.as_millis().min(u32::MAX as u128) as u32;
        let mask = protocol::motor_mask(motor);
        let command = protocol::motor_run_for_time(mask, ms, direction);
        self.run_with_speed(motor, speed, command, protocol::MOTOR_RUN_FOR_TIME_RESULT, duration + Duration::from_secs(5))
            .await
    }

    async fn motor_run_for_degrees(&self, motor: Motor, direction: Direction, speed: Speed, degrees: u32) -> HwResult<()> {
        let mask = protocol::motor_mask(motor);
        let degrees = degrees.min(i32::MAX as u32) as i32;
        let command = protocol::motor_run_for_degrees(mask, degrees, direction);
        // Generous upper bound: the motor itself reports when it is done.
        let estimate = degrees as f64 / (speed.percent().max(1) as f64 * 3.0);
        let timeout = Duration::from_secs_f64(estimate + 10.0);
        self.run_with_speed(motor, speed, command, protocol::MOTOR_RUN_FOR_DEGREES_RESULT, timeout).await
    }

    async fn motor_stop(&self, motor: Motor) -> HwResult<()> {
        let link = self.link(motor.device())?;
        let mask = protocol::motor_mask(motor);
        link.last_run.lock().unwrap_or_else(|e| e.into_inner()).clear();
        link.motor_command(&protocol::motor_stop(mask), protocol::MOTOR_STOP_RESULT, mask, REPLY_TIMEOUT).await
    }

    async fn motor_reset_position(&self, motor: Motor) -> HwResult<()> {
        let link = self.link(motor.device())?;
        let mask = protocol::motor_mask(motor);
        let data = protocol::motor_reset_relative_position(mask, 0);
        link.motor_command(&data, protocol::MOTOR_RESET_RELATIVE_POSITION_RESULT, mask, REPLY_TIMEOUT).await
    }

    async fn reset_motor_positions(&self) -> HwResult<()> {
        let resets = self.links().into_iter().filter_map(|link| {
            let mask = motor_stop_mask(link.kind)?;
            Some(async move {
                let data = protocol::motor_reset_relative_position(mask, 0);
                link.motor_command(&data, protocol::MOTOR_RESET_RELATIVE_POSITION_RESULT, mask, REPLY_TIMEOUT).await
            })
        });
        let results = futures::future::join_all(resets).await;
        results.into_iter().find(|r| r.is_err()).unwrap_or(Ok(()))
    }

    async fn stop_all(&self) -> HwResult<()> {
        // Fire-and-forget so STOP is never held up by a slow device.
        let stops = self.links().into_iter().filter_map(|link| {
            let mask = motor_stop_mask(link.kind)?;
            link.last_run.lock().unwrap_or_else(|e| e.into_inner()).clear();
            Some(async move { link.write(&protocol::motor_stop(mask)).await })
        });
        let results = futures::future::join_all(stops).await;
        results.into_iter().find(|r| r.is_err()).unwrap_or(Ok(()))
    }

    async fn read_sensor(&self, query: SensorQuery) -> HwResult<SensorValue> {
        let kind = match query {
            SensorQuery::Color | SensorQuery::Reflection => DeviceKind::ColorSensor,
            SensorQuery::MotorPosition(m) => m.device(),
            SensorQuery::ControllerLever(_) => DeviceKind::Controller,
            SensorQuery::Button(k) => k,
        };
        let link = self.link(kind)?;
        let live = link.live.lock().unwrap_or_else(|e| e.into_inner());
        match query {
            SensorQuery::Color => live.color.map(|(c, _)| SensorValue::Color(protocol::sensor_color(c))),
            SensorQuery::Reflection => live.color.map(|(_, r)| SensorValue::Percent(r as i32)),
            SensorQuery::MotorPosition(m) => live.motor_position[motor_index(m)].map(|p| SensorValue::Degrees(p as i64)),
            SensorQuery::ControllerLever(lever) => live.levers.map(|(l, r)| {
                SensorValue::Percent(if lever == Lever::Left { l } else { r } as i32)
            }),
            SensorQuery::Button(_) => Some(SensorValue::Pressed(live.button)),
        }
        .ok_or_else(|| no_reading(kind))
    }

    fn snapshot(&self) -> Vec<DeviceSnapshot> {
        self.links()
            .iter()
            .map(|link| {
                let card = link.card();
                let live = link.live.lock().unwrap_or_else(|e| e.into_inner());
                let mut readings = Vec::new();
                let motor = |r: &mut Vec<(String, String)>, label: &str, i: usize| {
                    if let (Some(p), Some(s)) = (live.motor_position[i], live.motor_speed[i]) {
                        r.push((format!("{label} speed"), format!("{s}%")));
                        r.push((format!("{label} position"), format_position(p)));
                        if let Some(a) = live.motor_angle[i] {
                            r.push((format!("{label} angle"), format!("{a}°")));
                        }
                    }
                };
                match link.kind {
                    DeviceKind::SingleMotor => motor(&mut readings, "Motor", 0),
                    DeviceKind::DoubleMotor => {
                        motor(&mut readings, "Left", 0);
                        motor(&mut readings, "Right", 1);
                    }
                    DeviceKind::ColorSensor => {
                        if let Some((c, r)) = live.color {
                            readings.push(("Colour".into(), protocol::sensor_color(c).label().into()));
                            readings.push(("Reflection".into(), format!("{r}%")));
                        }
                    }
                    DeviceKind::Controller => {
                        if let Some((l, r)) = live.levers {
                            readings.push(("Left lever".into(), format!("{l}%")));
                            readings.push(("Right lever".into(), format!("{r}%")));
                        }
                    }
                }
                readings.push(("Button".into(), if live.button { "pressed" } else { "—" }.into()));
                DeviceSnapshot {
                    kind: link.kind,
                    address: link.peripheral.address().to_string(),
                    card,
                    battery: live.battery,
                    readings,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::format_position;

    #[test]
    fn positions_show_turns_when_large() {
        assert_eq!(format_position(120), "120°");
        assert_eq!(format_position(-90), "-90°");
        assert_eq!(format_position(10005), "10005° (27.8 turns)");
        assert_eq!(format_position(-3628), "-3628° (-10.1 turns)");
    }
}
