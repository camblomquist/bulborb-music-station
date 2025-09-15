use std::{fs, mem, num::NonZeroU32, ops::Index, path::Path, slice::SliceIndex};

const LIST_SENTINEL: [u8; 3] = [0xFF, 0xFF, 0xFF];

const GESTURE_SET_LIST_ADDRESS_LOCATION: usize = 0x6;
const TIMETABLE_LIST_ADDRESS_LOCATION: usize = 0x14;

pub struct Bms(Vec<u8>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmsInstruction {
    NoteOn {
        voice: u8,
        note: u8,
        velocity: u8,
        duration: Option<NonZeroU32>,
    },
    NoteOff {
        voice: u8,
        release: u8,
    },
    Wait {
        duration: u16,
    },
    Call {
        condition: u8,
        address: u32,
    },
    Finish,
}

#[derive(Debug)]
pub struct RawGestureSet(pub Vec<RawGesture>);

#[derive(Debug)]
pub struct RawGesture(pub Vec<BmsInstruction>);

#[derive(Debug)]
pub struct RawTimetable(pub Vec<u16>);

struct BmsState {
    pos: usize,
}

impl Bms {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Self {
        let data = fs::read(path).unwrap();
        Self(data)
    }

    pub fn parse_gesture_sets(&self) -> Vec<RawGestureSet> {
        let address = self.parse_list_location(GESTURE_SET_LIST_ADDRESS_LOCATION);
        let set_locations = self.iter_address_list(address);
        let gesture_locations = set_locations.map(|address| self.iter_address_list(address));

        gesture_locations
            .map(|set| RawGestureSet(set.map(|address| self.parse_gesture(address)).collect()))
            .collect()
    }

    pub fn parse_timetables(&self) -> Vec<RawTimetable> {
        let address = self.parse_list_location(TIMETABLE_LIST_ADDRESS_LOCATION);
        let timetable_locations = self.iter_address_list(address);

        timetable_locations
            .map(|address| {
                RawTimetable(
                    self.0[(address as _)..]
                        .as_chunks()
                        .0
                        .iter()
                        .take_while(|&x| x != &[0xFF, 0xFF])
                        .map(|&x| u16::from_be_bytes(x))
                        .collect(),
                )
            })
            .collect()
    }

    fn parse_gesture(&self, address: usize) -> RawGesture {
        let mut instructions = Vec::new();

        let mut state = BmsState { pos: address };
        let mut next = self.parse_instruction(&mut state);
        while next != BmsInstruction::Finish {
            instructions.push(next);
            next = self.parse_instruction(&mut state);
        }

        RawGesture(instructions)
    }

    fn parse_list_location(&self, address: usize) -> usize {
        let slice = &self.0[address..];
        assert!(slice.len() >= mem::size_of::<u16>());
        let address = unsafe { slice.first_chunk().unwrap_unchecked() };
        u16::from_be_bytes(*address) as _
    }

    fn iter_address_list(&self, address: usize) -> impl Iterator<Item = usize> {
        let chunks = self.0[address..].as_chunks().0.iter();
        chunks
            .take_while(|&&x| x != LIST_SENTINEL)
            .map(|&[x, y, z]| u32::from_be_bytes([0, x, y, z]) as _)
    }

    fn parse_instruction(&self, state: &mut BmsState) -> BmsInstruction {
        use opcodes::*;

        let opcode = self.read_u8(state);
        match opcode {
            NOTE_ON_BEGIN..=NOTE_ON_END => {
                let note = opcode;
                let flags = self.read_u8(state);
                let voice = flags & 0x7;
                let extended_flags = (flags >> 5) & 0x3;

                if extended_flags & 0x4 != 0 {
                    panic!("NoteOn Unhandled Flag: Note from Register");
                }

                if extended_flags & 0x2 != 0 {
                    panic!("NoteOn Unhandled Flag: Portamento");
                }

                if extended_flags & 0x1 != 0 {
                    panic!("NoteOn Unhandled Flag: GateOn");
                }

                let velocity = self.read_u8(state);
                if velocity & 0x80 != 0 {
                    panic!("NoteOn Unhandled Flag: Velocity from Register");
                }

                let duration = if voice == 0 {
                    // This byte is used as a factor to get Dsp Time from Seq Time
                    // We shouldn't need to care about this?
                    self.read_u8(state);

                    let count = (flags >> 3) & 0x3;
                    match count {
                        0 => panic!("NoteOn without duration"),
                        1 => NonZeroU32::new(self.read_u8(state) as _),
                        2 => NonZeroU32::new(self.read_u16(state) as _),
                        3 => panic!("NoteOn Unhandled Flag: 24 byte sequence time"),
                        _ => unreachable!(),
                    }
                } else {
                    None
                };

                if voice != 0 && extended_flags & 1 != 0 {
                    panic!("NoteOn Unhandled Flag: Magic Register Read");
                }

                BmsInstruction::NoteOn {
                    voice,
                    note,
                    velocity,
                    duration,
                }
            }
            NOTE_OFF_BEGIN..=NOTE_OFF_END => {
                let voice = opcode & 0x7;
                let release = if opcode & 0x08 != 0 {
                    let rel = self.read_u8(state);
                    if rel > 100 { (rel - 98) * 20 } else { rel }
                } else {
                    0
                };
                BmsInstruction::NoteOff { voice, release }
            }
            CMD_WAIT8 => {
                let duration = self.read_u8(state) as u16;
                BmsInstruction::Wait { duration }
            }
            CMD_WAIT16 => {
                let duration = self.read_u16(state);
                BmsInstruction::Wait { duration }
            }
            CALL => {
                let condition = self.read_u8(state);
                let address = self.read_u24(state);
                BmsInstruction::Call { condition, address }
            }
            FINISH => BmsInstruction::Finish,
            _ => panic!("unknown opcode {:#2X}", opcode),
        }
    }

    fn read_u8(&self, state: &mut BmsState) -> u8 {
        let ret = self.0[state.pos];
        state.pos += 1;
        ret
    }

    fn read_u16(&self, state: &mut BmsState) -> u16 {
        let bytes = self.0[state.pos..state.pos + 2].try_into().unwrap();
        let ret = u16::from_be_bytes(bytes);
        state.pos += 2;
        ret
    }

    fn read_u24(&self, state: &mut BmsState) -> u32 {
        let bytes = &self.0[state.pos..state.pos + 3];
        let ret = u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]);
        state.pos += 3;
        ret
    }
}

mod opcodes {
    macro_rules! opcodes {
        ( $($i:ident = $x:literal),+ $(,)?) => {
            $(pub const $i: u8 = $x;)*
        };
    }

    opcodes! {
        NOTE_ON_BEGIN  = 0x00,
        NOTE_ON_END    = 0x79,
        CMD_WAIT8      = 0x80,
        NOTE_OFF_BEGIN = 0x81,
        NOTE_OFF_END   = 0x87,
        CMD_WAIT16     = 0x88,
        CALL           = 0xC4,
        FINISH         = 0xFF,
    }
}
