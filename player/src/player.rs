use std::{
    cell::{RefCell, RefMut},
    error::Error,
    fs::File,
    ops::ControlFlow,
    sync::{
        Arc,
        mpsc::{self, Sender},
    },
    time::{Duration, Instant},
};

use midly::{MetaMessage, TrackEvent, TrackEventKind};
use oxisynth::{MidiEvent, SoundFont, Synth};
use tinyaudio::{OutputDevice, OutputDeviceParameters, run_output_device};

use crate::{
    clock::Clock,
    synthext::{SynthEvent, SynthExt},
};
use bms_common::{Composition, CompositionTrack, Gesture, Timing};
use rand::{Rng, SeedableRng, rngs::SmallRng};

const LATENCY: Duration = Duration::from_millis(15);

const SWING_MAGNITUDE: f32 = 2.0;

pub struct Player<'a> {
    synth: Arc<spin::Mutex<Synth>>,
    synth_sender: RefCell<Option<Sender<SynthEvent>>>,
    beat_track: BeatTrack,
    conductor_tracks: Vec<ConductorTrack>,
    gesture_tracks: RefCell<Vec<Option<GestureTrack>>>,
    active_mix_tracks: RefCell<Vec<bool>>,
    rng: RefCell<SmallRng>,
    composition: Composition<'a>,
}

