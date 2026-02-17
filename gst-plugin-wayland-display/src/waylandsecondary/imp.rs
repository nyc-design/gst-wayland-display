use gst::glib;
use gst::subclass::prelude::*;
use gst::{Fraction, LoggableError};
use gst_base::prelude::BaseSrcExt;
use gst_base::subclass::base_src::CreateSuccess;
use gst_base::subclass::prelude::*;
use gst_video::{VideoCapsBuilder, VideoFormat, VideoInfo, VideoInfoDmaDrm};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::time::{Duration, Instant};
#[cfg(feature = "cuda")]
use waylanddisplaycore::utils::allocator::cuda;
use waylanddisplaycore::{Command, GstVideoInfo, Sender};

pub struct WaylandDisplaySecondary {
    settings: Mutex<Settings>,
    /// Command sender to the shared compositor (obtained from the global registry).
    compositor_tx: Mutex<Option<Sender<Command>>>,
}

impl Default for WaylandDisplaySecondary {
    fn default() -> Self {
        WaylandDisplaySecondary {
            settings: Mutex::new(Settings::default()),
            compositor_tx: Mutex::new(None),
        }
    }
}

#[derive(Debug, Default)]
pub struct Settings {
    /// The WAYLAND_DISPLAY socket name of the primary compositor to attach to.
    compositor_name: Option<String>,
}

#[glib::object_subclass]
impl ObjectSubclass for WaylandDisplaySecondary {
    const NAME: &'static str = "GstWaylandDisplaySecondary";
    type Type = super::WaylandDisplaySecondary;
    type ParentType = gst_base::PushSrc;
    type Interfaces = ();
}

impl ObjectImpl for WaylandDisplaySecondary {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
            vec![glib::ParamSpecString::builder("compositor-name")
                .nick("Compositor Name")
                .blurb("The WAYLAND_DISPLAY socket name of the primary compositor to share")
                .construct()
                .build()]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        match pspec.name() {
            "compositor-name" => {
                let mut settings = self.settings.lock().unwrap();
                settings.compositor_name = value
                    .get::<Option<String>>()
                    .expect("Type checked upstream");
            }
            _ => unreachable!(),
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            "compositor-name" => {
                let settings = self.settings.lock().unwrap();
                settings
                    .compositor_name
                    .clone()
                    .unwrap_or_default()
                    .to_value()
            }
            _ => unreachable!(),
        }
    }

    fn constructed(&self) {
        self.parent_constructed();

        let obj = self.obj();
        obj.set_element_flags(gst::ElementFlags::SOURCE);
        obj.set_live(true);
        obj.set_format(gst::Format::Time);
        obj.set_automatic_eos(false);
        obj.set_do_timestamp(true);
    }
}

impl GstObjectImpl for WaylandDisplaySecondary {}

impl ElementImpl for WaylandDisplaySecondary {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: Lazy<gst::subclass::ElementMetadata> = Lazy::new(|| {
            gst::subclass::ElementMetadata::new(
                "Wayland display secondary source",
                "Source/Video",
                "GStreamer video src for the secondary output of a shared wayland compositor",
                "Gamer <https://github.com/nyc-design/gst-wayland-display>",
            )
        });

        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: Lazy<Vec<gst::PadTemplate>> = Lazy::new(|| {
            let caps = VideoCapsBuilder::new()
                .format(VideoFormat::Rgbx)
                .height_range(..i32::MAX)
                .width_range(..i32::MAX)
                .framerate_range(Fraction::new(1, 1)..Fraction::new(i32::MAX, 1))
                .build();

            let mut dmabuf_caps = VideoCapsBuilder::new()
                .features([gstreamer_allocators::CAPS_FEATURE_MEMORY_DMABUF])
                .format(VideoFormat::DmaDrm)
                .height_range(..i32::MAX)
                .width_range(..i32::MAX)
                .framerate_range(Fraction::new(1, 1)..Fraction::new(i32::MAX, 1))
                .build();

            dmabuf_caps.merge(caps);

            #[cfg(feature = "cuda")]
            {
                let cuda_caps = VideoCapsBuilder::new()
                    .features([cuda::CAPS_FEATURE_MEMORY_CUDA_MEMORY])
                    .format_list([VideoFormat::Bgra, VideoFormat::Rgba])
                    .height_range(..i32::MAX)
                    .width_range(..i32::MAX)
                    .framerate_range(Fraction::new(1, 1)..Fraction::new(i32::MAX, 1))
                    .build();
                dmabuf_caps.merge(cuda_caps);
            }

            let src_pad_template = gst::PadTemplate::new(
                "src",
                gst::PadDirection::Src,
                gst::PadPresence::Always,
                &dmabuf_caps,
            )
            .unwrap();

            vec![src_pad_template]
        });

        PAD_TEMPLATES.as_ref()
    }

    fn change_state(
        &self,
        transition: gst::StateChange,
    ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        let res = self.parent_change_state(transition);
        match res {
            Ok(gst::StateChangeSuccess::Success) => {
                if transition.next() == gst::State::Paused {
                    Ok(gst::StateChangeSuccess::NoPreroll)
                } else {
                    Ok(gst::StateChangeSuccess::Success)
                }
            }
            x => x,
        }
    }
}

