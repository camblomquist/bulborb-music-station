use crate::{conductor::RawConductor, trackmap::RawTrackMap};

mod conductor;
mod trackmap;

fn main() {
    // let cnd = RawConductor::from_file("caveconc_05_0.cnd");
    // println!("{:?}\n", cnd);

    // for track in &cnd.tracks {
    //     println!("{}", track.title());
    // }

    let trackmaps = RawTrackMap::from_file("trackMap_Seq_T.txt");
    println!("{:?}", trackmaps);
}
