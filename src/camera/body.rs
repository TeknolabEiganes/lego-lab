//! Body sensing, like LEGO's Coding Canvas: where a body point is, how far
//! apart two points are, the angle between them, and whether they touch.
//!
//! Positions use Coding Canvas's units: the middle of the picture is 0, the
//! top and bottom edges are +45 and −45, and x runs the same way across the
//! picture (−60 to 60 for a normal 4:3 webcam). As in the camera preview,
//! the picture is mirrored, so x grows to the right of the screen.

use serde::{Deserialize, Serialize};

use super::pose::{MIN_SCORE, Pose};

/// Units from the middle of the picture to its top edge.
const HALF_HEIGHT: f32 = 45.0;
/// Points closer than this touch (Coding Canvas's value).
pub const TOUCH_DISTANCE: f32 = 20.0;
pub const MAX_DISTANCE: i32 = 200;
pub const MAX_POSITION: i32 = 100;

/// The 17 body points MoveNet finds, in its order. The wrists stand for
/// the hands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyPoint {
    Nose,
    LeftEye,
    RightEye,
    LeftEar,
    RightEar,
    LeftShoulder,
    RightShoulder,
    LeftElbow,
    RightElbow,
    LeftWrist,
    RightWrist,
    LeftHip,
    RightHip,
    LeftKnee,
    RightKnee,
    LeftAnkle,
    RightAnkle,
}

impl BodyPoint {
    pub const ALL: [BodyPoint; 17] = [
        BodyPoint::Nose,
        BodyPoint::LeftEye,
        BodyPoint::RightEye,
        BodyPoint::LeftEar,
        BodyPoint::RightEar,
        BodyPoint::LeftShoulder,
        BodyPoint::RightShoulder,
        BodyPoint::LeftElbow,
        BodyPoint::RightElbow,
        BodyPoint::LeftWrist,
        BodyPoint::RightWrist,
        BodyPoint::LeftHip,
        BodyPoint::RightHip,
        BodyPoint::LeftKnee,
        BodyPoint::RightKnee,
        BodyPoint::LeftAnkle,
        BodyPoint::RightAnkle,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            BodyPoint::Nose => "nose",
            BodyPoint::LeftEye => "left eye",
            BodyPoint::RightEye => "right eye",
            BodyPoint::LeftEar => "left ear",
            BodyPoint::RightEar => "right ear",
            BodyPoint::LeftShoulder => "left shoulder",
            BodyPoint::RightShoulder => "right shoulder",
            BodyPoint::LeftElbow => "left elbow",
            BodyPoint::RightElbow => "right elbow",
            BodyPoint::LeftWrist => "left wrist",
            BodyPoint::RightWrist => "right wrist",
            BodyPoint::LeftHip => "left hip",
            BodyPoint::RightHip => "right hip",
            BodyPoint::LeftKnee => "left knee",
            BodyPoint::RightKnee => "right knee",
            BodyPoint::LeftAnkle => "left ankle",
            BodyPoint::RightAnkle => "right ankle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    pub fn label(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
        }
    }
}

impl Pose {
    /// Where a body point is, in Coding Canvas units (y up, x mirrored),
    /// if the model is sure about it.
    pub fn position(&self, point: BodyPoint) -> Option<(f32, f32)> {
        let k = self.points.get(point.index()).filter(|k| k.score >= MIN_SCORE)?;
        Some(((0.5 - k.x) * 2.0 * HALF_HEIGHT * self.aspect, (0.5 - k.y) * 2.0 * HALF_HEIGHT))
    }

    pub fn coordinate(&self, axis: Axis, point: BodyPoint) -> Option<f32> {
        let (x, y) = self.position(point)?;
        Some(match axis {
            Axis::X => x,
            Axis::Y => y,
        })
    }

    pub fn distance(&self, one: BodyPoint, two: BodyPoint) -> Option<f32> {
        let (a, b) = (self.position(one)?, self.position(two)?);
        Some((a.0 - b.0).hypot(a.1 - b.1))
    }

    /// Direction from `one` to `two` in degrees, 0–359: 0° points right,
    /// 90° up, 180° left and 270° down.
    pub fn angle(&self, one: BodyPoint, two: BodyPoint) -> Option<f32> {
        let (a, b) = (self.position(one)?, self.position(two)?);
        Some((b.1 - a.1).atan2(b.0 - a.0).to_degrees().rem_euclid(360.0))
    }

    pub fn touching(&self, one: BodyPoint, two: BodyPoint) -> bool {
        one != two && self.distance(one, two).is_some_and(|d| d < TOUCH_DISTANCE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::pose::tests::standing;

    #[test]
    fn measures_like_coding_canvas() {
        let pose = standing(false, false);
        // The nose is at the top middle of the picture.
        let (x, y) = pose.position(BodyPoint::Nose).unwrap();
        assert!(x.abs() < 0.01 && (y - 27.0).abs() < 0.01, "{x} {y}");
        // In the mirrored picture, like a mirror, the left shoulder is on the left.
        assert!(pose.coordinate(Axis::X, BodyPoint::LeftShoulder).unwrap() < 0.0);
        let shoulders = pose.distance(BodyPoint::LeftShoulder, BodyPoint::RightShoulder).unwrap();
        assert!((shoulders - 0.2 * 90.0 * 4.0 / 3.0).abs() < 0.01);
        let angle = pose.angle(BodyPoint::LeftShoulder, BodyPoint::RightShoulder).unwrap();
        assert!(angle < 0.01 || angle > 359.99, "left shoulder → right shoulder points right: {angle}");
        let down = pose.angle(BodyPoint::Nose, BodyPoint::LeftHip).unwrap();
        assert!(down > 240.0 && down < 270.0, "down and a bit left: {down}");
    }

    #[test]
    fn hands_touch() {
        let mut pose = standing(false, false);
        assert!(!pose.touching(BodyPoint::LeftWrist, BodyPoint::RightWrist));
        pose.points[BodyPoint::LeftWrist.index()].x = 0.52;
        pose.points[BodyPoint::RightWrist.index()].x = 0.48;
        assert!(pose.touching(BodyPoint::LeftWrist, BodyPoint::RightWrist));
        assert!(!pose.touching(BodyPoint::LeftWrist, BodyPoint::LeftWrist), "a point doesn't touch itself");
        pose.points[BodyPoint::RightWrist.index()].score = 0.1;
        assert!(!pose.touching(BodyPoint::LeftWrist, BodyPoint::RightWrist), "unsure points don't touch");
    }
}
