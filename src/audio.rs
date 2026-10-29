//! Sound on the computer's own speakers, for Beep and Play Tune blocks set
//! to play "on this computer".
//!
//! The LEGO devices only beep, so the computer plays the same kind of sound:
//! a square-wave tone shaped like the devices' beep patterns.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::hardware::{BeepPattern, Tone};

pub const DEFAULT_VOLUME: u32 = 100;
const SAMPLE_RATE: u32 = 44_100;
/// Fade in/out so tones start and end without a click.
const FADE_MS: f32 = 4.0;
/// Loudness of a square wave at 100% volume (it is loud for its amplitude).
const FULL_SCALE: f32 = 0.45;

/// Where a sound block plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundOutput {
    /// The LEGO devices in the block's card group.
    #[default]
    Devices,
    /// The computer's speakers only.
    Computer,
    /// Both the devices and the computer.
    Both,
}

impl SoundOutput {
    pub const ALL: [SoundOutput; 3] = [SoundOutput::Devices, SoundOutput::Computer, SoundOutput::Both];

    pub fn label(self) -> &'static str {
        match self {
            SoundOutput::Devices => "LEGO devices",
            SoundOutput::Computer => "This computer",
            SoundOutput::Both => "Devices + computer",
        }
    }

    pub fn devices(self) -> bool {
        self != SoundOutput::Computer
    }

    pub fn computer(self) -> bool {
        self != SoundOutput::Devices
    }
}

/// A sound as (frequency in Hz, milliseconds) pieces; 0 Hz is silence.
pub type Notes = Vec<(u16, u32)>;

/// The pieces of a device beep pattern, repeated `count` times.
pub fn beep_notes(tone: Tone) -> Notes {
    let f = tone.frequency;
    let scaled = |ratio: f32| (f as f32 * ratio).round() as u16;
    let once: Notes = match tone.pattern {
        BeepPattern::Single => vec![(f, 200)],
        BeepPattern::Double => vec![(f, 120), (0, 80), (f, 120)],
        BeepPattern::Triple => vec![(f, 120), (0, 80), (f, 120), (0, 80), (f, 120)],
        BeepPattern::UpMiddleDown => vec![(scaled(1.26), 120), (f, 120), (scaled(0.79), 120)],
    };
    let mut out = Notes::new();
    for i in 0..tone.count.max(1) {
        if i > 0 {
            out.push((0, 150));
        }
        out.extend(&once);
    }
    out
}

pub fn length(notes: &[(u16, u32)]) -> Duration {
    Duration::from_millis(notes.iter().map(|&(_, ms)| ms as u64).sum())
}

/// Something that can play sounds on the computer (the real speakers, or a
/// fake in tests).
pub trait Speaker: Send + Sync {
    /// Start playing (without waiting). `volume` is 0–100%.
    fn play(&self, notes: &[(u16, u32)], volume: u32) -> Result<(), String>;
    /// Silence everything that is playing.
    fn stop(&self);
}

fn samples(notes: &[(u16, u32)], volume: u32) -> Vec<f32> {
    let amplitude = FULL_SCALE * volume.min(100) as f32 / 100.0;
    let rate = SAMPLE_RATE as f32;
    let fade = (FADE_MS / 1000.0 * rate) as usize;
    let mut out = Vec::new();
    for &(hz, ms) in notes {
        let n = (ms as f32 / 1000.0 * rate) as usize;
        if hz == 0 {
            out.extend(std::iter::repeat_n(0.0, n));
            continue;
        }
        let period = rate / hz as f32;
        for i in 0..n {
            let envelope = (i.min(n - i) as f32 / fade.max(1) as f32).min(1.0);
            let wave = if (i as f32 % period) < period / 2.0 { 1.0 } else { -1.0 };
            out.push(wave * amplitude * envelope);
        }
    }
    out
}

/// The computer's default sound output, opened the first time something
/// plays.
#[derive(Clone, Default)]
pub struct ComputerSpeaker {
    player: Arc<Mutex<Option<Result<Arc<rodio::Player>, String>>>>,
}

impl ComputerSpeaker {
    pub fn new() -> Self {
        Self::default()
    }

    fn player(&self) -> Result<Arc<rodio::Player>, String> {
        let mut slot = self.player.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(open_output());
        }
        slot.clone().unwrap_or_else(|| Err("no sound output".into()))
    }
}

/// The audio stream must stay on the thread that opened it, so a small
/// thread opens it and keeps it alive for the rest of the program.
fn open_output() -> Result<Arc<rodio::Player>, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("lego-lab-audio".into())
        .spawn(move || match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut sink) => {
                sink.log_on_drop(false);
                let _ = tx.send(Ok(Arc::new(rodio::Player::connect_new(sink.mixer()))));
                loop {
                    std::thread::park();
                }
            }
            Err(e) => {
                let _ = tx.send(Err(format!("this computer has no sound output ({e})")));
            }
        })
        .map_err(|e| format!("could not start sound ({e})"))?;
    rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| Err("the computer's sound didn't start".into()))
}

impl Speaker for ComputerSpeaker {
    fn play(&self, notes: &[(u16, u32)], volume: u32) -> Result<(), String> {
        let player = self.player()?;
        let channels = std::num::NonZero::new(1).expect("1 is not zero");
        let rate = std::num::NonZero::new(SAMPLE_RATE).expect("sample rate is not zero");
        let data: Vec<rodio::Sample> = samples(notes, volume).into_iter().map(|s| s as rodio::Sample).collect();
        player.append(rodio::buffer::SamplesBuffer::new(channels, rate, data));
        player.play();
        Ok(())
    }

    fn stop(&self) {
        let slot = self.player.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(Ok(player)) = slot.as_ref() {
            player.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beep_patterns_have_the_right_shape() {
        let tone = |pattern, count| Tone { pattern, frequency: 1000, count };
        assert_eq!(beep_notes(tone(BeepPattern::Single, 1)), vec![(1000, 200)]);
        assert_eq!(beep_notes(tone(BeepPattern::Double, 1)).iter().filter(|n| n.0 > 0).count(), 2);
        let twice = beep_notes(tone(BeepPattern::Triple, 2));
        assert_eq!(twice.iter().filter(|n| n.0 > 0).count(), 6);
        let up = beep_notes(tone(BeepPattern::UpMiddleDown, 1));
        assert!(up[0].0 > up[1].0 && up[1].0 > up[2].0);
        assert_eq!(length(&beep_notes(tone(BeepPattern::Single, 3))), Duration::from_millis(900));
    }

    #[test]
    fn volume_scales_the_samples() {
        let loud = samples(&[(440, 50)], 100);
        let quiet = samples(&[(440, 50)], 25);
        let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak(&loud) - FULL_SCALE).abs() < 1e-6);
        assert!((peak(&quiet) - FULL_SCALE / 4.0).abs() < 1e-6);
        assert!(samples(&[(440, 50)], 0).iter().all(|s| *s == 0.0));
        assert_eq!(loud[0], 0.0, "fades in");
    }
}