impl<'a> Player<'a> {
    pub fn new(composition: Composition<'a>) -> Self {
        let mut synth = Synth::default();

        let mut font = File::open("./data/weedsgm3.sf2").unwrap();
        let font = SoundFont::load(&mut font).unwrap();
        let _font = synth.add_font(font, true);

        let beat_track = BeatTrack::new(composition.tempo, composition.swing_time, true);

        let conductor_tracks = composition
            .tracks
            .iter()
            .enumerate()
            .map(|(id, track)| {
                let channel = id as u8;
                // Since multi-module isn't real and can't hurt me,
                // we can set these track parameters now
                synth.set_volume(channel, track.volume);
                synth.set_pan(channel, track.panning);

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

        let mut active_mix_tracks = vec![false; composition.tracks.len()];
        composition.mixes[0]
            .tracks
            .iter()
            .for_each(|&track| active_mix_tracks[track] = true);
        let active_mix_tracks = active_mix_tracks.into();

        let rng = SmallRng::from_os_rng().into();

        Self {
            synth: Arc::new(synth.into()),
            synth_sender: None.into(),
            beat_track,
            conductor_tracks,
            gesture_tracks,
            active_mix_tracks,
            rng,
            composition,
        }
    }

    pub fn play(&self) -> Result<(), Box<dyn Error>> {
        let _device = self.build_tiny_stream()?;

        let mut clock = Clock::new(self.composition.tempo, 120);
        let mut timestamp = Instant::now();
        loop {
            let start = Instant::now();

            if let Some(tempo) = self.beat_track.tick() {
                println!("Tempo: {:?}", Duration::from_micros(tempo as _));
                clock.set_tempo(tempo);
            }

            for track in &self.conductor_tracks {
                track.tick(self);
            }
            let mut gesture_tracks = self.gesture_tracks.borrow_mut();
            let gesture_tracks = gesture_tracks.iter_mut().filter(|track| track.is_some());
            for track in gesture_tracks {
                if track.as_mut().unwrap().tick(timestamp, self).is_break() {
                    *track = None;
                }
            }

            timestamp += clock.pulse_time();
            let cycle_time = Instant::now() - start;
            clock.tick(cycle_time);
        }

        // Ok(())
    }

    fn rng(&'_ self) -> RefMut<'_, impl Rng> {
        self.rng.borrow_mut()
    }

    fn send_event(
        &self,
        track_id: usize,
        timestamp: Instant,
        event: TrackEvent<'a>,
    ) -> ControlFlow<()> {
        let active = self.active_mix_tracks.borrow()[track_id];
        let sender = self.synth_sender.borrow();
        let sender = sender.as_ref().unwrap();
        match event.kind {
            TrackEventKind::Midi {
                channel: _,
                message,
            } if active => {
                if let Some(event) = self.convert_message(track_id, message) {
                    let _ = sender.send(SynthEvent { timestamp, event });
                }
                ControlFlow::Continue(())
            }
            TrackEventKind::Meta(MetaMessage::EndOfTrack) => {
                let channel = track_id as u8;
                let event = MidiEvent::AllNotesOff { channel };
                let _ = sender.send(SynthEvent { timestamp, event });
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
            midly::MidiMessage::NoteOff { key, vel: _ } => Some(MidiEvent::NoteOff {
                channel,
                key: key.as_int().saturating_add_signed(transposition),
            }),
            midly::MidiMessage::NoteOn { key, vel } => Some(MidiEvent::NoteOn {
                channel,
                key: key.as_int().saturating_add_signed(transposition),
                vel: vel.as_int(),
            }),
            midly::MidiMessage::Controller { controller, value } => {
                Some(MidiEvent::ControlChange {
                    channel,
                    ctrl: controller.as_int(),
                    value: value.as_int(),
                })
            }
            _ => None,
        }
    }

    fn start_gesture(&self, gesture: GestureTrack) {
        let id = gesture.track_id;
        self.gesture_tracks.borrow_mut()[id] = Some(gesture);
    }

    fn stop_gesture(&self, track_id: usize) {
        let sender = self.synth_sender.borrow();
        let sender = sender.as_ref().unwrap();
        let timestamp = Instant::now();
        let channel = track_id as u8;
        let event = MidiEvent::AllNotesOff { channel };
        let _ = sender.send(SynthEvent { timestamp, event });
        self.gesture_tracks.borrow_mut()[track_id] = None;
    }

    fn conductor_track(&self, id: usize) -> &ConductorTrack {
        &self.conductor_tracks[id]
    }

    fn gesture(&self, track_id: usize, gesture: usize) -> &Gesture<'a> {
        &self.composition.gesture_set_for_track(track_id)[gesture]
    }

    fn build_tiny_stream(&self) -> Result<OutputDevice, Box<dyn Error>> {
        let sample_rate = 44100;
        let latency = (LATENCY.as_secs_f64() * sample_rate as f64) as usize;
        let params = OutputDeviceParameters {
            channels_count: 2,
            sample_rate,
            channel_sample_count: latency / 2,
        };

        let (tx, rx) = mpsc::channel();
        *self.synth_sender.borrow_mut() = Some(tx);

        // We don't store the device so this doesn't create a cycle
        let synth = self.synth.clone();
        let mut prev = Instant::now();
        let data_callback = move |data: &mut [f32]| {
            // How I wish I could just lock this from outside
            let Some(mut synth) = synth.try_lock() else {
                return;
            };
            let sample_rate = sample_rate as f32;
            let mut data = data;
            let mut samples;
            for event in rx.try_iter() {
                if event.timestamp <= prev {
                    let _ = synth.send_event(event.event);
                }
                samples = ((event.timestamp - prev).as_secs_f32() * sample_rate) as usize;
                samples = samples.min(data.len());
                synth.write(&mut data[..samples]);
                data = &mut data[samples..];
                prev = event.timestamp;
            }
            synth.write(data);
        };
        let device = run_output_device(params, data_callback)?;
        Ok(device)
    }
}

struct BeatTrack {
    tempo: u32,
    swing_min: u32,
    swing_max: u32,
    beat: u32,
    state: RefCell<BeatTrackState>,
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
    state: RefCell<ConductorTrackState>,
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

    fn tick(&self, player: &Player) {
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

    fn on_cycle(&self, timing: Timing, player: &Player) {
        player.stop_gesture(self.id);
        if !timing.cutoff() {
            let gesture = self.next_gesture(player);
            let gesture = GestureTrack::new(self.id, gesture, player);
            player.start_gesture(gesture);
        }
    }

    // Get a Random Timing or the next timing in the in_order sequence
    // Returns None if in_order reached the end of the sequence
    fn next_timing(&self, player: &Player) -> Option<Timing> {
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

    fn next_gesture(&self, player: &Player) -> usize {
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
    state: RefCell<GestureTrackState>,
}

#[derive(Clone)]
struct GestureTrackState {
    next_event: u32,
    index: usize,
}

impl GestureTrack {
    fn new(track_id: usize, gesture: usize, player: &Player) -> Self {
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

    fn tick(&self, timestamp: Instant, player: &Player) -> ControlFlow<()> {
        if self.state.borrow().next_event == 0 {
            let gesture = player.gesture(self.track_id, self.gesture);
            player.send_event(self.track_id, timestamp, gesture[self.state.borrow().index])?;
            while let Some(next) = self.next_event(gesture) {
                let delta = next.delta.as_int();
                if delta == 0 {
                    player.send_event(self.track_id, timestamp, next)?;
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
