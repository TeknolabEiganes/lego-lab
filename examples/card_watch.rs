//! Watch a connected device for connection-card taps, then check the
//! advertisement after disconnecting: `cargo run --example card_watch`
use std::time::Duration;

use hardware::*;

#[path = "../src/hardware/mod.rs"]
#[allow(dead_code)]
mod hardware;

#[tokio::main]
async fn main() {
    let dev = lego::LegoBleDevice::new();
    dev.connect().await.expect("connect");
    println!("connected: {:?}", dev.discover().await.unwrap());
    println!(">>> Tap the card on the device now (watching for 25 s)…");
    let mut last = String::new();
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let cards: Vec<String> = dev.snapshot().iter().map(|s| format!("{}: {}", s.kind.label(), s.card)).collect();
        let now = cards.join(", ");
        if now != last {
            println!("    card now: {now}");
            last = now;
        }
    }
    dev.disconnect().await.ok();
    println!("disconnected; scanning to see what it advertises…");
    tokio::time::sleep(Duration::from_secs(2)).await;
    for line in lego::scan_report().await.unwrap() {
        println!("    {line}");
    }
}
