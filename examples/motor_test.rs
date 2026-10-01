//! Short, slow motor test on a real Double Motor:
//! `cargo run --example motor_test`
use std::time::{Duration, Instant};

use hardware::*;

#[path = "../src/hardware/mod.rs"]
#[allow(dead_code)]
mod hardware;

#[tokio::main]
async fn main() {
    let dev = lego::LegoBleDevice::new();
    dev.connect().await.expect("connect");
    let pos = |dev: &lego::LegoBleDevice| dev.snapshot().first().map(|s| s.readings.clone());
    println!("connected: {:?}", dev.discover().await.unwrap());
    println!("before: {:?}", pos(&dev));

    let t = Instant::now();
    let r = dev
        .motor_run_for_time(Motor::DoubleBoth, Direction::Clockwise, Speed::new(30), Duration::from_secs(1))
        .await;
    println!("both 30% for 1 s -> {r:?} after {:?}", t.elapsed());
    println!("after: {:?}", pos(&dev));

    let t = Instant::now();
    let r = dev.motor_run_for_degrees(Motor::DoubleLeft, Direction::Clockwise, Speed::new(30), 180).await;
    println!("left 180° -> {r:?} after {:?}", t.elapsed());
    println!("after: {:?}", pos(&dev));

    let r = dev.motor_run(Motor::DoubleRight, Direction::CounterClockwise, Speed::new(25)).await;
    println!("right run 25% ccw -> {r:?}");
    tokio::time::sleep(Duration::from_millis(700)).await;
    println!("while running: {:?}", pos(&dev));
    let r = dev.motor_stop(Motor::DoubleRight).await;
    println!("right stop -> {r:?}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("final: {:?}", pos(&dev));

    dev.stop_all().await.ok();
    dev.disconnect().await.ok();
}
