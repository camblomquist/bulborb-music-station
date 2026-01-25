#![allow(dead_code)]
use std::error::Error;

use bms_common::Composition;
use bms_composer::{
    bms::Bms, conductor::RawConductor, convert::RawComposition, trackmap::RawTrackMap,
};

use crate::player::Player;

mod clock;
mod player;
mod synthext;

fn main() {
    run().unwrap();
}

fn run() -> Result<(), Box<dyn Error>> {
    // let composition = load_new_05();
    // let composition = load_caveglass();
    // let composition = load_caverelax();
    let composition = load_caveconc();

    let player = Player::new(composition);
    player.play()?;

    Ok(())
}

fn load_new_05<'a>() -> Composition<'a> {
    println!("Loading new_05");
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

fn load_caveglass<'a>() -> Composition<'a> {
    println!("Loading caveglass_00_0");
    let bms = Bms::from_file("./data/bms/caveglass.bms");
    let conductor = RawConductor::from_file("./data/cnd/caveglass_00_0.cnd");
    let trackmap = RawTrackMap::from_file("./data/trackmap/Totaka/trackMap_Seq_T.txt");
    let name = String::from("caveglass.bms");
    RawComposition {
        name,
        bms,
        conductor,
        trackmap,
    }
    .convert()
}

fn load_caverelax<'a>() -> Composition<'a> {
    println!("Loading caverelax");
    let bms = Bms::from_file("./data/bms/caverelax.bms");
    let conductor = RawConductor::from_file("./data/cnd/caverelax.cnd");
    let trackmap = RawTrackMap::from_file("./data/trackmap/Totaka/trackMap_Seq_T.txt");
    let name = String::from("caverelax.bms");
    RawComposition {
        name,
        bms,
        conductor,
        trackmap,
    }
    .convert()
}

fn load_caveconc<'a>() -> Composition<'a> {
    println!("Loading caveconc");
    let bms = Bms::from_file("./data/bms/caveconc.bms");
    let conductor = RawConductor::from_file("./data/cnd/caveconc_00_0.cnd");
    let trackmap = RawTrackMap::from_file("./data/trackmap/Totaka/trackMap_Cond_T.txt");
    let name = String::from("caveconc_00_0.cnd");
    RawComposition {
        name,
        bms,
        conductor,
        trackmap,
    }
    .convert()
}
