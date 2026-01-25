use std::{
    collections::BinaryHeap,
    ops::{Index, IndexMut},
};

use crate::{
    bms::{Bms, BmsInstruction, BmsSubroutine, ParamType, RawGesture, RawGestureSet, RawTimetable},
    conductor::{RawConductor, RawConductorTrack},
    trackmap::{RawTrackMap, RawTrackMapEntry},
};

use bms_common::{Composition, CompositionTrack, Gesture, GestureSet, Mix, Timetable, Timing};
use midly::{
    MetaMessage, MidiMessage, PitchBend, TrackEvent, TrackEventKind,
    num::{u4, u7},
};

const NO_NOTE: u8 = 0xFF;
const DEFAULT_VELOCITY: u8 = 64;

const PITCH_BEND_RANGE: f32 = 12.0; // Assume 12 semitone range for pitch bend

// const PPQ: u16 = 120;

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
        let tempo = (60_000_000.0 / conductor.bpm as f64) as _;
        Composition {
            name,
            tempo,
            beat_interval: conductor.beat_interval as _,
            skip_notes: trackmap.skip_notes,
            mixes: convert_mixes(trackmap),
            tracks: convert_tracks(conductor.tracks, trackmap),
            timetables: convert_timetables(bms.timetables),
            gesture_sets: convert_gesture_sets(bms.gesture_sets),
            instruments: (),
        }
    }
}
fn convert_mixes(trackmap: &RawTrackMapEntry) -> Vec<Mix> {
    let mut end = 0;
    trackmap
        .mix_track_counts
        .into_iter()
        .map(|count| {
            let start = end;
            end += count as usize;
            Mix {
                tracks: (start..end).collect(),
            }
        })
        .collect()
}

