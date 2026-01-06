use std::{error::Error, fs::File, time::Duration};

use common::Composition;
use composer::{bms::Bms, conductor::RawConductor, convert::RawComposition, trackmap::RawTrackMap};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use midly::{MetaMessage, TrackEventKind};
use oxisynth::{MidiEvent, SoundFont, Synth};
use ringbuf::{
    HeapRb,
    traits::{Consumer, Producer, Split},
};

fn main() {
    // Load Composition
    // Create Synth
    // For Each Track:
    //  Spawn Gesture according to Timetable
    //  Play Gesture according to MIDI events
    // Need a way to sleep for tick duration
    //     May need to abstract this if not async
    // Channel per track should be fine?

    // sleep for a single tick, build up sub-millsecond delay
    // Keep track of update time, subtract from sleep time
    // Basically a game update loop
    run().unwrap();
}

fn run() -> Result<(), Box<dyn Error>> {
    let composition = load_composition();
    // println!("{:?}", composition.tracks);

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No output device available");

    let config = device.default_output_config().expect("No output config?");

    let mut synth = Synth::default();
    synth.set_sample_rate(config.sample_rate() as _);

    let mut font = File::open("./data/weedsgm3.sf2")?;
    let font = SoundFont::load(&mut font)?;
    let _font = synth.add_font(font, true);

    let latency = Duration::from_millis(10000);
    let latency_frames = latency.as_secs_f32() * config.sample_rate() as f32;
    let latency_samples = latency_frames as usize * config.channels() as usize;
    let mut buffer = vec![0f32; latency_samples].into_boxed_slice();
    let ring = HeapRb::<f32>::new(latency_samples * 2);
    let (mut producer, mut consumer) = ring.split();

    // Should instead send events to synth inside callback?
    // See https://github.com/PolyMeilex/Neothesia/blob/v0.3.1/neothesia/src/output_manager/synth_backend.rs
    let data_callback = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let count = consumer.pop_slice(data);
        if count < data.len() {
            eprintln!("underrun");
            data[count..].fill(0.0);
        }
    };
    let error_callback = |err: cpal::StreamError| {
        eprintln!("an error occurred on stream: {err}");
    };

    println!("Building stream");
    let stream = device.build_output_stream(&config.into(), data_callback, error_callback, None)?;
    println!("Playing stream");
    stream.play()?;

    // for (channel, track) in composition.tracks.iter().enumerate() {
    //     let channel = channel as u8;
    //     // synth.select_bank(channel, track.bank)?;
    //     // synth.select_program(channel, font, 0, track.program)?;
    // }
    // let track = &composition.tracks[0];
    let channel = 0;

    // let gesture_set = &composition.gesture_sets[track.gesture_set];

    for (i, gesture_set) in composition.gesture_sets.iter().enumerate() {
        println!("Playing gesture set {}", i);

        for (i, gesture) in gesture_set.iter().enumerate() {
            println!("Playing gesture {}", i);
            synth.send_event(MidiEvent::AllNotesOff { channel })?;

            for event in gesture {
                if event.delta.as_int() != 0 {
                    let mut delta =
                        ticks_to_duration(event.delta.as_int(), composition.tempo as _, 120);
                    while delta > Duration::ZERO {
                        let dur = delta.min(latency);
                        let portion = dur.div_duration_f32(latency);
                        let len = (latency_samples as f32 * portion) as usize;
                        let buffer = &mut buffer[..len];
                        synth.write(buffer.as_mut());
                        let count = producer.push_slice(&buffer);
                        if count < len {
                            eprintln!("overrun");
                        }
                        delta = delta.saturating_sub(dur);
                        wait(dur);
                    }
                }
                if let TrackEventKind::Midi {
                    channel: _,
                    message,
                } = event.kind
                {
                    let event = match message {
                        midly::MidiMessage::NoteOff { key, vel: _ } => Some(MidiEvent::NoteOff {
                            channel,
                            key: key.as_int(),
                        }),
                        midly::MidiMessage::NoteOn { key, vel } => Some(MidiEvent::NoteOn {
                            channel,
                            key: key.as_int(),
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
                    };
                    if let Some(event) = event {
                        synth.send_event(event)?;
                    }
                } else if let TrackEventKind::Meta(MetaMessage::EndOfTrack) = event.kind {
                    // println!("End of track");
                } else {
                    eprintln!("Received non-midi message");
                }
            }
        }
    }

    Ok(())
}

fn load_composition<'a>() -> Composition<'a> {
    println!("Loading Composition");
    let bms = Bms::from_file("./data/bms/new_05.bms");
    let conductor = RawConductor::from_file("./data/cnd/new_05_0.cnd");
    let trackmap = RawTrackMap::from_file("./data/trackmap/Totaka/trackMap_Cond_T.txt");
    let name = String::from("new_05_0.cnd");
    RawComposition {
        name,
        bms,
        conductor,
        trackmap,
    }
    .convert()
}

fn ticks_to_duration(ticks: u32, tempo: u32, tick_rate: u32) -> Duration {
    let time = ticks as f64 / tick_rate as f64;
    let time = (time * tempo as f64).floor() as u64;
    Duration::from_micros(time)
}

fn wait(dur: Duration) {
    std::thread::sleep(dur);
}
