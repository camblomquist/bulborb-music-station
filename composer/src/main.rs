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

    // let timetables = Bms::from_file("new_05.bms").parse_timetables();
    // println!("{:?}", timetables);

    let gesture_sets = Bms::from_file("new_05.bms").parse_gesture_sets();
    for (i, set) in gesture_sets.iter().enumerate() {
        println!("Set {}", i);
        for gesture in &set.0 {
            println!("  {:?}", gesture);
        }
    }
}
