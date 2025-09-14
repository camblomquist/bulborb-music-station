use std::{fs, mem, ops::Index, path::Path, slice::SliceIndex};

const LIST_SENTINEL: [u8; 3] = [0xFF, 0xFF, 0xFF];
const TIMETABLE_LIST_ADDRESS_LOCATION: usize = 0x14;

pub struct BmsData(Vec<u8>);

#[derive(Debug)]
pub struct RawTimetable(Vec<u16>);

impl BmsData {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Self {
        let data = fs::read(path).unwrap();
        Self(data)
    }

    pub fn get_timetables(&self) -> Vec<RawTimetable> {
        let address = self.get_list_location(TIMETABLE_LIST_ADDRESS_LOCATION);
        let timetable_locations = self.iter_address_list(address as _);

        timetable_locations
            .map(|address| {
                RawTimetable(
                    self[(address as _)..]
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

    pub fn get_list_location(&self, address: usize) -> u16 {
        let slice = &self.0[address..];
        assert!(slice.len() >= mem::size_of::<u16>());
        let address = unsafe { slice.first_chunk().unwrap_unchecked() };
        u16::from_be_bytes(*address)
    }

    pub fn iter_address_list(&self, address: u32) -> impl Iterator<Item = u32> {
        let data = &self.0;
        let address = address as usize;
        let chunks = data[address..].as_chunks().0.iter();
        chunks
            .take_while(|&&x| x != LIST_SENTINEL)
            .map(|&[x, y, z]| u32::from_be_bytes([0, x, y, z]))
    }
}

impl<I> Index<I> for BmsData
where
    I: SliceIndex<[u8]>,
{
    type Output = <I as SliceIndex<[u8]>>::Output;

    fn index(&self, index: I) -> &Self::Output {
        self.0.index(index)
    }
}
