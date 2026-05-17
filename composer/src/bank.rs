use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Index;
use std::path::Path;
use std::{array, iter};

use byteorder::{BigEndian, ReadBytesExt};

#[derive(Debug)]
pub struct Bank {
    pub id: u32,
    pub instruments: BTreeMap<usize, Instrument>,
    pub percussion_sets: BTreeMap<usize, PercussionSet>,
}

#[derive(Debug)]
pub struct Instrument {
    pub pitch: f32,
    pub volume: f32,
    pub oscillators: Vec<Oscillator>,
    pub rand_effects: Vec<RandEffect>,
    pub sense_effects: Vec<SenseEffect>,
    pub regions: Vec<KeyRegion>,
}

#[derive(Debug)]

pub struct Oscillator {
    pub target: Target,
    pub rate: f32,
    pub attack: Vec<EnvelopePoint>,
    pub release: Vec<EnvelopePoint>,
    pub width: f32,
    pub vertex: f32,
}
#[derive(Debug)]
pub struct KeyRegion {
    pub base_key: u8,
    pub regions: Vec<VelocityRegion>,
}
#[derive(Debug)]

pub struct VelocityRegion {
    pub velocity: u8,
    pub wave_id: u32,
    pub volume: f32,
    pub pitch: f32,
}

#[derive(Debug)]
pub struct EnvelopePoint {
    pub mode: EnvelopeMode,
    pub time: f32,
    pub value: f32,
}

#[derive(Debug)]
pub struct PercussionSet {
    pub percussions: BTreeMap<usize, Percussion>,
}

#[derive(Debug)]
pub struct Percussion {
    pub pitch: f32,
    pub volume: f32,
    pub effects: Vec<RandEffect>,
    pub regions: Vec<VelocityRegion>,
    pub panning: f32,
    pub release: u16,
}

#[derive(Debug)]
pub struct RandEffect {
    pub target: Target,
    pub offset: f32,
    pub scale: f32,
}

#[derive(Debug)]
pub struct SenseEffect {
    pub target: Target,
    pub register: SenseRegister,
    pub key: u32,
    pub floor: f32,
    pub ceiling: f32,
}

#[derive(Debug)]
pub enum Target {
    Volume = 0,
    Pitch,
    Pan,
    FxMix,
    Dolby,
}

#[derive(Debug)]
pub enum EnvelopeMode {
    Linear = 0,
    Square,
    SqRoot,
    SampleCell,
    Loop,
    Hold,
    Stop,
}

#[derive(Debug)]
pub enum SenseRegister {
    Velocity = 1,
    Note = 2,
}

impl Bank {
    pub fn load<P: AsRef<Path>>(path: P) -> Self {
        let mut file = File::open(path).unwrap();

        let mut magic = [0; 4];
        file.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"IBNK");

        let _size = file.read_u32::<BigEndian>().unwrap();
        let id = file.read_u32::<BigEndian>().unwrap();

        file.seek(SeekFrom::Start(0x20)).unwrap();
        file.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"BANK");

        let list: [u64; 240] = array::from_fn(|_| file.read_u32::<BigEndian>().unwrap() as _);

        let instruments = list[..128]
            .into_iter()
            .enumerate()
            .filter_map(|(i, &n)| {
                if n != 0 {
                    Some((i, Instrument::load(ReadScope::with_offset(&mut file, n))))
                } else {
                    None
                }
            })
            .collect();

        let percussion_sets = list[228..]
            .into_iter()
            .enumerate()
            .filter(|(_, n)| **n != 0)
            .filter_map(|(i, &n)| {
                if n != 0 {
                    Some((i, PercussionSet::load(ReadScope::with_offset(&mut file, n))))
                } else {
                    None
                }
            })
            .collect();

        Self {
            id,
            instruments,
            percussion_sets,
        }
    }
}

impl Instrument {
    fn load(mut reader: impl Read + Seek) -> Self {
        let mut magic = [0; 4];
        reader.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"INST");

        // Padding
        reader.seek_relative(4).unwrap();

        let volume = reader.read_f32::<BigEndian>().unwrap();
        let pitch = reader.read_f32::<BigEndian>().unwrap();

        let mut oscillators = [0; 2];
        reader.read_u32_into::<BigEndian>(&mut oscillators).unwrap();

