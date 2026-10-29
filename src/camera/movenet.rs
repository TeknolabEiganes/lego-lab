//! MoveNet SinglePose Lightning (Google, Apache-2.0): finds 17 body points
//! in a picture. The same model LEGO's Coding Canvas uses, run here with
//! `tract` on the CPU (about 20 ms per frame).

use std::sync::{Arc, OnceLock};

use tract_onnx::prelude::*;

use super::capture::Frame;
use super::pose::{KEYPOINTS, Keypoint, Pose};

static MODEL_BYTES: &[u8] = include_bytes!("../../assets/movenet-singlepose-lightning.onnx");
const INPUT: usize = 192;

pub struct MoveNet {
    plan: Arc<TypedRunnableModel>,
}

impl MoveNet {
    /// Loaded once and shared; turning the camera off and on is instant.
    pub fn shared() -> Result<&'static MoveNet, String> {
        static MODEL: OnceLock<Result<MoveNet, String>> = OnceLock::new();
        MODEL.get_or_init(|| MoveNet::load().map_err(|e| format!("could not load the pose model: {e}"))).as_ref().map_err(Clone::clone)
    }

    fn load() -> TractResult<MoveNet> {
        let plan = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(MODEL_BYTES))?
            .with_input_fact(0, i32::fact([1, INPUT, INPUT, 3]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(MoveNet { plan })
    }

    /// Find the body points in a frame. The picture is scaled to fit a
    /// square with black bars, like MoveNet expects, and the points are
    /// mapped back to the frame.
    pub fn detect(&self, frame: &Frame) -> TractResult<Pose> {
        let (w, h) = (frame.width as f32, frame.height as f32);
        let side = w.max(h);
        let (pad_x, pad_y) = ((side - w) / 2.0, (side - h) / 2.0);
        let step = side / INPUT as f32;
        let input = tract_ndarray::Array4::from_shape_fn((1, INPUT, INPUT, 3), |(_, y, x, c)| {
            let sx = (x as f32 + 0.5) * step - pad_x;
            let sy = (y as f32 + 0.5) * step - pad_y;
            if sx < 0.0 || sy < 0.0 || sx >= w || sy >= h {
                0
            } else {
                frame.rgb[(sy as usize * frame.width + sx as usize) * 3 + c] as i32
            }
        });
        let out = self.plan.run(tvec!(Tensor::from(input).into()))?;
        let raw = out[0].to_plain_array_view::<f32>()?;
        let raw: Vec<f32> = raw.iter().copied().collect();
        anyhow::ensure!(raw.len() >= KEYPOINTS * 3, "unexpected pose model output");
        let mut points = [Keypoint::default(); KEYPOINTS];
        for (i, p) in points.iter_mut().enumerate() {
            // MoveNet gives (y, x, score) in the padded square.
            let (y, x, score) = (raw[i * 3], raw[i * 3 + 1], raw[i * 3 + 2]);
            *p = Keypoint::from([(x * side - pad_x) / w, (y * side - pad_y) / h, score]);
        }
        Ok(Pose::new(w / h, points))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_loads_and_sees_nobody_in_a_blank_picture() {
        let model = MoveNet::shared().unwrap();
        let frame = Frame { width: 64, height: 48, rgb: vec![128; 64 * 48 * 3] };
        let pose = model.detect(&frame).unwrap();
        assert_eq!(pose.points.len(), KEYPOINTS);
        assert!(!pose.has_person());
    }
}
