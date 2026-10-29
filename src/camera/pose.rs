//! Body poses: keypoints from the pose model, the built-in poses (arms up)
//! and recognising poses the children trained themselves.
//!
//! This follows LEGO's Coding Canvas "Pose Classification": MoveNet finds 17
//! body points, and a pose is either one of the built-in ones or one of the
//! user's own classes, learned from examples.

use serde::{Deserialize, Serialize};

pub const KEYPOINTS: usize = 17;

pub const LEFT_SHOULDER: usize = 5;
pub const RIGHT_SHOULDER: usize = 6;
pub const LEFT_WRIST: usize = 9;
pub const RIGHT_WRIST: usize = 10;

/// Lines of the stick figure, as keypoint index pairs.
pub const SKELETON: [(usize, usize); 16] = [
    (0, 1),
    (0, 2),
    (1, 3),
    (2, 4),
    (5, 6),
    (5, 7),
    (7, 9),
    (6, 8),
    (8, 10),
    (5, 11),
    (6, 12),
    (11, 12),
    (11, 13),
    (13, 15),
    (12, 14),
    (14, 16),
];

/// A keypoint is trusted above this score.
pub const MIN_SCORE: f32 = 0.3;
/// A person is in view when at least this many keypoints are trusted.
const MIN_PERSON_POINTS: usize = 5;
/// The most poses (classes) a project can train.
pub const MAX_CLASSES: usize = 8;
/// The most examples one pose can hold.
pub const MAX_EXAMPLES: usize = 200;
/// Fraction of the nearest examples that must agree on a pose.
const MATCH_CONFIDENCE: f32 = 0.6;
const NEIGHBOURS: usize = 5;

/// One body point. `x` and `y` are 0–1 across the camera image (not
/// mirrored), `score` is how sure the model is (0–1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f32; 3]", into = "[f32; 3]")]
pub struct Keypoint {
    pub x: f32,
    pub y: f32,
    pub score: f32,
}

impl From<[f32; 3]> for Keypoint {
    fn from([x, y, score]: [f32; 3]) -> Self {
        let clean = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        Keypoint { x: clean(x), y: clean(y), score: clean(score) }
    }
}

impl From<Keypoint> for [f32; 3] {
    fn from(k: Keypoint) -> Self {
        // Three decimals is far finer than the model's accuracy and keeps
        // project files small.
        let round = |v: f32| (v * 1000.0).round() / 1000.0;
        [round(k.x), round(k.y), round(k.score)]
    }
}

/// All 17 body points of one person in one camera frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    /// Image width / height, so distances can be measured in equal units.
    pub aspect: f32,
    pub points: Vec<Keypoint>,
}

impl Pose {
    pub fn new(aspect: f32, points: [Keypoint; KEYPOINTS]) -> Self {
        Pose { aspect, points: points.to_vec() }
    }

    fn valid(&self) -> bool {
        self.points.len() == KEYPOINTS && self.aspect.is_finite() && self.aspect > 0.1 && self.aspect < 10.0
    }

    fn point(&self, i: usize) -> Option<Keypoint> {
        self.points.get(i).copied().filter(|k| k.score >= MIN_SCORE)
    }

    /// Is somebody in view?
    pub fn has_person(&self) -> bool {
        self.points.iter().filter(|k| k.score >= MIN_SCORE).count() >= MIN_PERSON_POINTS
    }

    /// Blend towards `next` to steady the jitter between frames.
    pub fn smoothed(&self, next: &Pose, amount: f32) -> Pose {
        let points = self
            .points
            .iter()
            .zip(&next.points)
            .map(|(a, b)| Keypoint {
                x: a.x + (b.x - a.x) * amount,
                y: a.y + (b.y - a.y) * amount,
                score: b.score,
            })
            .collect();
        Pose { aspect: next.aspect, points }
    }

