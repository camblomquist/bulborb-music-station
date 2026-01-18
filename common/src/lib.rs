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
    pub tracks: Vec<CompositionTrack>,
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
pub struct CompositionTrack {
    pub title: String,
    pub bank: u32,
    pub program: u8,
    pub skippable: bool, // PikNum from TrackMap
    pub unison_track: Option<usize>,
    pub initial_delay: u32,
    pub extra_delay: u32, // eighthDelay, Is there a music theory name for this?
    pub in_order: bool,
    pub gesture_set: usize,
    pub timetable: usize,
    pub on_cycles: u32,
    pub off_cycles: u32,
    pub transposition: i8,
    pub volume: u8,
    pub panning: u8,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timing(u16);

pub type Timetable = Vec<Timing>;
pub type GestureSet<'a> = Vec<Gesture<'a>>;
pub type Gesture<'a> = midly::Track<'a>;

impl<'a> Composition<'a> {
    pub fn gesture_set_for_track(&self, track_index: usize) -> &GestureSet<'a> {
        &self.gesture_sets[self.tracks[track_index].gesture_set]
    }
}

impl Timing {
    pub fn new(value: u16) -> Self {
        Self(value)
    }

    pub fn cutoff(&self) -> bool {
        self.0 & 0x8000 != 0
    }

    pub fn delta(&self) -> u32 {
        (self.0 & 0x7FFF) as u32
    }
}
