use crate::{bms::Bms, conductor::RawConductor, trackmap::RawTrackMap};

mod bms;
mod conductor;
mod trackmap;

fn main() {
    // let cnd = RawConductor::from_file("caveconc_05_0.cnd");
    // println!("{:?}\n", cnd);

    // for track in &cnd.tracks {
    //     println!("{}", track.title());
    // }

    // let trackmaps = RawTrackMap::from_file("trackMap_Seq_T.txt");
    // println!("{:?}", trackmaps);

    let bms = Bms::from_file("new_05.bms");
    for (i, set) in bms.gesture_sets.iter().enumerate() {
        println!("Set {}", i);
        for gesture in &set.0 {
            println!("  {:?}", gesture);
        }
    }
    println!("{:?}", bms.timetables);
}
