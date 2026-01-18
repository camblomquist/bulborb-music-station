use std::ffi::CStr;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

use byteorder::{BigEndian, ReadBytesExt};

#[derive(Debug)]
pub struct RawConductor {
    pub swing_time: u8,
    pub bpm: u8,
    pub tracks: Vec<RawConductorTrack>,
}

#[derive(Debug)]
pub struct RawConductorTrack {
    pub text: [u8; 8],
    pub initial_delay: i16,
    pub eighth_delay: bool,
    pub unison_track: u8,
    pub echo: u8, // This appears to be effectively unused?
    pub module: RawConductorModule,
}

// A Track has modules as children which define most of the parameters
// There can theoretically be multiple modules but in practice, a track only has one
#[derive(Debug)]
pub struct RawConductorModule {
    pub in_order: bool,
    pub bank: u8,
    pub program: u8,
    pub gesture_set: u16,
    pub timing_ruleset: u16,
    pub on_cycles: u8,
    pub off_cycles: u8,
    pub transposition: i16,
    pub volume: u8,
    pub panning: u8,
    pub eighth_delay: bool,
}

impl RawConductor {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Self {
        let mut file = File::open(path).unwrap();

        let mut buf = [0; 3];
        file.read_exact(&mut buf).unwrap();
        let [swing_time, bpm, count] = buf;

        file.rewind().unwrap();

        let tracks = (0..count)
            .map(|_| {
                file.seek_relative(23).unwrap();
                RawConductorTrack::from_file(&mut file)
            })
            .collect();

        Self {
            swing_time,
            bpm,
            tracks,
        }
    }
}

impl RawConductorTrack {
    pub fn title(&self) -> &str {
        unsafe {
            CStr::from_bytes_until_nul(&self.text).map_or_else(
                |_| str::from_utf8_unchecked(&self.text),
                |text| CStr::to_str(text).unwrap_unchecked(),
            )
        }
    }

    fn from_file(file: &mut File) -> Self {
        // The first value here is the number of modules associated with the track
        // Since this is always 1, we ignore it
        let _ = file.read_u8().unwrap();

        let initial_delay = file.read_i16::<BigEndian>().unwrap();
        let eighth_delay = file.read_u8().unwrap();

        let mut buf = [0; 8];
        file.read_exact(&mut buf).unwrap();
        let text = buf;

        let unison_track = file.read_u8().unwrap();
        let echo = file.read_u8().unwrap();

        // Skip empty space between track and module
        file.seek_relative(3).unwrap();

        let module = RawConductorModule::from_file(file);

        Self {
            text,
            initial_delay,
            eighth_delay: eighth_delay == 1,
            unison_track,
            echo,
            module,
        }
    }
}

impl RawConductorModule {
    fn from_file(file: &mut File) -> Self {
        // Skip the first five bytes
        // These correspond to
        // - _64 an unused delay (i16)
        // - _94 an unknown i16 that is always set to 0x40
        // - _C4 an unknown u8 that appears to always be 0
        file.seek_relative(5).unwrap();

        let in_order = file.read_u8().unwrap();
        let bank = file.read_u8().unwrap();
        let program = file.read_u8().unwrap();
        let gesture_set = file.read_u16::<BigEndian>().unwrap();
        let timing_ruleset = file.read_u16::<BigEndian>().unwrap();
        let on_cycles = file.read_u8().unwrap();
        let off_cycles = file.read_u8().unwrap();
        let transposition = file.read_i16::<BigEndian>().unwrap();
        let volume = file.read_u8().unwrap();
        let panning = file.read_u8().unwrap();
        let eighth_delay = file.read_u8().unwrap();

        // The original parser skipped more than this
        // We only need to skip the one byte to align us for the next track
        file.seek_relative(1).unwrap();

        Self {
            in_order: in_order == 1,
            bank,
            program,
            gesture_set,
            timing_ruleset,
            on_cycles,
            off_cycles,
            transposition,
            volume,
            panning,
            eighth_delay: eighth_delay == 1,
        }
    }
}
