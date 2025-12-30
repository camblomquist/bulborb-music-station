use std::{fs, mem, num::NonZeroU32, ops::Index, path::Path, slice::SliceIndex};

const LIST_SENTINEL: [u8; 3] = [0xFF, 0xFF, 0xFF];

const GESTURE_SET_LIST_ADDRESS_LOCATION: usize = 0x6;
const TIMETABLE_LIST_ADDRESS_LOCATION: usize = 0x14;

#[derive(Debug)]
pub struct Bms {
    pub gesture_sets: Vec<RawGestureSet>,
    pub timetables: Vec<RawTimetable>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
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
    SetParam {
        param: ParamType,
        value: f32,
        time: Option<NonZeroU32>,
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

struct BmsParser<'a> {
    data: &'a [u8],
    pos: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Volume = 0,
}

impl Bms {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Self {
        let data = fs::read(path).unwrap();
        let parser = BmsParser::new(&data, 0);

        let gesture_sets = parser.parse_gesture_sets();
        let timetables = parser.parse_timetables();

        Self {
            gesture_sets,
            timetables,
        }
    }
}

impl<'a> BmsParser<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    fn parse_gesture_sets(&self) -> Vec<RawGestureSet> {
        let address = self.parse_list_location(GESTURE_SET_LIST_ADDRESS_LOCATION);
        let set_locations = self.iter_address_list(address);
        let gesture_locations = set_locations.map(|address| self.iter_address_list(address));

        gesture_locations
            .map(|set| RawGestureSet(set.map(|address| self.parse_gesture(address)).collect()))
            .collect()
    }

    fn parse_gesture(&self, address: usize) -> RawGesture {
        let mut instructions = Vec::new();

        let mut parser = Self::new(self.data, address);
        let mut next = parser.parse_instruction();
        while next != BmsInstruction::Finish {
            instructions.push(next);
            next = parser.parse_instruction();
        }

        instructions.push(BmsInstruction::Finish);

        RawGesture(instructions)
    }

    fn parse_timetables(&self) -> Vec<RawTimetable> {
        let address = self.parse_list_location(TIMETABLE_LIST_ADDRESS_LOCATION);
        let timetable_locations = self.iter_address_list(address);

        timetable_locations
            .map(|address| {
                RawTimetable(
                    self.data[(address as _)..]
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

    fn parse_instruction(&mut self) -> BmsInstruction {
        use opcodes::*;

        // Original decoder does some fancy bitmasking to multiplex opcodes
        // We are lazy and will just match on the opcode, code duplication be damned
        let opcode = self.read_u8();
        match opcode {
            NOTE_ON_BEGIN..=NOTE_ON_END => {
                let note = opcode;
                let flags = self.read_u8();
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

                let velocity = self.read_u8();
                if velocity & 0x80 != 0 {
                    panic!("NoteOn Unhandled Flag: Velocity from Register");
                }

                let duration = if voice == 0 {
                    // This byte is used as a factor to get Dsp Time from Seq Time
                    // We shouldn't need to care about this?
                    self.read_u8();

                    let count = (flags >> 3) & 0x3;
                    match count {
                        0 => panic!("NoteOn without duration"),
                        1 => NonZeroU32::new(self.read_u8() as _),
                        2 => NonZeroU32::new(self.read_u16() as _),
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
                    let rel = self.read_u8();
                    if rel > 100 { (rel - 98) * 20 } else { rel }
                } else {
                    0
                };
                BmsInstruction::NoteOff { voice, release }
            }
            CMD_WAIT8 => {
                let duration = self.read_u8() as u16;
                BmsInstruction::Wait { duration }
            }
            CMD_WAIT16 => {
                let duration = self.read_u16();
                BmsInstruction::Wait { duration }
            }
            PARAM_S16_NODUR => {
                let param = self.read_u8();
                let param = match param {
                    x if x == ParamType::Volume as u8 => ParamType::Volume,
                    _ => panic!("SetParam Unhandled Param: {}", param),
                };

                let value = self.read_i16() as f32 / 32767.0;
                let time = None;

                BmsInstruction::SetParam { param, value, time }
            }
            PARAM_S16_DUR_U8 => {
                let param = self.read_u8();
                let param = match param {
                    x if x == ParamType::Volume as u8 => ParamType::Volume,
                    _ => panic!("SetParam Unhandled Param: {}", param),
                };

                let value = self.read_i16() as f32 / 32767.0;
                let time = NonZeroU32::new(self.read_u8() as _);

                BmsInstruction::SetParam { param, value, time }
            }
            CALL => {
                let condition = self.read_u8();
                let address = self.read_u24();
                BmsInstruction::Call { condition, address }
            }
            FINISH => BmsInstruction::Finish,
            _ => panic!("unknown opcode {:#2X}", opcode),
        }
    }

    fn parse_list_location(&self, address: usize) -> usize {
        let slice = &self.data[address..];
        assert!(slice.len() >= mem::size_of::<u16>());
        let address = unsafe { slice.first_chunk().unwrap_unchecked() };
        u16::from_be_bytes(*address) as _
    }

    fn iter_address_list(&self, address: usize) -> impl Iterator<Item = usize> {
        let chunks = self.data[address..].as_chunks().0.iter();
        chunks
            .take_while(|&&x| x != LIST_SENTINEL)
            .map(|&[x, y, z]| u32::from_be_bytes([0, x, y, z]) as _)
    }

    fn read_u8(&mut self) -> u8 {
        let ret = self.data[self.pos];
        self.pos += 1;
        ret
    }

    fn read_u16(&mut self) -> u16 {
        let bytes = self.data[self.pos..self.pos + 2].try_into().unwrap();
        let ret = u16::from_be_bytes(bytes);
        self.pos += 2;
        ret
    }

    fn read_i16(&mut self) -> i16 {
        self.read_u16() as _
    }

    fn read_u24(&mut self) -> u32 {
        let bytes = &self.data[self.pos..self.pos + 3];
        let ret = u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]);
        self.pos += 3;
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
        NOTE_ON_BEGIN    = 0x00,
        NOTE_ON_END      = 0x79,
        CMD_WAIT8        = 0x80,
        NOTE_OFF_BEGIN   = 0x81,
        NOTE_OFF_END     = 0x87,
        CMD_WAIT16       = 0x88,
        PARAM_S16_NODUR  = 0x9C,
        PARAM_S16_DUR_U8 = 0x9E,
        CALL             = 0xC4,
        FINISH           = 0xFF,
    }
}
