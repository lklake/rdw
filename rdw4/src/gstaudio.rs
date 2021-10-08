use futures::StreamExt;
use gst::prelude::*;
use gst_audio::prelude::*;
use std::{collections::HashMap, default::Default, error::Error};

#[derive(Debug)]
struct GstAudioOut {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
}

impl GstAudioOut {
    fn new(caps: &str) -> Result<Self, Box<dyn Error>> {
        let pipeline = &format!("appsrc name=src is-live=1 do-timestamp=0 format=time caps=\"{}\" ! queue ! audioconvert ! audioresample ! autoaudiosink name=sink", caps);
        let pipeline = gst::parse_launch(pipeline)?;
        let pipeline = pipeline.dynamic_cast::<gst::Pipeline>().unwrap();
        let src = pipeline
            .by_name("src")
            .unwrap()
            .dynamic_cast::<gst_app::AppSrc>()
            .unwrap();
        Ok(Self { pipeline, src })
    }
}

#[derive(Debug)]
struct GstAudioIn {
    pipeline: gst::Pipeline,
    sink: gst_app::AppSink,
    queue: Vec<u8>,
}

impl GstAudioIn {
    fn new(caps: &str) -> Result<Self, Box<dyn Error>> {
        let pipeline = &format!("autoaudiosrc name=src ! queue ! audioconvert ! audioresample ! appsink caps=\"{}\" name=sink", caps);
        let pipeline = gst::parse_launch(pipeline)?;
        let pipeline = pipeline.dynamic_cast::<gst::Pipeline>().unwrap();
        let sink = pipeline
            .by_name("sink")
            .unwrap()
            .dynamic_cast::<gst_app::AppSink>()
            .unwrap();
        Ok(Self {
            pipeline,
            sink,
            queue: Default::default(),
        })
    }
}

#[derive(Debug, Default)]
pub struct GstAudio {
    out: HashMap<u64, GstAudioOut>,
    in_: HashMap<u64, GstAudioIn>,
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
            .by_interface(gst_audio::StreamVolume::static_type())
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

    pub fn init_in(&mut self, id: u64, caps: &str) -> Result<(), Box<dyn Error>> {
        if self.in_.contains_key(&id) {
            return Err(format!("id {} is already setup", id).into());
        }

        let in_ = GstAudioIn::new(caps)?;
        self.in_.insert(id, in_);
        Ok(())
    }

    pub fn fini_in(&mut self, id: u64) {
        self.in_.remove(&id);
    }

    fn get_in(&self, id: u64) -> Result<&GstAudioIn, String> {
        self.in_
            .get(&id)
            .ok_or_else(|| format!("Stream not found: {}", id))
    }

    pub fn set_enabled_in(&mut self, id: u64, enabled: bool) -> Result<(), Box<dyn Error>> {
        self.get_in(id)?.pipeline.set_state(if enabled {
            gst::State::Playing
        } else {
            gst::State::Ready
        })?;
        Ok(())
    }

    pub fn set_volume_in(
        &mut self,
        id: u64,
        mute: bool,
        vol: Option<f64>,
    ) -> Result<(), Box<dyn Error>> {
        let stream_vol = self
            .get_in(id)?
            .pipeline
            .by_interface(gst_audio::StreamVolume::static_type())
            .ok_or("Pipeline doesn't support volume")?
            .dynamic_cast::<gst_audio::StreamVolume>()
            .unwrap();

        stream_vol.set_mute(mute);
        if let Some(vol) = vol {
            stream_vol.set_volume(gst_audio::StreamVolumeFormat::Cubic, vol);
        }
        Ok(())
    }

    pub async fn read_in(&mut self, id: u64, size: u64) -> Result<Vec<u8>, Box<dyn Error>> {
        use std::io::prelude::*;

        let in_ = self
            .in_
            .get_mut(&id)
            .ok_or_else(|| format!("Stream not found: {}", id))?;
        let mut stream = in_.sink.stream();
        while in_.queue.len() < size as _ {
            let sample = stream.next().await.ok_or_else(|| "EOS?".to_string())?;
            let buffer = sample.buffer().ok_or_else(|| "No buffer?".to_string())?;
            let mut cursor = buffer.as_cursor_readable();
            let len = cursor.stream_len()?;
            let cur = in_.queue.len();
            let end = cur + len as usize;
            in_.queue.resize(end, 0);
            cursor.read_exact(&mut in_.queue[cur..end])?;
        }
        let remainder = in_.queue.split_off(size as _);
        Ok(std::mem::replace(&mut in_.queue, remainder))
    }
}