impl BaseSrcImpl for WaylandDisplaySecondary {
    fn caps(&self, filter: Option<&gst::Caps>) -> Option<gst::Caps> {
        // Advertise the same caps as the primary element — the compositor will
        // allocate the secondary buffer in the same format family.
        let mut caps = VideoCapsBuilder::new()
            .format(VideoFormat::Rgbx)
            .height_range(..i32::MAX)
            .width_range(..i32::MAX)
            .framerate_range(Fraction::new(1, 1)..Fraction::new(i32::MAX, 1))
            .build();

        #[cfg(feature = "cuda")]
        {
            let cuda_caps = VideoCapsBuilder::new()
                .features([cuda::CAPS_FEATURE_MEMORY_CUDA_MEMORY])
                .format_list([VideoFormat::Bgra, VideoFormat::Rgba])
                .height_range(..i32::MAX)
                .width_range(..i32::MAX)
                .framerate_range(Fraction::new(1, 1)..Fraction::new(i32::MAX, 1))
                .build();
            caps.merge(cuda_caps);
        }

        let dmabuf_caps = VideoCapsBuilder::new()
            .features([gstreamer_allocators::CAPS_FEATURE_MEMORY_DMABUF])
            .format(VideoFormat::DmaDrm)
            .height_range(..i32::MAX)
            .width_range(..i32::MAX)
            .framerate_range(Fraction::new(1, 1)..Fraction::new(i32::MAX, 1))
            .build();
        caps.merge(dmabuf_caps);

        if let Some(filter) = filter {
            caps = caps.intersect(filter);
        }

        Some(caps)
    }

    fn negotiate(&self) -> Result<(), LoggableError> {
        self.parent_negotiate()
    }

    fn set_caps(&self, caps: &gst::Caps) -> Result<(), LoggableError> {
        let video_info = match VideoInfoDmaDrm::from_caps(caps) {
            Ok(dma_video_info) => GstVideoInfo::DMA(dma_video_info),
            #[cfg(feature = "cuda")]
            Err(_) => {
                let base_video_info =
                    VideoInfo::from_caps(caps).expect("failed to get video info");
                let is_cuda = caps
                    .features(0)
                    .expect("Failed to get features")
                    .contains(cuda::CAPS_FEATURE_MEMORY_CUDA_MEMORY);
                if is_cuda {
                    // For CUDA secondary, we just report RAW for now since we
                    // don't have a CUDA context here. The compositor will handle
                    // the actual CUDA buffer allocation via the primary element.
                    GstVideoInfo::RAW(base_video_info)
                } else {
                    GstVideoInfo::RAW(base_video_info)
                }
            }
            #[cfg(not(feature = "cuda"))]
            Err(_) => {
                GstVideoInfo::RAW(VideoInfo::from_caps(caps).expect("failed to get video info"))
            }
        };

        // Send SecondaryVideoInfo to the shared compositor
        let compositor_tx = self.compositor_tx.lock().unwrap();
        if let Some(ref tx) = *compositor_tx {
            let _ = tx.send(Command::SecondaryVideoInfo(video_info));
        } else {
            tracing::warn!("Secondary element: compositor_tx not set when set_caps called");
        }

        self.parent_set_caps(caps)
    }

    fn start(&self) -> Result<(), gst::ErrorMessage> {
        let compositor_name = {
            let settings = self.settings.lock().unwrap();
            settings.compositor_name.clone()
        };

        // Wait briefly for the primary compositor to register. This makes the
        // bottom-screen app resilient to startup ordering.
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let tx = match compositor_name.as_ref() {
                Some(name) => waylanddisplaycore::lookup_compositor(name),
                None => waylanddisplaycore::lookup_active_compositor(),
            };

            if let Some(tx) = tx {
                let label = compositor_name.as_deref().unwrap_or("<active>");
                tracing::info!("Secondary element connected to compositor '{}'", label);
                let mut compositor_tx = self.compositor_tx.lock().unwrap();
                *compositor_tx = Some(tx);
                return Ok(());
            }

            if Instant::now() >= deadline {
                let msg = match compositor_name.as_ref() {
                    Some(name) => format!(
                        "Could not find compositor '{}' in registry. Start the primary dual-screen app first.",
                        name
                    ),
                    None => "No active compositor found. Start the primary dual-screen app first."
                        .to_string(),
                };
                return Err(gst::error_msg!(gst::LibraryError::Failed, ("{}", msg)));
            }

            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn stop(&self) -> Result<(), gst::ErrorMessage> {
        let mut compositor_tx = self.compositor_tx.lock().unwrap();
        *compositor_tx = None;
        tracing::info!("Secondary element stopped");
        Ok(())
    }

    fn is_seekable(&self) -> bool {
        false
    }
}

impl PushSrcImpl for WaylandDisplaySecondary {
    fn create(
        &self,
        _buffer: Option<&mut gst::BufferRef>,
    ) -> Result<CreateSuccess, gst::FlowError> {
        let tx = {
            let compositor_tx = self.compositor_tx.lock().unwrap();
            compositor_tx.clone()
        };
        let Some(tx) = tx else {
            return Err(gst::FlowError::Eos);
        };

        // Request frames until secondary output is ready.
        // Avoid returning FlowError::Error for transient "not ready yet" cases,
        // which can permanently tear down the pipeline.
        loop {
            let (buffer_tx, buffer_rx) = std::sync::mpsc::sync_channel(0);
            if let Err(err) = tx.send(Command::SecondaryBuffer(buffer_tx, None)) {
                tracing::warn!(?err, "Failed to send secondary buffer command.");
                return Err(gst::FlowError::Eos);
            }

            match buffer_rx.recv() {
                Ok(Ok(buffer)) => return Ok(CreateSuccess::NewBuffer(buffer)),
                Ok(Err(err)) => {
                    tracing::debug!(?err, "Secondary frame not ready yet; retrying");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(err) => {
                    tracing::warn!(?err, "Failed to recv secondary buffer ack.");
                    return Err(gst::FlowError::Error);
                }
            }
        }
    }
}
