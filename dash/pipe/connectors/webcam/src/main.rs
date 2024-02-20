use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, NaiveDateTime, Utc};
use clap::Parser;
use dash_openapi::image::{ImageCodec, ImageSize};
use dash_pipe_provider::{
    storage::StorageIO, FunctionContext, PipeArgs, PipeMessage, PipeMessages, PipePayload,
};
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use tokio::{spawn, sync::mpsc};
use v4l::{
    buffer::{Metadata, Type as BufferType},
    io::{mmap::Stream, traits::CaptureStream},
    video::Capture,
    Device, FourCC,
};

fn main() {
    PipeArgs::<Function>::from_env().loop_forever()
}

#[derive(Clone, Debug, Serialize, Deserialize, Parser)]
pub struct FunctionArgs {
    #[arg(
        long,
        env = "PIPE_WEBCAM_CAMERA_DEVICE",
        value_name = "INDEX",
        default_value_t = FunctionArgs::default_camera_device()
    )]
    #[serde(default = "FunctionArgs::default_camera_device")]
    camera_device: usize,

    #[arg(
        long,
        env = "PIPE_WEBCAM_CAMERA_BUFFER_SIZE",
        value_name = "INDEX",
        default_value_t = FunctionArgs::default_camera_buffer_size()
    )]
    #[serde(default = "FunctionArgs::default_camera_buffer_size")]
    camera_buffer_size: usize,

    #[arg(
        long,
        env = "PIPE_WEBCAM_CAMERA_CODEC",
        value_name = "TYPE",
        value_enum,
        default_value_t = Default::default()
    )]
    #[serde(default)]
    camera_codec: ImageCodec,

    #[arg(
        long,
        env = "PIPE_WEBCAM_CAMERA_FPS",
        value_name = "FPS",
        default_value_t = FunctionArgs::default_camera_fps()
    )]
    #[serde(default = "FunctionArgs::default_camera_fps")]
    camera_fps: f64,

    #[arg(
        long,
        env = "PIPE_WEBCAM_CAMERA_WIDTH",
        value_name = "SIZE",
        default_value_t = FunctionArgs::default_camera_width()
    )]
    #[serde(default = "FunctionArgs::default_camera_width")]
    camera_width: u32,

    #[arg(
        long,
        env = "PIPE_WEBCAM_CAMERA_HEIGHT",
        value_name = "SIZE",
        default_value_t = FunctionArgs::default_camera_height()
    )]
    #[serde(default = "FunctionArgs::default_camera_height")]
    camera_height: u32,
}

impl FunctionArgs {
    const fn default_camera_device() -> usize {
        0
    }

    const fn default_camera_buffer_size() -> usize {
        4
    }

    const fn default_camera_fps() -> f64 {
        60.0
    }

    const fn default_camera_width() -> u32 {
        1920
    }

    const fn default_camera_height() -> u32 {
        1080
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Function {
    camera_codec: ImageCodec,
    #[derivative(Debug = "ignore")]
    capture: mpsc::Receiver<Result<(Bytes, Metadata), String>>,
    ctx: FunctionContext,
    frame_counter: FrameCounter,
    frame_size: ImageSize,
}

pub type FunctionOutput = ::dash_openapi::image::Image;

#[async_trait]
impl ::dash_pipe_provider::FunctionBuilder for Function {
    type Args = FunctionArgs;

    async fn try_new(
        args: &<Self as ::dash_pipe_provider::FunctionBuilder>::Args,
        ctx: &mut FunctionContext,
        _storage: &Arc<StorageIO>,
    ) -> Result<Self> {
        async fn loop_capture_frames(
            args: FunctionArgs,
            tx: &mpsc::Sender<Result<(Bytes, Metadata), String>>,
        ) -> Result<()> {
            let FunctionArgs {
                camera_buffer_size,
                camera_codec,
                camera_device,
                camera_fps,
                camera_height,
                camera_width,
            } = args.clone();

            let device = Device::new(camera_device)
                .map_err(|error| anyhow!("failed to init video device: {error}"))?;

            {
                let mut fmt = device
                    .format()
                    .map_err(|error| anyhow!("failed to retrieve video format: {error}"))?;
                fmt.width = camera_width;
                fmt.height = camera_height;
                fmt.fourcc = FourCC::new(camera_codec.as_fourcc());
                device
                    .set_format(&fmt)
                    .map_err(|error| anyhow!("failed to set video format: {error}"))?;
            }

            let mut capture = Stream::with_buffers(
                &device,
                BufferType::VideoCapture,
                camera_buffer_size
                    .try_into()
                    .map_err(|_| anyhow!("too large camera buffer size: {camera_buffer_size}"))?,
            )
            .map_err(|error| anyhow!("failed to open video capture: {error}"))?;

            loop {
                let (buf, metadata) = capture
                    .next()
                    .map_err(|error| anyhow!("failed to open video capture: {error}"))?;

                tx.send(Ok((Bytes::from(buf.to_vec()), *metadata))).await?;
            }
        }

        let FunctionArgs {
            camera_codec,
            camera_buffer_size,
            camera_height,
            camera_width,
            ..
        } = args;

        let (tx, rx) = mpsc::channel(*camera_buffer_size);
        spawn({
            let args = args.clone();
            async move {
                if let Err(error) = loop_capture_frames(args, &tx).await {
                    tx.send(Err(error.to_string())).await.ok();
                }
            }
        });

        Ok(Self {
            camera_codec: *camera_codec,
            ctx: {
                ctx.disable_load();
                ctx.clone()
            },
            capture: rx,
            frame_counter: Default::default(),
            frame_size: ImageSize {
                width: *camera_width,
                height: *camera_height,
            },
        })
    }
}

#[async_trait]
impl ::dash_pipe_provider::Function for Function {
    type Input = ();
    type Output = ::dash_openapi::image::Image;

    async fn tick(
        &mut self,
        _inputs: PipeMessages<<Self as ::dash_pipe_provider::Function>::Input>,
    ) -> Result<PipeMessages<<Self as ::dash_pipe_provider::Function>::Output>> {
        let (frame, timestamp) = match self.capture.recv().await {
            Some(Ok((frame, meta))) => match self.camera_codec {
                ImageCodec::Jpeg => {
                    let timestamp = NaiveDateTime::from_timestamp_opt(
                        meta.timestamp.sec,
                        (meta.timestamp.usec * 1_000) as u32,
                    );
                    (frame, timestamp)
                }
            },
            Some(Err(error)) => return self.ctx.terminate_err(anyhow!(error)),
            None => {
                return self
                    .ctx
                    .terminate_err(anyhow!("video capture is disconnected!"))
            }
        };

        let frame_idx = self.frame_counter.next();
        let payloads = vec![PipePayload::new(
            format!(
                "images/{frame_idx:06}{ext}",
                ext = self.camera_codec.as_extension(),
            ),
            Some(frame),
        )];
        let value = FunctionOutput {
            codec: self.camera_codec,
            index: frame_idx,
            size: self.frame_size,
        };

        Ok(PipeMessages::Single({
            let mut payload = PipeMessage::with_payloads(payloads, value);
            if let Some(timestamp) = timestamp {
                payload.set_timestamp(DateTime::from_naive_utc_and_offset(timestamp, Utc));
            }
            payload
        }))
    }
}

#[derive(Debug, Default)]
struct FrameCounter(usize);

impl FrameCounter {
    fn next(&mut self) -> usize {
        let index = self.0;
        self.0 += 1;
        index
    }
}
