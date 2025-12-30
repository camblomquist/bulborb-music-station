use std::ops::{Index, IndexMut};

use crate::{
    bms::{Bms, BmsInstruction, ParamType, RawGesture, RawGestureSet, RawTimetable},
    conductor::{RawConductor, RawConductorTrack},
    trackmap::{RawTrackMap, RawTrackMapEntry},
};

use common::{Composition, Gesture, GestureSet, Timetable, Timing, Track};
use midly::{
    MetaMessage, MidiMessage, TrackEvent, TrackEventKind,
    num::{u4, u7},
};

const NO_NOTE: u8 = 0xFF;
const DEFAULT_VELOCITY: u7 = u7::new(64);

const GESTURE_START_SUBROUTINE: u32 = 0x15D;

pub struct RawComposition {
    pub name: String,
    pub bms: Bms,
    pub conductor: RawConductor,
    pub trackmap: RawTrackMap,
}

impl RawComposition {
    pub fn convert<'a>(self) -> Composition<'a> {
        let RawComposition {
            name,
            bms,
            conductor,
            trackmap,
        } = self;

        let trackmap = &trackmap[&name];
        Composition {
            tempo: conductor.bpm,
            swing_time: conductor.swing_time,
            skip_notes: trackmap.skip_notes,
            tracks: convert_tracks(conductor.tracks, trackmap),
            timetables: convert_timetables(bms.timetables),
            gesture_sets: convert_gesture_sets(bms.gesture_sets),
            instruments: (),
        }
    }
}

fn convert_tracks(tracks: Vec<RawConductorTrack>, trackmap: &RawTrackMapEntry) -> Vec<Track> {
    tracks
        .into_iter()
        .zip(trackmap.skip_tracks)
        .map(|(track, skippable)| {
            let title = String::from(track.title());
            let unison_track = if track.unison_track != 16 {
                Some(track.unison_track)
            } else {
                None
            };
            // Based on how JAS interprets the eighthDelay, it should be OK to combine them
            let extra_delay = match (track.eighth_delay, track.module.eighth_delay) {
                (true, true) => 120,
                (false, false) => 0,
                _ => 60,
            };
            Track {
                title,
                bank: track.module.bank as _,
                program: track.module.program,
                skippable,
                unison_track,
                initial_delay: track.initial_delay as _,
                extra_delay,
                in_order: track.module.in_order,
                gesture_set: track.module.gesture_set as _,
                timetable: track.module.timing_ruleset as _,
                on_cycles: track.module.on_cycles,
                off_cycles: track.module.off_cycles,
                transposition: track.module.transposition,
                volume: track.module.volume,
                panning: track.module.panning,
            }
        })
        .collect()
}

fn convert_timetables(timetables: Vec<RawTimetable>) -> Vec<Timetable> {
    timetables
        .into_iter()
        .map(|timetable| {
            timetable
                .0
                .into_iter()
                .map(|timing| Timing::new(timing))
                .collect()
        })
        .collect()
}

fn convert_gesture_sets<'a>(gesture_sets: Vec<RawGestureSet>) -> Vec<GestureSet<'a>> {
    gesture_sets
        .into_iter()
        .map(|set| set.0.into_iter().map(convert_gesture).collect())
        .collect()
}

fn convert_gesture<'a>(gesture: RawGesture) -> Gesture<'a> {
    GestureParser::new(gesture).parse()
}

struct GestureParser<'a> {
    channel: u4, // Pointless really
    raw: Option<RawGesture>,
    gesture: Gesture<'a>,
    voice_map: [u8; 16],
    params: TimedParamSet,
    delay: u32,
}

impl<'a> GestureParser<'a> {
    fn new(raw: RawGesture) -> Self {
        let channel = 0.into();

        let params = TimedParamSet::default();

        // Gesture should start by resetting Params to default state
        let gesture = params
            .0
            .iter()
            .map(|param| TrackEvent {
                delta: 0.into(),
                kind: TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::Controller {
                        controller: param.controller,
                        value: param.midi_value(),
                    },
                },
            })
            .collect();

