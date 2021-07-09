use gst::prelude::*;
use gst_audio::StreamVolumeExt;
use std::{collections::HashMap, error::Error};

#[derive(Debug)]
struct GstAudioOut {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
    sink: gst::Element,
}

impl GstAudioOut {
    fn new(caps: &str) -> Result<Self, Box<dyn Error>> {
        let pipeline = &format!("appsrc name=src is-live=1 do-timestamp=0 format=time caps=\"{}\" ! queue ! audioconvert ! audioresample ! autoaudiosink name=sink", caps);
        let pipeline = gst::parse_launch(pipeline)?;
        let pipeline = pipeline.dynamic_cast::<gst::Pipeline>().unwrap();
        let src = pipeline
            .get_by_name("src")
            .unwrap()
            .dynamic_cast::<gst_app::AppSrc>()
            .unwrap();
        let sink = pipeline.get_by_name("sink").unwrap();
        Ok(Self {
            pipeline,
            src,
            sink,
        })
    }
}

#[derive(Debug, Default)]
pub struct GstAudio {
    out: HashMap<u64, GstAudioOut>,
}

impl GstAudio {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        gst::init()?;

        Ok(Self::default())
    }

    pub fn init_out(&mut self, id: u64, caps: &str) -> Result<(), Box<dyn Error>> {
        if self.out.contains_key(&id) {
            return Err(format!("id {} is already setup", id).into());
        }

        let out = GstAudioOut::new(caps)?;
        self.out.insert(id, out);
        Ok(())
    }

    pub fn fini_out(&mut self, id: u64) {
        self.out.remove(&id);
    }

    fn get_out(&self, id: u64) -> Result<&GstAudioOut, String> {
        self.out
            .get(&id)
            .ok_or_else(|| format!("Stream not found: {}", id))
    }

    pub fn set_enabled_out(&mut self, id: u64, enabled: bool) -> Result<(), Box<dyn Error>> {
        self.get_out(id)?.pipeline.set_state(if enabled {
            gst::State::Playing
        } else {
            gst::State::Ready
        })?;
        Ok(())
    }

    pub fn set_volume_out(
        &mut self,
        id: u64,
        mute: bool,
        vol: Option<f64>,
    ) -> Result<(), Box<dyn Error>> {
        let stream_vol = self
            .get_out(id)?
            .pipeline
            .get_by_interface(gst_audio::StreamVolume::static_type())
            .ok_or("Pipeline doesn't support volume")?
            .dynamic_cast::<gst_audio::StreamVolume>()
            .unwrap();

        stream_vol.set_mute(mute);
        if let Some(vol) = vol {
            stream_vol.set_volume(gst_audio::StreamVolumeFormat::Cubic, vol);
        }
        Ok(())
    }

    pub fn write_out(&mut self, id: u64, data: Vec<u8>) -> Result<(), Box<dyn Error>> {
        self.get_out(id)?
            .src
            .push_buffer(gst::Buffer::from_slice(data))?;
        Ok(())
    }
}
