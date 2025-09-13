use std::ffi::CStr;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

#[derive(Debug)]
pub struct RawConductor {
    pub swing_time: u8,
    pub bpm: u8,
    pub tracks: Vec<RawConductorTrack>,
}

#[derive(Debug)]
pub struct RawConductorTrack {
    pub text: [u8; 8],
    pub initial_delay: u8,
    pub eighth_delay_b: bool,
    pub eighth_delay_q: bool,
    pub track_copy: u8,
    pub echo: u8,
    pub in_order: bool,
    pub bank: u8,
    pub program: u8,
    pub gesture_set: u8,
    pub ruleset: u8,
    pub gesture_count: u8,
    pub silent_count: u8,
    pub transposition: i8,
    pub volume: u8,
    pub panning: i8,
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
                file.seek_relative(25).unwrap();
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
        // Two bytes before the first value here is a non-zero value. Seems to always be 0x01

        // Copying bjorn's homework because this is so much nicer than reading byte-by-byte
        let mut buf = [0; 2];
        file.read_exact(&mut buf).unwrap();
        let [initial_delay, eighth_delay_b] = buf;

        let mut buf = [0; 8];
        file.read_exact(&mut buf).unwrap();
        let text = buf;

        let mut buf = [0; 2];
        file.read_exact(&mut buf).unwrap();
        let [track_copy, echo] = buf;

        // Skip mostly empty space
        // Two bytes before this value is another non-zero value. Seems to always be 0x40
        file.seek_relative(8).unwrap();

        let mut buf = [0; 15];
        file.read_exact(&mut buf).unwrap();

        let [
            in_order,
            bank,
            program,
            _,
            gesture_set,
            _,
            ruleset,
            gesture_count,
            silent_count,
            _,
            transposition,
            volume,
            panning,
            eighth_delay_q,
            _,
        ] = buf;

        Self {
            text,
            initial_delay,
            eighth_delay_b: eighth_delay_b == 1,
            eighth_delay_q: eighth_delay_q == 1,
            track_copy,
            echo,
            in_order: in_order == 1,
            bank,
            program,
            gesture_set,
            ruleset,
            gesture_count,
            silent_count,
            transposition: transposition as i8,
            volume,
            panning: panning as i8 - 64,
        }
    }
}
