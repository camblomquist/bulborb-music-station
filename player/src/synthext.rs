use oxisynth::{MidiEvent, Synth};

pub trait SynthExt {
    fn set_volume(&mut self, channel: u8, volume: u8);
    fn set_pan(&mut self, channel: u8, pan: u8);

    fn channel_off(&mut self, channel: u8);
}

impl SynthExt for Synth {
    fn set_volume(&mut self, channel: u8, volume: u8) {
        let _ = self.send_event(MidiEvent::ControlChange {
            channel,
            ctrl: 7,
            value: volume,
        });
    }

    fn set_pan(&mut self, channel: u8, pan: u8) {
        let _ = self.send_event(MidiEvent::ControlChange {
            channel,
            ctrl: 10,
            value: pan,
        });
    }

    fn channel_off(&mut self, channel: u8) {
        let _ = self.send_event(MidiEvent::AllNotesOff { channel });
    }
}