    /// The person's own left/right arm is raised (wrist clearly above the
    /// shoulder).
    fn arm_up(&self, shoulder: usize, wrist: usize) -> bool {
        let (Some(s), Some(w)) = (self.point(shoulder), self.point(wrist)) else { return false };
        w.y < s.y - 0.5 * self.shoulder_width()
    }

    /// Shoulder-to-shoulder distance in image heights, with a floor so a
    /// side-on person still gets a sensible scale.
    fn shoulder_width(&self) -> f32 {
        match (self.point(LEFT_SHOULDER), self.point(RIGHT_SHOULDER)) {
            (Some(l), Some(r)) => (((l.x - r.x) * self.aspect).hypot(l.y - r.y)).max(0.08),
            _ => 0.15,
        }
    }

    /// Which built-in pose this is, if a person is in view.
    pub fn builtin(&self) -> Option<CameraPose> {
        if !self.has_person() {
            return None;
        }
        Some(match (self.arm_up(LEFT_SHOULDER, LEFT_WRIST), self.arm_up(RIGHT_SHOULDER, RIGHT_WRIST)) {
            (true, true) => CameraPose::BothArmsUp,
            (true, false) => CameraPose::LeftArmUp,
            (false, true) => CameraPose::RightArmUp,
            (false, false) => CameraPose::ArmsDown,
        })
    }

    /// Numbers describing the pose for comparing with trained examples:
    /// every point relative to the middle of the shoulders, in shoulder
    /// widths, so it doesn't matter where in the picture the person stands
    /// or how close they are. Points the model isn't sure about are zero.
    pub fn features(&self) -> Vec<f32> {
        let centre = match (self.point(LEFT_SHOULDER), self.point(RIGHT_SHOULDER)) {
            (Some(l), Some(r)) => ((l.x + r.x) / 2.0, (l.y + r.y) / 2.0),
            _ => {
                let seen: Vec<Keypoint> = self.points.iter().copied().filter(|k| k.score >= MIN_SCORE).collect();
                let n = seen.len().max(1) as f32;
                (seen.iter().map(|k| k.x).sum::<f32>() / n, seen.iter().map(|k| k.y).sum::<f32>() / n)
            }
        };
        let scale = self.shoulder_width();
        let mut out = Vec::with_capacity(KEYPOINTS * 3);
        for i in 0..KEYPOINTS {
            match self.point(i) {
                Some(k) => {
                    out.push((k.x - centre.0) * self.aspect / scale);
                    out.push((k.y - centre.1) / scale);
                    out.push(1.0);
                }
                None => out.extend([0.0, 0.0, 0.0]),
            }
        }
        out
    }
}

/// What a camera condition looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CameraPose {
    Person,
    NoPerson,
    ArmsDown,
    LeftArmUp,
    RightArmUp,
    BothArmsUp,
    /// One of the project's own trained poses, by position in the list.
    Custom(u8),
}

impl CameraPose {
    pub const BUILTIN: [CameraPose; 6] = [
        CameraPose::Person,
        CameraPose::NoPerson,
        CameraPose::ArmsDown,
        CameraPose::LeftArmUp,
        CameraPose::RightArmUp,
        CameraPose::BothArmsUp,
    ];

    /// Everything a block can pick, given the project's trained pose names.
    pub fn options(custom: &[String]) -> Vec<CameraPose> {
        let mut out = Self::BUILTIN.to_vec();
        out.extend((0..custom.len().min(MAX_CLASSES)).map(|i| CameraPose::Custom(i as u8)));
        out
    }

    pub fn label(self, custom: &[String]) -> String {
        match self {
            CameraPose::Person => "a person".into(),
            CameraPose::NoPerson => "no person".into(),
            CameraPose::ArmsDown => "arms down".into(),
            CameraPose::LeftArmUp => "left arm up".into(),
            CameraPose::RightArmUp => "right arm up".into(),
            CameraPose::BothArmsUp => "both arms up".into(),
            CameraPose::Custom(i) => match custom.get(i as usize) {
                Some(name) => format!("“{name}”"),
                None => format!("pose {} (not trained)", i + 1),
            },
        }
    }

