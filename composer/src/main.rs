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

    let names = [
        "caveconc",
        "caverelax",
        "new_00",
        "new_01",
        "new_02",
        "new_03",
        "new_04",
        "new_05",
        "new_06",
        "new_07",
        "new_08",
        "cavesoil",
        "caveglass",
        "cavemetal",
        "cavetsumiki",
        "cavetile",
        "cavekeyget",
    ];

    for name in &names {
        println!("Extracting {}", name);
        let bms = Bms::from_file(format!("data/bms/{}.bms", name));
        println!("  Gesture Sets: {}", bms.gesture_sets.len());
        println!("  Timetables: {}", bms.timetables.len());
    }
}
