use crate::conductor::RawConductor;

mod conductor;

fn main() {
    let cnd = RawConductor::from_file("caveconc_05_0.cnd");
    println!("{:?}\n", cnd);

    for track in &cnd.tracks {
        println!("{}", track.title());
    }
}
