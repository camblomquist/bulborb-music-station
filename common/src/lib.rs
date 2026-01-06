use midly;

// A single composition
// For now just storing everything needed to play the composition
// Can split into separate parts if it presents issues later
#[derive(Debug)]
pub struct Composition<'a> {
    pub name: String,
    pub tempo: u32,
    pub swing_time: u8,
    pub skip_notes: [bool; 8], // PikMask from Track Map
    pub mixes: Vec<Mix>,
    pub tracks: Vec<Track>,
    pub timetables: Vec<Timetable>,
    pub gesture_sets: Vec<GestureSet<'a>>,
    pub instruments: (),
}

#[derive(Debug)]
pub struct Mix {
    // List of Track Indices associated with this mix
    // TrackMap constrains this further for space savings
    // There really isn't a good reason to do it this way instead
    pub tracks: Vec<usize>,
}

#[derive(Debug)]
pub struct Track {
    pub title: String,
    pub bank: u32,
    pub program: u8,
    pub skippable: bool, // PikNum from TrackMap
    pub unison_track: Option<u8>,
    pub initial_delay: u16,
    pub extra_delay: u16, // eighthDelay, Is there a music theory name for this?
    pub in_order: bool,
    pub gesture_set: usize,
    pub timetable: usize,
    pub on_cycles: u8,
    pub off_cycles: u8,
    pub transposition: i16,
    pub volume: u8,
    pub panning: u8,
}

#[derive(Debug)]
pub struct Timing(u16);

pub type Timetable = Vec<Timing>;
pub type GestureSet<'a> = Vec<Gesture<'a>>;
pub type Gesture<'a> = midly::Track<'a>;

impl Timing {
    pub fn new(value: u16) -> Self {
        Self(value)
    }

    pub fn cutoff(&self) -> bool {
        self.0 & 0x8000 != 0
    }

    pub fn delta(&self) -> u16 {
        self.0 & 0x7FFF
    }
}
