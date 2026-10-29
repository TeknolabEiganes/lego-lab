//! Notes for the Beep block and built-in melodies for the Play Tune block.
//!
//! The devices only play fixed-length beeps, so a tune is a series of
//! single beeps that the executor spaces out in time.

use serde::{Deserialize, Serialize};

/// Notes offered by the Beep block, as MIDI numbers: the white keys from C4
/// to C7 (C7 ≈ 2093 Hz, below the devices' 2700 Hz limit).
pub const NOTES: [u8; 22] = [
    60, 62, 64, 65, 67, 69, 71, // C4–B4
    72, 74, 76, 77, 79, 81, 83, // C5–B5
    84, 86, 88, 89, 91, 93, 95, // C6–B6
    96, // C7
];

/// A4, 440 Hz: what the Beep block always played before notes existed.
pub const DEFAULT_NOTE: u8 = 69;
/// Note of a new Beep block: C7, near the top of the range, where the
/// devices' small beepers are loudest.
pub const PALETTE_NOTE: u8 = 96;

/// Frequency of a MIDI note in Hz (A4 = 69 = 440 Hz).
pub fn note_hz(note: u8) -> u16 {
    (440.0 * 2f64.powf((note as f64 - 69.0) / 12.0)).round() as u16
}

/// Note name such as "C4" or "F#5".
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[note as usize % 12], note as i32 / 12 - 1)
}

/// The nearest offered note, for repairing loaded files.
pub fn nearest_note(note: u8) -> u8 {
    *NOTES.iter().min_by_key(|n| (**n as i32 - note as i32).abs()).unwrap_or(&DEFAULT_NOTE)
}

/// Length of one beat in a tune.
const BEAT_MS: u64 = 220;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Tune {
    #[default]
    Twinkle,
    OdeToJoy,
    MaryLamb,
    TaDa,
    SadTrombone,
    Siren,
}

/// One note of a tune: how long until the next note starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuneNote {
    pub frequency: u16,
    pub milliseconds: u64,
}

impl Tune {
    pub const ALL: [Tune; 6] = [Tune::Twinkle, Tune::OdeToJoy, Tune::MaryLamb, Tune::TaDa, Tune::SadTrombone, Tune::Siren];

    pub fn label(self) -> &'static str {
        match self {
            Tune::Twinkle => "Twinkle Twinkle",
            Tune::OdeToJoy => "Ode to Joy",
            Tune::MaryLamb => "Mary's Little Lamb",
            Tune::TaDa => "Ta-da!",
            Tune::SadTrombone => "Sad trombone",
            Tune::Siren => "Siren",
        }
    }

    /// (MIDI note, length in half beats).
    fn score(self) -> &'static [(u8, u8)] {
        match self {
            Tune::Twinkle => &[
                (72, 2), (72, 2), (79, 2), (79, 2), (81, 2), (81, 2), (79, 4),
                (77, 2), (77, 2), (76, 2), (76, 2), (74, 2), (74, 2), (72, 4),
            ],
            Tune::OdeToJoy => &[
                (76, 2), (76, 2), (77, 2), (79, 2), (79, 2), (77, 2), (76, 2), (74, 2),
                (72, 2), (72, 2), (74, 2), (76, 2), (76, 3), (74, 1), (74, 4),
            ],
            Tune::MaryLamb => &[
                (76, 2), (74, 2), (72, 2), (74, 2), (76, 2), (76, 2), (76, 4),
                (74, 2), (74, 2), (74, 4), (76, 2), (79, 2), (79, 4),
            ],
            Tune::TaDa => &[(72, 1), (76, 1), (79, 1), (84, 4)],
            Tune::SadTrombone => &[(79, 2), (78, 2), (77, 2), (76, 6)],
            Tune::Siren => &[(81, 2), (76, 2), (81, 2), (76, 2), (81, 2), (76, 2), (81, 2), (76, 2)],
        }
    }

    pub fn notes(self) -> Vec<TuneNote> {
        self.score()
            .iter()
            .map(|&(note, half_beats)| TuneNote { frequency: note_hz(note), milliseconds: half_beats as u64 * BEAT_MS / 2 })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::MAX_BEEP_HZ;

    #[test]
    fn note_frequencies_and_names() {
        assert_eq!(note_hz(69), 440);
        assert_eq!(note_hz(60), 262);
        assert_eq!(note_hz(72), 523);
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(78), "F#5");
        assert_eq!(nearest_note(61), 60);
        assert_eq!(nearest_note(200), 96);
    }

    #[test]
    fn every_note_is_playable() {
        for n in NOTES {
            assert!(note_hz(n) <= MAX_BEEP_HZ);
        }
        for tune in Tune::ALL {
            assert!(!tune.notes().is_empty());
            assert!(tune.notes().iter().all(|n| n.frequency <= MAX_BEEP_HZ && n.milliseconds > 0));
        }
    }
}
