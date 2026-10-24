//! Processing shared with the iPhone app: preamp, compression and stereo width.
use pm_native::dsp::{pm_eq_process_channel, pm_spatial_process_interleaved, PMBiquadCoeffs};
use serde::{Deserialize, Serialize};
use super::eq::Equalizer;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioSettings {
    pub preamp: f32,
    pub compression: bool,
    pub spatial: bool,
    pub spatial_intensity: f32,
}
impl Default for AudioSettings {
    fn default() -> Self { Self { preamp: 0.0, compression: false, spatial: false, spatial_intensity: 0.35 } }
}
impl AudioSettings {
    pub fn valid(&self) -> bool {
        self.preamp.is_finite() && (-12.0..=6.0).contains(&self.preamp)
            && self.spatial_intensity.is_finite() && (0.0..=1.0).contains(&self.spatial_intensity)
    }
}

pub struct Processor {
    equalizer: Option<Equalizer>,
    settings: AudioSettings,
    rate: u32,
    channels: usize,
    gain: f32,
    envelopes: Vec<f32>,
    side_filter: PMBiquadCoeffs,
    side_state: [f32; 4],
    width: f32,
}
impl Processor {
    pub fn transition(previous: Option<Self>, gains: Option<&[f32;10]>, rate: u32, channels: usize, settings: AudioSettings) -> Option<Self> {
        if rate == 0 || channels == 0 || !settings.valid() { return None; }
        let previous=previous.filter(|old|old.rate==rate && old.channels==channels);
        let gain=previous.as_ref().map_or(1.0,|old|old.gain);
        let width=previous.as_ref().map_or(0.0,|old|old.width);
        let side_state=previous.as_ref().map_or([0.0;4],|old|old.side_state);
        let envelopes=previous.as_ref().filter(|old|old.settings.compression && settings.compression).map_or_else(||vec![1.0;channels],|old|old.envelopes.clone());
        let equalizer=Equalizer::transition(previous.and_then(|old|old.equalizer),gains,rate,channels);
        let omega=2.0*std::f32::consts::PI*90.0/rate as f32;
        let cos=omega.cos();let alpha=omega.sin()/std::f32::consts::SQRT_2;let a0=1.0+alpha;
        Some(Self {equalizer,settings,rate,channels,gain,envelopes,
            side_filter:PMBiquadCoeffs{b0:(1.0+cos)/2.0/a0,b1:-(1.0+cos)/a0,b2:(1.0+cos)/2.0/a0,a1:-2.0*cos/a0,a2:(1.0-alpha)/a0},side_state,width})
    }
    pub fn reset(&mut self) {
        if let Some(eq)=self.equalizer.as_mut(){eq.reset();}
        self.side_state.fill(0.0);self.envelopes.fill(1.0);
    }
    pub fn process(&mut self,samples:&mut [f32]) {
        if let Some(equalizer)=self.equalizer.as_mut(){equalizer.process(samples);}
        let target_gain=10.0_f32.powf(self.settings.preamp/20.0);
        let target_width=if self.settings.spatial{self.settings.spatial_intensity}else{0.0};
        if self.settings.preamp==0.0 && self.gain==1.0 && !self.settings.compression && target_width==0.0 && self.width==0.0 {return;}
        let step=1.0-(-1.0/(0.03*self.rate as f32)).exp();
        for frame in samples.chunks_exact_mut(self.channels) {
            self.gain+=(target_gain-self.gain)*step;
            if (self.gain-target_gain).abs()<0.00001{self.gain=target_gain;}
            for sample in frame { *sample=if sample.is_finite(){*sample*self.gain}else{0.0}; }
        }
        let Ok(frames)=i32::try_from(samples.len()/self.channels) else{return};
        let Ok(stride)=i32::try_from(self.channels) else{return};
        if frames==0{return;}
        if self.settings.compression {
            for channel in 0..self.channels {
                // Each channel has its own valid envelope; there are no filter pointers when band_count is zero.
                unsafe {pm_eq_process_channel(samples.as_mut_ptr().add(channel),frames,stride,std::ptr::null(),0,std::ptr::null_mut(),1.0,true,0.6,3.0,1.2,&mut self.envelopes[channel],false,1.0);}
            }
        }
        let blend=1.0-(-(frames as f32)/(0.03*self.rate as f32)).exp();
        self.width+=(target_width-self.width)*blend;
        if (self.width-target_width).abs()<0.00001 {self.width=target_width;}
        if self.channels==2 && self.width>0.0 {
            // The same native mid/side algorithm as iOS, with a 90 Hz side high-pass.
            unsafe {pm_spatial_process_interleaved(samples.as_mut_ptr(),frames,stride,self.width,&self.side_filter,true,self.side_state.as_mut_ptr());}
        }
        for sample in samples { *sample=if sample.is_finite(){sample.clamp(-1.0,1.0)}else{0.0}; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn defaults_are_bit_exact_bypass(){let mut p=Processor::transition(None,None,48000,2,AudioSettings::default()).unwrap();let mut data=vec![0.2,-0.4,0.6,0.1];let original=data.clone();p.process(&mut data);assert_eq!(data,original);}
    #[test] fn preamp_is_smooth_and_reaches_the_requested_gain(){let mut p=Processor::transition(None,None,48000,2,AudioSettings{preamp:-6.0,..Default::default()}).unwrap();let mut data=vec![0.5;48000];p.process(&mut data);assert!((data[0]-0.5).abs()<0.001);assert!((data[data.len()-1]-0.2506).abs()<0.001);}
    #[test] fn widening_changes_stereo_and_keeps_mono_safe(){for channels in [1,2]{let mut p=Processor::transition(None,None,48000,channels,AudioSettings{spatial:true,spatial_intensity:1.0,..Default::default()}).unwrap();let mut data:Vec<f32>=(0..4800).flat_map(|i|{let a=(i as f32*0.2).sin()*0.4;if channels==2{vec![a,-a]}else{vec![a]}}).collect();let before=data.clone();p.process(&mut data);assert!(data.iter().all(|v|v.is_finite()&&v.abs()<=1.0));if channels==1{assert_eq!(data,before)}else{assert_ne!(data,before)}}}
    #[test] fn compression_and_boost_are_bounded(){let mut p=Processor::transition(None,None,44100,2,AudioSettings{preamp:6.0,compression:true,..Default::default()}).unwrap();let mut data=vec![0.95;44100];p.process(&mut data);assert!(data.iter().all(|v|v.is_finite()&&v.abs()<=1.0));let mut quiet=Processor::transition(None,None,44100,2,AudioSettings{compression:true,..Default::default()}).unwrap();let mut signal=vec![0.95;44100];quiet.process(&mut signal);assert!(signal[signal.len()-1]<0.95);}
    #[test] fn invalid_controls_are_rejected(){for value in [f32::NAN,f32::INFINITY,-13.0,7.0]{assert!(!AudioSettings{preamp:value,..Default::default()}.valid())}for value in [f32::NAN,-0.1,1.1]{assert!(!AudioSettings{spatial_intensity:value,..Default::default()}.valid())}}
}