        let mut rand_effects = [0; 2];
        reader
            .read_u32_into::<BigEndian>(&mut rand_effects)
            .unwrap();

        let mut sense_effects = [0; 2];
        reader
            .read_u32_into::<BigEndian>(&mut sense_effects)
            .unwrap();

        let count = reader.read_u32::<BigEndian>().unwrap() as _;
        let regions = iter::from_fn(|| {
            let i = reader.read_u32::<BigEndian>().unwrap() as _;
            Some(KeyRegion::load(ReadScope::with_offset(&mut reader, i)))
        })
        .take(count)
        .collect();

        let oscillators = oscillators
            .into_iter()
            .filter(|i| *i != 0)
            .map(|i| Oscillator::load(ReadScope::with_offset(&mut reader, i as _)))
            .collect();
        let rand_effects = rand_effects
            .into_iter()
            .filter(|i| *i != 0)
            .map(|i| RandEffect::load(ReadScope::with_offset(&mut reader, i as _)))
            .collect();
        let sense_effects = sense_effects
            .into_iter()
            .filter(|i| *i != 0)
            .map(|i| SenseEffect::load(ReadScope::with_offset(&mut reader, i as _)))
            .collect();

        Self {
            pitch,
            volume,
            oscillators,
            rand_effects,
            sense_effects,
            regions,
        }
    }
}

impl Oscillator {
    fn load(mut reader: impl Read + Seek) -> Self {
        let target = reader.read_u8().unwrap().try_into().unwrap();
        reader.seek_relative(3).unwrap();
        let rate = reader.read_f32::<BigEndian>().unwrap();
        let attack = reader.read_u32::<BigEndian>().unwrap() as _;
        let release = reader.read_u32::<BigEndian>().unwrap() as _;
        let width = reader.read_f32::<BigEndian>().unwrap();
        let vertex = reader.read_f32::<BigEndian>().unwrap();

        let attack = EnvelopePoint::load_list(ReadScope::with_offset(&mut reader, attack));
        let release = EnvelopePoint::load_list(ReadScope::with_offset(&mut reader, release));

        Self {
            target,
            rate,
            attack,
            release,
            width,
            vertex,
        }
    }
}

impl EnvelopePoint {
    fn load_list(mut reader: impl Read + Seek) -> Vec<Self> {
        iter::from_fn(|| {
            let next = reader.read_u16::<BigEndian>().unwrap();
            if next <= 0xA {
                let next1 = reader.read_i16::<BigEndian>().unwrap();
                let next2 = reader.read_i16::<BigEndian>().unwrap();
                Some((next, next1, next2))
            } else {
                None
            }
        })
        .map(|(mode, time, value)| Self {
            mode: mode.try_into().unwrap(),
            time: time as f32 / 600.0,
            value: value as f32 / 32768.0,
        })
        .collect()
    }
}

impl PercussionSet {
    fn load(mut reader: impl Read + Seek) -> Self {
        let mut magic = [0; 4];
        reader.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"PER2");

        reader.seek_relative(0x84).unwrap();

        let mut pmaps = [0; 128];
        reader.read_u32_into::<BigEndian>(&mut pmaps).unwrap();

        let mut pannings = [0; 128];
        reader.read_i8_into(&mut pannings).unwrap();

        let mut releases = [0; 128];
        reader.read_u16_into::<BigEndian>(&mut releases).unwrap();

        let percussions = (0..128)
            .filter_map(|i| {
                let offset = pmaps[i] as _;
                if offset != 0 {
                    let mut percussion =
                        Percussion::load(ReadScope::with_offset(&mut reader, offset));
                    percussion.panning = pannings[i] as f32 / 127.0;
                    percussion.release = releases[i];
                    Some((i, percussion))
                } else {
                    None
                }
            })
            .collect();

        Self { percussions }
    }
}

impl Percussion {
    fn load(mut reader: impl Read + Seek) -> Self {
        let pitch = reader.read_f32::<BigEndian>().unwrap();
        let volume = reader.read_f32::<BigEndian>().unwrap();

        let mut effects = [0; 2];
        reader.read_u32_into::<BigEndian>(&mut effects).unwrap();

        let count = reader.read_u32::<BigEndian>().unwrap() as _;
        let regions = iter::from_fn(|| {
            let i = reader.read_u32::<BigEndian>().unwrap() as _;
            Some(VelocityRegion::load(ReadScope::with_offset(&mut reader, i)))
        })
        .take(count)
        .collect();

        let effects = effects
            .into_iter()
            .filter(|i| *i != 0)
            .map(|i| RandEffect::load(ReadScope::with_offset(&mut reader, i as _)))
            .collect();

        Self {
            pitch,
            volume,
            effects,
            regions,
            panning: 0.0,
            release: 0,
        }
    }
}

