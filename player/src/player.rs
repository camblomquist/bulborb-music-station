use std::{
    cell::RefCell,
    error::Error,
    fs::File,
    ops::ControlFlow,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use atomic_refcell::{AtomicRefCell, AtomicRefMut};
use midly::{MetaMessage, TrackEvent, TrackEventKind};
use oxisynth::{MidiEvent, SoundFont, Synth, SynthDescriptor};
use tinyaudio::{OutputDevice, OutputDeviceParameters, run_output_device};

use crate::{clock::Clock, synthext::SynthExt};
use bms_common::{Composition, CompositionTrack, Gesture, PITCH_BEND_RANGE, Timing};
use rand::{Rng, SeedableRng, rngs::SmallRng};

type AudioStream = OutputDevice;

const LATENCY: Duration = Duration::from_millis(50);

const SWING_MAGNITUDE: f32 = 2.0;

pub struct Player {
    inner: Arc<PlayerInner>,
    stream: RefCell<Option<AudioStream>>,
}

struct PlayerInner {
    sample_rate: f32,
    playing: AtomicBool,
    clock: AtomicRefCell<Clock>,
    synth: AtomicRefCell<Synth>,
    beat_track: BeatTrack,
    conductor_tracks: Vec<ConductorTrack>,
    gesture_tracks: AtomicRefCell<Vec<Option<GestureTrack>>>,
    active_mix_tracks: AtomicRefCell<Vec<bool>>,
    rng: AtomicRefCell<SmallRng>,
    composition: Arc<Composition<'static>>,
}

impl Player {
    pub fn new(composition: Arc<Composition<'static>>) -> Self {
        let descriptor = SynthDescriptor::default();
        let sample_rate = descriptor.sample_rate;
        let playing = AtomicBool::new(false);
        let mut synth = Synth::default();

        let mut font = File::open("./data/SGM.sf2").unwrap();
        let font = SoundFont::load(&mut font).unwrap();
        let font = synth.add_font(font, true);

        let beat_track = BeatTrack::new(composition.tempo, composition.beat_interval, false);

        let conductor_tracks = composition
            .tracks
            .iter()
            .enumerate()
            .map(|(id, track)| {
                let channel = id as u8;
                // Since multi-module isn't real and can't hurt me,
                // we can set these track parameters now
                synth
                    .select_program(channel, font, 0, track.program)
                    .unwrap();
                synth.set_volume(channel, track.volume);
                synth.set_pan(channel, track.panning);
                synth
                    .set_pitch_wheel_sensitivity(channel, PITCH_BEND_RANGE as u8)
                    .unwrap();

                let mut initial_delay = track.initial_delay;
                initial_delay += if let Some(unison) = track.unison_track {
                    composition.tracks[unison].extra_delay
                } else {
                    track.extra_delay
                };

                ConductorTrack::new(id, initial_delay, track)
            })
            .collect();

        let gesture_tracks = vec![None; composition.tracks.len()].into();

        let active_mix_tracks = vec![false; composition.tracks.len()].into();

        let clock = Clock::new(composition.tempo, 120).into();
        let synth = synth.into();
        let rng = SmallRng::from_os_rng().into();

        let inner = PlayerInner {
            sample_rate,
            playing,
            clock,
            synth,
            beat_track,
            conductor_tracks,
            gesture_tracks,
            active_mix_tracks,
            rng,
            composition,
        }
        .into();
        Self {
            inner,
            stream: None.into(),
        }
    }

    pub fn play(&self) -> Result<(), Box<dyn Error>> {
        if self.stream.borrow().is_none() {
            let stream = self.build_stream()?;
            *self.stream.borrow_mut() = Some(stream);
        }
        self.inner.set_mix(0, true);
        self.inner.set_mix(1, true);
        self.inner.set_mix(2, true);
        // self.inner.set_mix(3, true);
        // self.inner.set_mix(4, true);

        self.inner.playing.store(true, Ordering::Release);
        Ok(())
    }

    pub fn pause(&self) -> Result<(), Box<dyn Error>> {
        if self.stream.borrow().is_some() {
            self.inner.playing.store(false, Ordering::Release);
            Ok(())
        } else {
            Err("No active stream".into())
        }
    }

    pub fn stop(&self) -> Result<(), Box<dyn Error>> {
        if let Some(mut stream) = self.stream.take() {
            stream.close();
            Ok(())
        } else {
            Err("No active stream".into())
        }
    }

    pub fn build_stream(&self) -> Result<OutputDevice, Box<dyn Error>> {
        let sample_rate = self.inner.sample_rate;
        let channels_count = 2;
        let latency = (LATENCY.as_secs_f64() * sample_rate as f64) as usize;
        let params = OutputDeviceParameters {
            channels_count,
            sample_rate: sample_rate as _,
            channel_sample_count: latency / channels_count,
        };

        let player = self.inner.clone();
        let mut start = 0;
        let data_callback = move |data: &mut [f32]| {
            if !player.playing.load(Ordering::Acquire) {
                return;
            }
            let len = data.len();
            let mut end = start.min(len);
            player.synth.borrow_mut().write(&mut data[..end]);
            while end < len {
                let duration = player.tick();
                end += (duration.as_secs_f32() * sample_rate) as usize * channels_count;
                player
                    .synth
                    .borrow_mut()
                    .write(&mut data[start..end.min(len)]);
                start = end;
            }
            start = start.saturating_sub(len);
        };
        let device = run_output_device(params, data_callback)?;
        Ok(device)
    }
}

impl PlayerInner {
    fn tick(&self) -> Duration {
        if let Some(tempo) = self.beat_track.tick() {
            self.clock.borrow_mut().set_tempo(tempo);
            println!("Tempo: {:?}", Duration::from_micros(tempo as _),);
        }

        for track in &self.conductor_tracks {
            track.tick(self);
        }
        let mut gesture_tracks = self.gesture_tracks.borrow_mut();
        let gesture_tracks = gesture_tracks.iter_mut().filter(|track| track.is_some());
        for track in gesture_tracks {
            if track.as_mut().unwrap().tick(self).is_break() {
                *track = None;
            }
        }
        self.clock.borrow().pulse_time()
    }

    fn send_event(&self, track_id: usize, event: TrackEvent<'_>) -> ControlFlow<()> {
        let active = self.active_mix_tracks.borrow()[track_id];
        match event.kind {
            TrackEventKind::Midi {
                channel: _,
                message,
            } if active => {
                if let Some(event) = self.convert_message(track_id, message) {
                    let _ = self.synth.borrow_mut().send_event(event);
                }
                ControlFlow::Continue(())
            }
            TrackEventKind::Meta(MetaMessage::EndOfTrack) => {
                let channel = track_id as u8;
                let event = MidiEvent::AllNotesOff { channel };
                let _ = self.synth.borrow_mut().send_event(event);
                ControlFlow::Break(())
            }
            _ => ControlFlow::Break(()),
        }
    }

    fn convert_message(
        &self,
        track_id: usize,
        message: midly::MidiMessage,
    ) -> Option<oxisynth::MidiEvent> {
        let channel = track_id as u8;
        let transposition = self.composition.tracks[track_id].transposition;
        match message {
            midly::MidiMessage::ChannelAftertouch { vel } => Some(MidiEvent::ChannelPressure {
                channel,
                value: vel.as_int(),
            }),
            midly::MidiMessage::Controller { controller, value } => {
                Some(MidiEvent::ControlChange {
                    channel,
                    ctrl: controller.as_int(),
                    value: value.as_int(),
                })
            }
            midly::MidiMessage::NoteOff { key, vel: _ } => Some(MidiEvent::NoteOff {
                channel,
                key: key.as_int().saturating_add_signed(transposition),
            }),
            midly::MidiMessage::NoteOn { key, vel } => Some(MidiEvent::NoteOn {
                channel,
                key: key.as_int().saturating_add_signed(transposition),
                vel: vel.as_int(),
            }),
            midly::MidiMessage::PitchBend { bend } => Some(MidiEvent::PitchBend {
                channel,
                value: bend.0.as_int(),
            }),
            _ => None,
        }
    }

    fn start_gesture(&self, gesture: GestureTrack) {
        let id = gesture.track_id;
        self.gesture_tracks.borrow_mut()[id] = Some(gesture);
    }

    fn stop_gesture(&self, track_id: usize) {
        self.reset_channel(track_id);
        self.gesture_tracks.borrow_mut()[track_id] = None;
    }

    fn reset_channel(&self, track_id: usize) {
        let channel = track_id as u8;
        let events = [
            MidiEvent::AllNotesOff { channel },
            MidiEvent::PitchBend {
                channel,
                value: 0x2000,
            },
        ];
        let mut synth = self.synth.borrow_mut();
        for event in events {
            let _ = synth.send_event(event);
        }
    }

    fn set_mix(&self, mix_id: usize, enabled: bool) {
        let mut active_mix_tracks = self.active_mix_tracks.borrow_mut();
        self.composition.mixes[mix_id]
            .tracks
            .iter()
            .for_each(|&track| active_mix_tracks[track] = enabled);
    }

    fn conductor_track(&self, id: usize) -> &ConductorTrack {
        &self.conductor_tracks[id]
    }

    fn gesture(&self, track_id: usize, gesture: usize) -> &Gesture<'_> {
        &self.composition.gesture_set_for_track(track_id)[gesture]
    }

    fn rng(&'_ self) -> AtomicRefMut<'_, impl Rng> {
        self.rng.borrow_mut()
    }
}

struct BeatTrack {
    tempo: u32,
    swing_min: u32,
    swing_max: u32,
    beat: u32,
    state: AtomicRefCell<BeatTrackState>,
}

struct BeatTrackState {
    next_beat: u32,
    off_beat: bool,
    swing: bool,
}

impl BeatTrack {
    fn new(tempo: u32, beat: u32, start_swing: bool) -> Self {
        // JAS gets the beat from mBeatInterval but I can't find where it's set from the conductor
        // Still, it must be set for the default does not sound correct

        // JAS flips the names here but they were WRONG
        // We also have to do the math slightly differently because we use µs/beat for tempo
        let swing_min = tempo as f32 * 2.0 / (SWING_MAGNITUDE + 1.0);
        let swing_max = swing_min * SWING_MAGNITUDE;

        let state = BeatTrackState {
            next_beat: 0,
            off_beat: false,
            swing: start_swing,
        };
        Self {
            tempo,
            swing_min: swing_min as u32,
            swing_max: swing_max as u32,
            beat,
            state: state.into(),
        }
    }

    fn tick(&self) -> Option<u32> {
        let mut state = self.state.borrow_mut();
        let tempo = if state.next_beat == 0 {
            state.off_beat = !state.off_beat;
            let tempo = match (state.swing, state.off_beat) {
                (true, true) => self.swing_max,
                (true, false) => self.swing_min,
                (false, _) => self.tempo,
            };
            state.next_beat = self.beat;
            Some(tempo)
        } else {
            None
        };

        state.next_beat = state.next_beat.saturating_sub(1);
        tempo
    }
}

struct ConductorTrack {
    id: usize,
    unison_track: Option<usize>,
    in_order: bool,
    gesture_set: usize,
    timetable: usize,
    on_cycles: u32,
    off_cycles: u32,
    state: AtomicRefCell<ConductorTrackState>,
}

struct ConductorTrackState {
    cycle: Cycle,
    next_cycle: u32,
    next_event: u32,
    timing: usize,
    history: [usize; 2],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cycle {
    On,
    Off,
}

impl ConductorTrack {
    fn new(id: usize, initial_delay: u32, track: &CompositionTrack) -> Self {
        // JASTrack uses a wait operation with initial delay before running the track
        // We can get the same effect by starting at the end of an Off cycle
        let state = ConductorTrackState {
            cycle: Cycle::Off,
            next_cycle: 0,
            next_event: initial_delay,
            timing: usize::MAX,
            history: [usize::MAX; 2],
        }
        .into();
        Self {
            id,
            unison_track: track.unison_track,
            in_order: track.in_order,
            gesture_set: track.gesture_set,
            timetable: track.timetable,
            on_cycles: track.on_cycles,
            off_cycles: track.off_cycles,
            state,
        }
    }

    fn tick(&self, player: &PlayerInner) {
        if self.state.borrow().next_event == 0 {
            let timing = self.next_timing(player);
            if timing.is_none() || !self.in_order {
                self.tick_cycle();
            }
            // If we just started a new in_order cycle, need to retry timing
            let timing = timing.or_else(|| self.next_timing(player)).unwrap();
            let cycle = self.state.borrow().cycle;
            match cycle {
                Cycle::On => self.on_cycle(timing, player),
                Cycle::Off => (),
            };
            self.state.borrow_mut().next_event = timing.delta();
        }

        let mut state = self.state.borrow_mut();
        state.next_event = state.next_event.saturating_sub(1);
    }

    fn tick_cycle(&self) {
        let mut state = self.state.borrow_mut();
        if state.next_cycle == 0 {
            let (next, count) = match state.cycle {
                Cycle::On if self.off_cycles > 0 => (Cycle::Off, self.off_cycles),
                _ => (Cycle::On, self.on_cycles),
            };
            state.cycle = next;
            state.next_cycle = count;
            println!("({}) {:?} for {}", self.id, next, count);
        }
        state.timing = 0;
        state.next_cycle = state.next_cycle.saturating_sub(1);
    }

    fn on_cycle(&self, timing: Timing, player: &PlayerInner) {
        player.stop_gesture(self.id);
        if !timing.cutoff() {
            let gesture = self.next_gesture(player);
            let gesture = GestureTrack::new(self.id, gesture, player);
            player.start_gesture(gesture);
        }
    }

    // Get a Random Timing or the next timing in the in_order sequence
    // Returns None if in_order reached the end of the sequence
    fn next_timing(&self, player: &PlayerInner) -> Option<Timing> {
        let mut state = self.state.borrow_mut();
        let timetable = &player.composition.timetables[self.timetable];
        let unison_track = self
            .unison_track
            .map(|unison| player.conductor_track(unison));
        if self.in_order {
            let timing = if let Some(unison) = unison_track {
                // Since the unison track will have incremented already,
                // Need to subtract 1 to get its actual timing
                timetable.get(unison.timing().saturating_sub(1)).copied()
            } else {
                timetable.get(state.timing).copied()
            };
            state.timing = state.timing.saturating_add(1);
            timing
        } else {
            if let Some(unison) = unison_track {
                timetable.get(unison.timing()).copied()
            } else {
                state.timing = player.rng().random_range(0..timetable.len());
                timetable.get(state.timing).copied()
            }
        }
    }

    fn next_gesture(&self, player: &PlayerInner) -> usize {
        let gesture = if let Some(unison) = self.unison_track {
            player.conductor_track(unison).gesture()
        } else {
            let len = player.composition.gesture_sets[self.gesture_set].len();
            println!("({}) Sampling GS{}[{}]", self.id, self.gesture_set, len);
            let gesture = player.rng().random_range(0..len);
            if len > 1 && self.repeated_gesture().is_some_and(|g| g == gesture) {
                println!("({}) Re-Sampling [{}]", self.id, len - 1);
                // This is a really silly way to retry but it's how they did it
                // Likely not what they intended?
                let gesture = player.rng().random_range(0..(len - 1));
                if gesture <= self.gesture() {
                    gesture + 1
                } else {
                    gesture
                }
            } else {
                gesture
            }
        };

        let mut state = self.state.borrow_mut();
        state.history[0] = gesture;
        state.history.swap(0, 1);
        gesture
    }

    fn timing(&self) -> usize {
        self.state.borrow().timing
    }

    fn gesture(&self) -> usize {
        self.state.borrow().history[1]
    }

    fn repeated_gesture(&self) -> Option<usize> {
        let state = self.state.borrow();
        if state.history[0] == state.history[1] && state.history[0] != usize::MAX {
            Some(state.history[0])
        } else {
            None
        }
    }
}

#[derive(Clone)]
struct GestureTrack {
    track_id: usize,
    gesture: usize,
    state: AtomicRefCell<GestureTrackState>,
}

#[derive(Clone)]
struct GestureTrackState {
    next_event: u32,
    index: usize,
}

impl GestureTrack {
    fn new(track_id: usize, gesture: usize, player: &PlayerInner) -> Self {
        let next_event = player.gesture(track_id, gesture)[0].delta.as_int();

        Self {
            track_id,
            gesture,
            state: GestureTrackState {
                next_event,
                index: 0,
            }
            .into(),
        }
    }

    fn tick(&self, player: &PlayerInner) -> ControlFlow<()> {
        if self.state.borrow().next_event == 0 {
            let gesture = player.gesture(self.track_id, self.gesture);
            player.send_event(self.track_id, gesture[self.state.borrow().index])?;
            while let Some(next) = self.next_event(gesture) {
                let delta = next.delta.as_int();
                if delta == 0 {
                    player.send_event(self.track_id, next)?;
                } else {
                    self.state.borrow_mut().next_event = delta;
                    break;
                }
            }
        }

        let mut state = self.state.borrow_mut();
        state.next_event = state.next_event.saturating_sub(1);
        ControlFlow::Continue(())
    }

    fn next_event<'a>(&self, gesture: &Gesture<'a>) -> Option<TrackEvent<'a>> {
        let mut state = self.state.borrow_mut();
        state.index += 1;
        gesture.get(state.index).copied()
    }
}