        Self {
            channel,
            raw: Some(raw),
            gesture,
            voice_map: [NO_NOTE; 16],
            params,
            delay: 0,
        }
    }

    fn parse(mut self) -> Gesture<'a> {
        let raw = self.raw.take().unwrap().0;
        for event in raw {
            // Params may modify current delay
            self.update_params();

            let delta = self.delay.into();
            match event {
                BmsInstruction::NoteOn {
                    voice,
                    note,
                    velocity,
                    duration,
                } => {
                    let voice = voice as usize;

                    self.voice_map[voice] = note;
                    self.gesture.push(TrackEvent {
                        delta,
                        kind: TrackEventKind::Midi {
                            channel: self.channel,
                            message: MidiMessage::NoteOn {
                                key: note.into(),
                                vel: velocity.into(),
                            },
                        },
                    });

                    if let Some(duration) = duration {
                        // May have active params while note is running
                        self.delay = duration.get();
                        self.update_params();

                        self.gesture.push(TrackEvent {
                            delta: self.delay.into(),
                            kind: TrackEventKind::Midi {
                                channel: self.channel,
                                message: MidiMessage::NoteOff {
                                    key: self.voice_map[voice as usize].into(),
                                    vel: DEFAULT_VELOCITY,
                                },
                            },
                        });
                        self.voice_map[voice as usize] = NO_NOTE;
                    }

                    self.delay = 0;
                }
                BmsInstruction::NoteOff { voice, release } => {
                    let voice = voice as usize;
                    self.gesture.push(TrackEvent {
                        delta,
                        kind: TrackEventKind::Midi {
                            channel: self.channel,
                            message: MidiMessage::NoteOff {
                                key: self.voice_map[voice].into(),
                                vel: release.into(),
                            },
                        },
                    });
                    self.voice_map[voice] = NO_NOTE;
                    self.delay = 0;
                }
                BmsInstruction::Wait { duration } => {
                    self.delay += duration as u32;
                }
                BmsInstruction::SetParam { param, value, time } => {
                    let param = &mut self.params[param];
                    if let Some(time) = time {
                        // Param is changed over time
                        // This is annoying to do in MIDI
                        // Set up the param to be updated later
                        param.time = time.get();
                        param.delta = (value - param.current) / param.time as f32;
                    } else {
                        // Param is changed immediately
                        // This too is annoying because BMS is eager and MIDI is not
                        // Set the param to this value and insert the CC with the built-up delay
                        param.time = 0;
                        param.delta = 0.0;
                        param.current = value;

                        self.gesture.push(TrackEvent {
                            delta,
                            kind: TrackEventKind::Midi {
                                channel: self.channel,
                                message: MidiMessage::Controller {
                                    controller: param.controller,
                                    value: param.midi_value(),
                                },
                            },
                        });
                        self.delay = 0;
                    }
                }
                BmsInstruction::Call {
                    condition: _,
                    address,
                } => {
                    // This should probably never be an issue but better safe than sorry
                    match address {
                        GESTURE_START_SUBROUTINE => (),
                        _ => panic!("Unknown subroutine at address {:#X}", address),
                    }
                }
                BmsInstruction::Finish => {
                    self.gesture.push(TrackEvent {
                        delta,
                        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
                    });
                    break;
                }
            };
        }
        self.gesture
    }

    fn update_params(&mut self) {
        while self.delay > 0 {
            let mut delta = 1;
            for param in &mut self.params.0 {
                if param.update() {
                    self.gesture.push(TrackEvent {
                        delta: delta.into(),
                        kind: TrackEventKind::Midi {
                            channel: self.channel,
                            message: MidiMessage::Controller {
                                controller: param.controller,
                                value: param.midi_value(),
                            },
                        },
                    });
                    self.delay -= delta;
                    // Force delta to 0 so the rest of the params update at the same time
                    delta = 0;
                }
            }
            // If delta hasn't been zeroed, no params were updated
            if delta != 0 {
                break;
            }
        }
    }
}

#[derive(Default)]
struct TimedParam {
    controller: u7,
    current: f32,
    delta: f32,
    time: u32,
}

impl TimedParam {
    fn update(&mut self) -> bool {
        if self.time > 0 {
            self.current += self.delta;
            self.time -= 1;
            true
        } else {
            false
        }
    }
    fn midi_value(&self) -> u7 {
        ((self.current * 127.0) as u8).into()
    }
}
struct TimedParamSet([TimedParam; ParamType::count()]);

impl Default for TimedParamSet {
    fn default() -> Self {
        let mut params = Self([TimedParam::default(); ParamType::count()]);

        params[ParamType::Volume].controller = ParamType::Volume.controller();
        params[ParamType::Volume].current = 1.0;

        params
    }
}

impl Index<ParamType> for TimedParamSet {
    type Output = TimedParam;

    fn index(&self, index: ParamType) -> &Self::Output {
        &self.0[index as usize]
    }
}

impl IndexMut<ParamType> for TimedParamSet {
    fn index_mut(&mut self, index: ParamType) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}