    pub fn sanitize(&mut self) {
        if let CameraPose::Custom(i) = self {
            *i = (*i).min(MAX_CLASSES as u8 - 1);
        }
    }
}

/// A pose the children taught the camera, with the examples they recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoseClass {
    pub name: String,
    #[serde(default)]
    pub examples: Vec<Pose>,
}

impl PoseClass {
    pub fn new(name: impl Into<String>) -> Self {
        PoseClass { name: name.into(), examples: Vec::new() }
    }
}

/// Repair trained poses loaded from a file.
pub fn sanitize_classes(classes: &mut Vec<PoseClass>) {
    classes.truncate(MAX_CLASSES);
    for class in classes.iter_mut() {
        class.name = class.name.chars().filter(|c| !c.is_control()).take(40).collect();
        if class.name.trim().is_empty() {
            class.name = "pose".into();
        }
        class.examples.retain(Pose::valid);
        class.examples.truncate(MAX_EXAMPLES);
    }
}

/// What the camera sees in one frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Detection {
    pub person: bool,
    /// Built-in pose, when a person is in view.
    pub builtin: Option<CameraPose>,
    /// Index of the matching trained pose, if one matches well enough.
    pub custom: Option<usize>,
    /// The body points, when a person is in view (for body sensing).
    pub pose: Option<Pose>,
}

impl Detection {
    pub fn from_pose(pose: Option<&Pose>, classes: &[PoseClass]) -> Self {
        let Some(pose) = pose.filter(|p| p.has_person()) else { return Detection::default() };
        Detection {
            person: true,
            builtin: pose.builtin(),
            custom: classify(pose, classes).map(|(i, _)| i),
            pose: Some(pose.clone()),
        }
    }

    pub fn matches(&self, pose: CameraPose) -> bool {
        match pose {
            CameraPose::Person => self.person,
            CameraPose::NoPerson => !self.person,
            CameraPose::Custom(i) => self.custom == Some(i as usize),
            builtin => self.builtin == Some(builtin),
        }
    }
}

