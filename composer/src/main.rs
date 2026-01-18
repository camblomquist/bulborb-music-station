#![feature(variant_count)]

use midly::MetaMessage;

use crate::{bms::Bms, conductor::RawConductor, trackmap::RawTrackMap};

mod bms;
mod conductor;
mod convert;
mod trackmap;

fn main() {
    // let cnd = RawConductor::from_file("caveconc_05_0.cnd");
    // println!("{:?}\n", cnd);

    // for track in &cnd.tracks {
    //     println!("{}", track.title());
    // }

    // let trackmaps = RawTrackMap::from_file("trackMap_Seq_T.txt");
    // println!("{:?}", trackmaps);

    let names = ["caverelax", "new_05", "new_06", "new_07", "caveglass"];

    let _incomplete = [
        "cavemetal",
        "caveconc",
        "new_00",
        "new_01",
        "new_02",
        "new_03",
        "new_04",
        "new_08",
        "cavesoil",
        "cavetsumiki",
        "cavetile",
        "cavekeyget",
    ];

    // use midly::{
    //     MidiMessage, TrackEvent, TrackEventKind,
    //     num::{u4, u7, u28},
    // };
    // const CUTOFF: TrackEventKind = TrackEventKind::Midi {
    //     channel: u4::new(0),
    //     message: MidiMessage::NoteOff {
    //         key: u7::new(60),
    //         vel: u7::new(127),
    //     },
    // };
    // let event = TrackEvent {
    //     delta: u28::new(120 as _),
    //     kind: CUTOFF,
    // };
    // println!(
    //     "u28: {} u7: {} u4: {}",
    //     std::mem::size_of::<u28>(),
    //     std::mem::size_of::<u7>(),
    //     std::mem::size_of::<u4>()
    // );
    // println!("TrackEventKind: {}", std::mem::size_of::<TrackEventKind>());
    // println!("MetaMessage: {}", std::mem::size_of::<MetaMessage>());
    // println!("&[u8]: {}", std::mem::size_of::<&[u8]>());

    // println!("MidiMessage: {}", std::mem::size_of::<MidiMessage>());
    // println!("{}", std::mem::size_of_val(&event));
    for name in &names {
        println!("Extracting {}", name);
        let bms = Bms::from_file(format!("data/bms/{}.bms", name));
    }
}