impl KeyRegion {
    fn load(mut reader: impl Read + Seek) -> Self {
        let base_key = reader.read_u8().unwrap();
        reader.seek_relative(3).unwrap();

        let count = reader.read_u32::<BigEndian>().unwrap() as _;
        let regions = iter::from_fn(|| {
            let i = reader.read_u32::<BigEndian>().unwrap() as _;
            Some(VelocityRegion::load(ReadScope::with_offset(&mut reader, i)))
        })
        .take(count)
        .collect();

        Self { base_key, regions }
    }
}

impl VelocityRegion {
    fn load(mut reader: impl Read + Seek) -> Self {
        let velocity = reader.read_u8().unwrap();
        reader.seek_relative(3).unwrap();
        let wave_id = reader.read_u32::<BigEndian>().unwrap() & 0xFFFF;
        let volume = reader.read_f32::<BigEndian>().unwrap();
        let pitch = reader.read_f32::<BigEndian>().unwrap();

        Self {
            velocity,
            wave_id,
            volume,
            pitch,
        }
    }
}

impl RandEffect {
    fn load(mut reader: impl Read + Seek) -> Self {
        let target = reader.read_u8().unwrap().try_into().unwrap();
        reader.seek_relative(3).unwrap();
        let offset = reader.read_f32::<BigEndian>().unwrap();
        let scale = reader.read_f32::<BigEndian>().unwrap();

        Self {
            target,
            offset,
            scale,
        }
    }
}

impl SenseEffect {
    fn load(mut reader: impl Read + Seek) -> Self {
        let target = reader.read_u8().unwrap().try_into().unwrap();
        let register = reader.read_u8().unwrap().try_into().unwrap();
        let key = reader.read_u8().unwrap() as _;
        reader.seek_relative(1).unwrap();
        let floor = reader.read_f32::<BigEndian>().unwrap();
        let ceiling = reader.read_f32::<BigEndian>().unwrap();

        Self {
            target,
            register,
            key,
            floor,
            ceiling,
        }
    }
}

impl TryFrom<u8> for Target {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Volume),
            1 => Ok(Self::Pitch),
            2 => Ok(Self::Pan),
            3 => Ok(Self::FxMix),
            4 => Ok(Self::Dolby),
            _ => Err(()),
        }
    }
}

impl TryFrom<u16> for EnvelopeMode {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Linear),
            1 => Ok(Self::Square),
            2 => Ok(Self::SqRoot),
            3 => Ok(Self::SampleCell),
            4 => Ok(Self::Loop),
            5 => Ok(Self::Hold),
            6 => Ok(Self::Stop),
            _ => Err(()),
        }
    }
}

impl TryFrom<u8> for SenseRegister {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Velocity),
            2 => Ok(Self::Note),
            _ => Err(()),
        }
    }
}

struct ReadScope<T: Seek> {
    inner: T,
    pos: u64,
}

impl<T: Seek> ReadScope<T> {
    fn with_offset(mut reader: T, offset: u64) -> Self {
        let pos = reader.stream_position().unwrap();
        reader.seek(SeekFrom::Start(offset)).unwrap();
        Self { inner: reader, pos }
    }
}

impl<T: Read + Seek> Read for ReadScope<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<T: Seek> Seek for ReadScope<T> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl<T: Seek> Drop for ReadScope<T> {
    fn drop(&mut self) {
        let _ = self.inner.seek(SeekFrom::Start(self.pos));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load() {
        let bank = Bank::load("../data/Audio_Modding/include/5.bnk");
        println!("ID: {}", bank.id);
        println!("Instrument Count: {}", bank.instruments.len());
        println!("Percussion Count: {}", bank.percussion_sets.len());

        println!("\nInstrument 104:\n{:#?}", bank.instruments[&104]);
        println!("\nPercussion 11:\n{:#?}", bank.percussion_sets[&11]);
    }
}
