//! Optional VK profile broadcast and listening reports, matching the phone app.
use serde::{Deserialize, Serialize};
use super::{Session,Track,VkClient};
use crate::{Error,Result};
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListeningEvent {
    pub e:String,pub audio_id:String,pub uuid:i32,pub start_time:u64,pub playback_started_at:u64,
    pub duration:u32,pub track_code:String,pub streaming_type:String,pub shuffle:bool,
    pub repeat_mode:String,pub reason:String,pub state:String,
}
impl ListeningEvent {
    pub fn valid(&self)->bool {
        matches!(self.e.as_str(),"music_start_playback"|"music_stop_playback") && self.uuid>0
            && self.audio_id.len()<=64 && !self.audio_id.is_empty() && self.audio_id.chars().all(|c|c.is_ascii_digit()||c=='_'||c=='-')
            && self.track_code.len()<=4096 && self.duration<=86400
            && matches!(self.streaming_type.as_str(),"online"|"offline") && matches!(self.repeat_mode.as_str(),"off"|"all"|"one")
            && self.reason=="user" && self.state=="app"
    }
}
impl VkClient {
    pub fn broadcast(&self,session:&Session,track:Option<&Track>)->Result<()> {
        let params=track.map(|t|vec![("audio",t.id())]).unwrap_or_default();
        self.call_once(session,"audio.setBroadcast",&params)?;Ok(())
    }
    pub fn report_playback(&self,session:&Session,events:&[ListeningEvent])->Result<()> {
        if events.is_empty(){return Ok(())}
        if events.len()>32 || events.iter().any(|e|!e.valid()){return Err(Error::InvalidRequest)}
        // VK's event field is named "repeat". Keep a Rust-friendly name locally.
        let mut encoded=serde_json::to_value(events).map_err(|_|Error::InvalidRequest)?;
        for event in encoded.as_array_mut().ok_or(Error::InvalidRequest)? {let map=event.as_object_mut().ok_or(Error::InvalidRequest)?;if let Some(repeat)=map.remove("repeat_mode"){map.insert("repeat".into(),repeat);}}
        self.call_once(session,"stats.trackEvents",&[("events",encoded.to_string())])?;Ok(())
    }
}
