//! Webcam capture through Video4Linux.

use std::io;

use v4l::buffer::Type;
use v4l::io::mmap::Stream;
use v4l::io::traits::CaptureStream;
use v4l::video::Capture;
use v4l::{Device, FourCC, Format};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

/// An RGB picture, 3 bytes per pixel, rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

enum PixelFormat {
    Yuyv,
    Mjpeg,
}

pub struct Webcam {
    stream: Stream<'static>,
    format: PixelFormat,
    width: usize,
    height: usize,
    pub name: String,
}

impl Webcam {
    /// Open the first camera that can stream pictures.
    pub fn open() -> Result<Webcam, String> {
        let mut nodes = v4l::context::enum_devices();
        nodes.sort_by_key(|n| n.index());
        if nodes.is_empty() {
            return Err("no camera found. Plug in a webcam and try again.".into());
        }
        let mut last_error = String::new();
        for node in nodes {
            match Self::open_path(node.path()) {
                Ok(mut cam) => {
                    if let Some(name) = node.name() {
                        cam.name = name;
                    }
                    return Ok(cam);
                }
                Err(e) => last_error = format!("{}: {e}", node.path().display()),
            }
        }
        Err(format!("could not open the camera ({last_error})"))
    }

    fn open_path(path: &std::path::Path) -> io::Result<Webcam> {
        let device = Device::with_path(path)?;
        let mut chosen = None;
        // Uncompressed YUYV first: every webcam has it and it needs no decoding.
        for (fourcc, format) in [(b"YUYV", PixelFormat::Yuyv), (b"MJPG", PixelFormat::Mjpeg)] {
            let wanted = Format::new(WIDTH, HEIGHT, FourCC::new(fourcc));
            if let Ok(actual) = device.set_format(&wanted)
                && actual.fourcc == wanted.fourcc
            {
                chosen = Some((actual, format));
                break;
            }
        }
        let Some((actual, format)) = chosen else {
            return Err(io::Error::other("not a camera with a supported picture format"));
        };
        // The stream keeps its own handle to the device, which closes when
        // the stream is dropped.
        let mut stream = Stream::with_buffers(&device, Type::VideoCapture, 4)?;
        stream.set_timeout(std::time::Duration::from_secs(3));
        Ok(Webcam {
            stream,
            format,
            width: actual.width as usize,
            height: actual.height as usize,
            name: path.display().to_string(),
        })
    }

    pub fn next_frame(&mut self) -> Result<Frame, String> {
        let (buf, meta) = self.stream.next().map_err(|e| format!("the camera stopped sending pictures ({e})"))?;
        let used = (meta.bytesused as usize).min(buf.len());
        let data = if used > 0 { &buf[..used] } else { buf };
        match self.format {
            PixelFormat::Yuyv => yuyv_to_rgb(data, self.width, self.height),
            PixelFormat::Mjpeg => {
                let img = image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
                    .map_err(|e| format!("could not decode a camera picture ({e})"))?
                    .to_rgb8();
                Ok(Frame { width: img.width() as usize, height: img.height() as usize, rgb: img.into_raw() })
            }
        }
    }
}

fn yuyv_to_rgb(data: &[u8], width: usize, height: usize) -> Result<Frame, String> {
    if data.len() < width * height * 2 {
        return Err("the camera sent an incomplete picture".into());
    }
    let mut rgb = Vec::with_capacity(width * height * 3);
    for px in data[..width * height * 2].chunks_exact(4) {
        let (y0, u, y1, v) = (px[0] as f32, px[1] as f32 - 128.0, px[2] as f32, px[3] as f32 - 128.0);
        for y in [y0, y1] {
            rgb.push((y + 1.402 * v).clamp(0.0, 255.0) as u8);
            rgb.push((y - 0.344 * u - 0.714 * v).clamp(0.0, 255.0) as u8);
            rgb.push((y + 1.772 * u).clamp(0.0, 255.0) as u8);
        }
    }
    Ok(Frame { width, height, rgb })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_yuyv() {
        // Two pixels: white and black, no colour.
        let frame = yuyv_to_rgb(&[255, 128, 0, 128], 2, 1).unwrap();
        assert_eq!(frame.rgb, vec![255, 255, 255, 0, 0, 0]);
        assert!(yuyv_to_rgb(&[0; 3], 2, 1).is_err());
    }
}