fn convert_tracks(
    tracks: Vec<RawConductorTrack>,
    trackmap: &RawTrackMapEntry,
) -> Vec<CompositionTrack> {
    tracks
        .into_iter()
        .zip(trackmap.skip_tracks)
        .map(|(track, skippable)| {
            let title = String::from(track.title());
            let unison_track = if track.unison_track != 16 {
                Some(track.unison_track as usize)
            } else {
                None
            };
            // Convert beats to clock ticks
            let initial_delay = track.initial_delay as u32 * 120;
            // Based on how JAS interprets the eighthDelay, it should be OK to combine them
            let extra_delay = match (track.eighth_delay, track.module.eighth_delay) {
                (true, true) => 120,
                (false, false) => 0,
                _ => 60,
            };
            CompositionTrack {
                title,
                bank: track.module.bank as _,
                program: track.module.program,
                skippable,
                unison_track,
                initial_delay,
                extra_delay,
                in_order: track.module.in_order,
                gesture_set: track.module.gesture_set as _,
                timetable: track.module.timing_ruleset as _,
                on_cycles: track.module.on_cycles as _,
                off_cycles: track.module.off_cycles as _,
                transposition: track.module.transposition as _,
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
    channel: u4,
    raw: Option<RawGesture>,
    events: BinaryHeap<Event<'a>>,
    voice_map: [u8; 16],
    params: ParamSet,
    time: u32,
    counter: Counter,
}

impl<'a> GestureParser<'a> {
    fn new(raw: RawGesture) -> Self {
        let channel = u4::new(0);
        let params = ParamSet::default();

        let time = 0;
        let mut counter = Counter::default();

        // Gesture should start by resetting Params to default state
        let events = params
            .0
            .iter()
            .map(|param| Event {
                time,
                counter: counter.get(),
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
            events,
            voice_map: [NO_NOTE; 16],
            params,
            time,
            counter,
        }
    }

    fn parse(mut self) -> Gesture<'a> {
        let raw = self.raw.take().unwrap().0;
        for [event, next_event] in raw.array_windows() {
            match *event {
                BmsInstruction::NoteOn {
                    voice,
                    note,
                    velocity,
                    duration,
                } => {
                    let voice = voice as usize;

                    self.voice_map[voice] = note;
                    self.push_event(TrackEventKind::Midi {
                        channel: self.channel,
                        message: MidiMessage::NoteOn {
                            key: note.into(),
                            vel: velocity.into(),
                        },
                    });

                    if let Some(duration) = duration {
                        self.time += duration.get();

                        // Only add the note off if the next instruction doesn't modify this note
                        if !next_event.modifies_note() {
                            self.push_note_off(voice, None);
                        }
                    }
                }
                BmsInstruction::GateOn {
                    note,
                    velocity,
                    duration,
                } => {
                    // A Gate can only modify an existing note on voice 0
                    // Use the base note as the reference point
                    let base_note = self.voice_map[0];

                    // Treat the note gate as an instantaneous pitch bend
                    let bend = (note as i16 - base_note as i16) as f32 / PITCH_BEND_RANGE;
                    assert!(bend < 1.0);
                    let bend = 0x2000 + (bend * 0x2000 as f32) as u16;

                    self.push_event(TrackEventKind::Midi {
                        channel: self.channel,
                        message: MidiMessage::PitchBend {
                            bend: PitchBend(bend.into()),
                        },
                    });
                    // Treat the velocity gate as aftertouch?
                    self.push_event(TrackEventKind::Midi {
                        channel: self.channel,
                        message: MidiMessage::ChannelAftertouch {
                            vel: velocity.into(),
                        },
                    });

                    self.time += duration;

                    // Only add the note off and reset bend if the next instruction doesn't modify this note
                    if !next_event.modifies_note() {
                        self.push_note_off(0, None);
                        self.push_event(TrackEventKind::Midi {
                            channel: self.channel,
                            message: MidiMessage::PitchBend {
                                bend: PitchBend(0.into()),
                            },
                        });
                    }
                }
                BmsInstruction::Portamento {
                    from_note,
                    to_note,
                    velocity,
                    duration,
                } => {
                    // Although portamento doesn't strictly require it, it always seems to be used with a gate
                    // There are two instances where this isn't the case that appear unintentional
                    // We treat these as gates anyway and thus still work on the base note
                    let base_note = self.voice_map[0];

                    // Even if we aren't at the base note, we need to ensure we're within range of it
                    let total_bend = (to_note as i16 - base_note as i16) as f32 / PITCH_BEND_RANGE;
                    assert!(total_bend < 1.0);

                    // Use from_note as base bend
                    let base_bend = (from_note as i16 - base_note as i16) as f32 / PITCH_BEND_RANGE;
                    let base_bend = 0x2000 + (base_bend * 0x2000 as f32) as u16;

                    let bend = (to_note as i16 - from_note as i16) as f32 / PITCH_BEND_RANGE;
                    let bend_increment = ((bend * 0x2000 as f32) / duration as f32) as u16;

                    let mut bend = base_bend;
                    // This this has to include the duration?
                    // Starts with the equivalent of a gate then sweeps
                    // Without the final step, we don't actually reach it?
                    for _ in 0..=duration {
                        self.push_event(TrackEventKind::Midi {
                            channel: self.channel,
                            message: MidiMessage::PitchBend {
                                bend: PitchBend(bend.into()),
                            },
                        });
                        bend += bend_increment;
                        self.time += 1;
                    }

                    // Still treating velocity as aftertouch as it doesn't get changed over time
                    // Treat the velocity gate as aftertouch?
                    self.push_event(TrackEventKind::Midi {
                        channel: self.channel,
                        message: MidiMessage::ChannelAftertouch {
                            vel: velocity.into(),
                        },
                    });

                    // Only add the note off and reset bend if the next instruction doesn't modify this note
                    if !next_event.modifies_note() {
                        self.push_note_off(0, None);
                        self.push_event(TrackEventKind::Midi {
                            channel: self.channel,
                            message: MidiMessage::PitchBend {
                                bend: PitchBend(0.into()),
                            },
                        });
                    }
                }
                BmsInstruction::NoteOff { voice, release } => {
                    let voice = voice as usize;
                    self.push_note_off(voice, Some(release));
                }
                BmsInstruction::Wait { duration } => {
                    self.time += duration as u32;
                }
                BmsInstruction::SetParam { param, value, time } => {
                    if let Some(time) = time {
                        // Note that this doe *not* update the current track time
                        let time = time.get();
                        let delta = (value - self.params[param].value) / time as f32;
                        for delay in 0..time {
                            self.params[param].value += delta;
                            self.push_delayed_event(
                                delay,
                                TrackEventKind::Midi {
                                    channel: self.channel,
                                    message: MidiMessage::Controller {
                                        controller: self.params[param].controller,
                                        value: self.params[param].midi_value(),
                                    },
                                },
                            );
                        }
                    } else {
                        self.params[param].value = value;

                        self.push_event(TrackEventKind::Midi {
                            channel: self.channel,
                            message: MidiMessage::Controller {
                                controller: self.params[param].controller,
                                value: self.params[param].midi_value(),
                            },
                        });
                    }
                }
                BmsInstruction::Call {
                    condition: _,
                    subroutine,
                } => {
                    match subroutine {
                        BmsSubroutine::GestureStart => {
                            // TODO: Figure out what this needs to do
                            // 15D outswitch 0x0
                            // 15F param_set_8 0x8 1
                            // 162 param_set_8 0x9 0
                            // 165 param_set_8 0xA 1
                            // 168 panswset 52 33 52
                        }
                    }
                }
                BmsInstruction::Finish => {
                    // Likely unreachable due to windows
                    break;
                }
            };
        }

        // Because we look at the windows, the last event never gets added
        // That last event should always be Finish so we can just add it now
        self.push_event(TrackEventKind::Meta(MetaMessage::EndOfTrack));

        let mut gesture = Gesture::with_capacity(self.events.len());
        let mut prev_time = 0;
        while let Some(event) = self.events.pop() {
            let delta = event.time - prev_time;
            gesture.push(TrackEvent {
                delta: delta.into(),
                kind: event.kind,
            });
            prev_time = event.time;
        }

        gesture
    }

    fn push_event(&mut self, kind: TrackEventKind<'a>) {
        self.events.push(Event {
            time: self.time,
            counter: self.counter.get(),
            kind,
        });
    }

    fn push_delayed_event(&mut self, delay: u32, kind: TrackEventKind<'a>) {
        self.events.push(Event {
            time: self.time + delay,
            counter: self.counter.get(),
            kind,
        });
    }

    fn push_note_off(&mut self, voice: usize, velocity: Option<u8>) {
        self.push_event(TrackEventKind::Midi {
            channel: self.channel,
            message: MidiMessage::NoteOff {
                key: self.voice_map[voice].into(),
                vel: velocity.unwrap_or(DEFAULT_VELOCITY).into(),
            },
        });

        self.voice_map[voice] = NO_NOTE;
    }
}

#[derive(Default)]
struct Param {
    controller: u7,
    value: f32,
}

impl Param {
    fn midi_value(&self) -> u7 {
        ((self.value * 127.0) as u8).into()
    }
}

struct ParamSet([Param; ParamType::count()]);

impl Default for ParamSet {
    fn default() -> Self {
        let mut params = Self([Param::default(); ParamType::count()]);

        params[ParamType::Volume].controller = ParamType::Volume.controller();
        params[ParamType::Volume].value = 1.0;

        params
    }
}

impl Index<ParamType> for ParamSet {
    type Output = Param;

    fn index(&self, index: ParamType) -> &Self::Output {
        &self.0[index as usize]
    }
}

impl IndexMut<ParamType> for ParamSet {
    fn index_mut(&mut self, index: ParamType) -> &mut Self::Output {
        &mut self.0[index as usize]
    }
}

#[derive(Default)]
struct Counter(u32);

impl Counter {
    fn get(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }
}

// Track Event with an absolute time and an extra value to ensure stability
struct Event<'a> {
    time: u32,
    counter: u32,
    kind: TrackEventKind<'a>,
}

impl<'a> Ord for Event<'a> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Lower times evaluate as higher priority
        // NB: event kind is not a factor, must ensure unique counter value
        other
            .time
            .cmp(&self.time)
            .then(other.counter.cmp(&self.counter))
    }
}

impl<'a> PartialOrd for Event<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> PartialEq for Event<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time && self.counter == other.counter
    }
}

impl<'a> Eq for Event<'a> {}
