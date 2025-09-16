use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug)]
pub struct RawTrackMap {
    name: String,
    mix_track_counts: [u8; 6],
    skip_tracks: [bool; 16],
    skip_notes: [bool; 8],
}

impl RawTrackMap {
    pub fn from_file<P: AsRef<Path>>(path: P) -> BTreeMap<String, RawTrackMap> {
        let mut trackmaps = BTreeMap::new();

        // File is Shift JIS which isn't valid UTF-8
        // Instead of pulling in encoding lib, just treat as bytes
        let data = std::fs::read(path).unwrap();

        // Hate this entire thing but it doesn't really need to be any nicer to function

        let mut lines = data
            .split_inclusive(|&x| x == '\n' as _)
            .map(|line| unsafe {
                line.split(|&x| x == '#' as _)
                    .next()
                    .unwrap_unchecked()
                    .trim_ascii()
            })
            .filter(|line| !(line.is_empty() || line[0] == '{' as _ || line[0] == '}' as _))
            .map(|line| str::from_utf8(line).unwrap());

        while let Some(line) = lines.next()
            && line != "endoffile"
        {
            let name = line.to_string();

            let mut mixes = lines.by_ref().take(6).map(|line| line.parse().unwrap());
            let mut mix_track_counts = [0; 6];
            mix_track_counts.fill_with(|| mixes.next().unwrap());

            let mut skips = lines
                .by_ref()
                .take(4)
                .flat_map(|line| line.split_ascii_whitespace())
                .map(|x| x == "1");
            let mut skip_tracks = [false; 16];
            skip_tracks.fill_with(|| skips.next().unwrap());

            let mut skips = lines
                .by_ref()
                .take(2)
                .flat_map(|line| line.split_ascii_whitespace())
                .map(|x| x == "1");
            let mut skip_notes = [false; 8];
            skip_notes.fill_with(|| skips.next().unwrap());

            trackmaps.insert(
                name.clone(),
                RawTrackMap {
                    name,
                    mix_track_counts,
                    skip_tracks,
                    skip_notes,
                },
            );
        }
        trackmaps
    }
}
