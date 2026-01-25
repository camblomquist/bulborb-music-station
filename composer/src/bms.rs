use std::{fs, mem, num::NonZeroU32, path::Path};

use midly::num::u7;

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
    GateOn {
        note: u8,
        velocity: u8,
        duration: u32,
    },
    Portamento {
        from_note: u8,
        to_note: u8,
        velocity: u8,
        duration: u32,
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
        subroutine: BmsSubroutine,
    },
    Finish,
}

impl BmsInstruction {
    pub fn modifies_note(&self) -> bool {
        match self {
            BmsInstruction::GateOn { .. } => true,
            _ => false,
        }
    }
}
// Directly storing the address isn't useful to us since we can't execute the bytecode
// Instead, we have a list of subroutines we know how to translate
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BmsSubroutine {
    GestureStart = 0x15D,
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
    prev_note: u8,
    prev_eflags: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Volume = 0,
}

impl ParamType {
    pub const fn count() -> usize {
        mem::variant_count::<Self>()
    }

    pub const fn controller(self) -> u7 {
        let value = match self {
            Self::Volume => 11,
        };
        u7::new(value)
    }
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
        Self {
            data,
            pos,
            prev_note: 0,
            prev_eflags: 0,
        }
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

                let (note, glide_note) = if extended_flags & 0x2 != 0 {
                    (self.prev_note, Some(note))
                } else {
                    (note, None)
                };

                let velocity = self.read_u8();
                let velocity = if (velocity & 0x80) == 0 {
                    velocity
                } else if velocity == 0x80 {
                    // new_00 has a note with velocity 128 which should be read from Register 0
                    // Register 0 is never set in the gesture track thus the read returns 0
                    // This likely isn't intentional which gives us a hard choice
                    // Follow the logic or follow the likely intention?
                    // Due to similar weird register reads, we choose to follow intention
                    127
                } else {
                    // No cave gesture should reach this but just in case
                    panic!(
                        "NoteOn Unhandled Flag: Velocity ({:#X}) from Register/Port",
                        velocity
                    );
                };

                let duration = if voice == 0 {
                    // This byte is used as a factor to get Dsp Time from Seq Time
                    // We shouldn't need to care about this?
                    self.read_u8();

                    // Here is the other potential weird register read
                    // If only one byte is pulled and has the 0x80 flag set, it's a register read
                    // This happens quite a few times but the registers are invalid and would read junk data
                    // Here we treat them as regular durations because they were likely intended to be
                    let count = (flags >> 3) & 0x3;
                    match count {
                        0 => panic!("NoteOn without duration"),
                        1 => NonZeroU32::new(self.read_u8() as _),
                        2 => NonZeroU32::new(self.read_u16() as _),
                        3 => NonZeroU32::new(self.read_u24()),
                        _ => unreachable!(),
                    }
                } else {
                    None
                };

                if voice != 0 && extended_flags & 1 != 0 {
                    panic!("NoteOn Unhandled Flag: Magic Register Read");
                }

                let instruction = if let Some(glide_note) = glide_note {
                    let duration = duration.unwrap().get();
                    BmsInstruction::Portamento {
                        from_note: note,
                        to_note: glide_note,
                        velocity,
                        duration,
                    }
                } else if self.prev_eflags & 0x1 != 0 {
                    let duration = duration.unwrap().get();
                    BmsInstruction::GateOn {
                        note,
                        velocity,
                        duration,
                    }
                } else {
                    BmsInstruction::NoteOn {
                        voice,
                        note,
                        velocity,
                        duration,
                    }
                };

                // If we have a glide note, that's the actual prev_note
                self.prev_note = glide_note.unwrap_or(note);
                self.prev_eflags = extended_flags;

                instruction
            }
            NOTE_OFF_BEGIN..=NOTE_OFF_END => {
                let voice = opcode & 0x7;
                let release = if opcode & 0x08 != 0 {
                    let rel = self.read_u8();
                    if rel > 100 { (rel - 98) * 20 } else { rel }
                } else {
                    0
                };
                self.prev_note = 0;
                self.prev_eflags = 0;
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
                // This should probably never be an issue but better safe than sorry
                match address {
                    _ if address == BmsSubroutine::GestureStart as u32 => BmsInstruction::Call {
                        condition,
                        subroutine: BmsSubroutine::GestureStart,
                    },
                    _ => panic!("Unknown subroutine {:#X} at {:#X}", address, self.pos - 3),
                }
            }
            FINISH => {
                self.prev_note = 0;
                self.prev_eflags = 0;
                BmsInstruction::Finish
            }
            _ => panic!(
                "unknown opcode {:#2X} at address {:#X}",
                opcode,
                self.pos - 1
            ),
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