/// Which trained pose `pose` looks most like: the majority of its nearest
/// recorded examples, with how many of them agreed (0–1). Needs at least
/// two poses with examples, otherwise everything would match the only one.
pub fn classify(pose: &Pose, classes: &[PoseClass]) -> Option<(usize, f32)> {
    if classes.iter().filter(|c| !c.examples.is_empty()).count() < 2 {
        return None;
    }
    let features = pose.features();
    let mut nearest: Vec<(f32, usize)> = classes
        .iter()
        .enumerate()
        .flat_map(|(ci, c)| c.examples.iter().map(move |e| (ci, e)))
        .map(|(ci, e)| {
            let d: f32 = e.features().iter().zip(&features).map(|(a, b)| (a - b) * (a - b)).sum();
            (d, ci)
        })
        .collect();
    nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
    nearest.truncate(NEIGHBOURS);
    let mut votes = vec![0usize; classes.len()];
    for &(_, ci) in &nearest {
        votes[ci] += 1;
    }
    let (best, &count) = votes.iter().enumerate().max_by_key(|&(_, v)| *v)?;
    let confidence = count as f32 / nearest.len() as f32;
    (confidence >= MATCH_CONFIDENCE).then_some((best, confidence))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const NOSE: usize = 0;
    const LEFT_HIP: usize = 11;
    const RIGHT_HIP: usize = 12;

    /// A person facing the camera. Their left side is on the image's right.
    pub fn standing(left_arm_up: bool, right_arm_up: bool) -> Pose {
        let mut p = [Keypoint { x: 0.5, y: 0.5, score: 0.9 }; KEYPOINTS];
        let at = |x: f32, y: f32| Keypoint { x, y, score: 0.9 };
        p[NOSE] = at(0.5, 0.2);
        p[LEFT_SHOULDER] = at(0.6, 0.35);
        p[RIGHT_SHOULDER] = at(0.4, 0.35);
        p[LEFT_HIP] = at(0.57, 0.7);
        p[RIGHT_HIP] = at(0.43, 0.7);
        p[7] = at(0.65, if left_arm_up { 0.22 } else { 0.5 });
        p[8] = at(0.35, if right_arm_up { 0.22 } else { 0.5 });
        p[LEFT_WRIST] = at(0.66, if left_arm_up { 0.08 } else { 0.62 });
        p[RIGHT_WRIST] = at(0.34, if right_arm_up { 0.08 } else { 0.62 });
        Pose::new(4.0 / 3.0, p)
    }

    fn nobody() -> Pose {
        Pose::new(4.0 / 3.0, [Keypoint { x: 0.5, y: 0.5, score: 0.05 }; KEYPOINTS])
    }

    #[test]
    fn recognises_built_in_poses() {
        assert_eq!(standing(false, false).builtin(), Some(CameraPose::ArmsDown));
        assert_eq!(standing(true, false).builtin(), Some(CameraPose::LeftArmUp));
        assert_eq!(standing(false, true).builtin(), Some(CameraPose::RightArmUp));
        assert_eq!(standing(true, true).builtin(), Some(CameraPose::BothArmsUp));
        assert_eq!(nobody().builtin(), None);

        let seen = Detection::from_pose(Some(&standing(true, false)), &[]);
        assert!(seen.matches(CameraPose::Person));
        assert!(seen.matches(CameraPose::LeftArmUp));
        assert!(!seen.matches(CameraPose::BothArmsUp));
        assert!(!seen.matches(CameraPose::NoPerson));
        assert!(Detection::from_pose(Some(&nobody()), &[]).matches(CameraPose::NoPerson));
        assert!(Detection::from_pose(None, &[]).matches(CameraPose::NoPerson));
    }

    #[test]
    fn features_ignore_where_the_person_stands() {
        let a = standing(true, false);
        let mut b = a.clone();
        for k in &mut b.points {
            k.x -= 0.2;
        }
        let (fa, fb) = (a.features(), b.features());
        assert!(fa.iter().zip(&fb).all(|(x, y)| (x - y).abs() < 1e-4));
    }

    #[test]
    fn classifies_trained_poses() {
        let mut star = PoseClass::new("star");
        let mut rest = PoseClass::new("rest");
        star.examples = vec![standing(true, true); 3];
        rest.examples = vec![standing(false, false); 3];
        let classes = vec![rest, star];
        assert_eq!(classify(&standing(true, true), &classes).map(|c| c.0), Some(1));
        assert_eq!(classify(&standing(false, false), &classes).map(|c| c.0), Some(0));

        let only_one = vec![classes[0].clone()];
        assert_eq!(classify(&standing(false, false), &only_one), None, "one pose can't be told apart");

        let seen = Detection::from_pose(Some(&standing(true, true)), &classes);
        assert!(seen.matches(CameraPose::Custom(1)));
        assert!(!seen.matches(CameraPose::Custom(0)));
    }

    #[test]
    fn sanitize_drops_broken_examples() {
        let json = r#"[{"name":"","examples":[{"aspect":1.3,"points":[[0.1,0.2,0.9]]},
            {"aspect":1.3,"points":[[0.1,2.0,0.9],[0,0,0],[0,0,0],[0,0,0],[0,0,0],[0,0,0],[0,0,0],[0,0,0],
            [0,0,0],[0,0,0],[0,0,0],[0,0,0],[0,0,0],[0,0,0],[0,0,0],[0,0,0],[0,0,0]]}]}]"#;
        let mut classes: Vec<PoseClass> = serde_json::from_str(json).unwrap();
        sanitize_classes(&mut classes);
        assert_eq!(classes[0].name, "pose");
        assert_eq!(classes[0].examples.len(), 1, "wrong number of points removed");
        assert_eq!(classes[0].examples[0].points[0].y, 1.0, "clamped into the image");
    }
}
