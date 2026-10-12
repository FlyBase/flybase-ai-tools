//! Release-to-release coordinate conversion over bundled interval maps (spec 5.4). No database access.
use serde::Serialize;
use std::collections::HashMap;
use std::sync::LazyLock;

const DMEL_R5_TO_R6: &str = include_str!("../../data/coordinates/dmel_r5_to_r6.tsv");

/// Blocks reported in a partial or multi-block conversion; more are summarized in a note.
pub const MAX_BLOCKS_REPORTED: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Pair {
    pub species: &'static str,
    pub from_release: u8,
    pub to_release: u8,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub from_start: u64,
    pub from_end: u64,
    pub to_chr: String,
    pub to_start: u64,
    pub to_end: u64,
    pub minus: bool,
}

impl Block {
    fn convert(&self, pos: u64) -> u64 {
        if self.minus {
            self.to_end - (pos - self.from_start)
        } else {
            self.to_start + (pos - self.from_start)
        }
    }
}

pub struct ReleaseMap {
    pub pair: Pair,
    pub source: &'static str,
    chrs: HashMap<String, Vec<Block>>,
}

pub static MAPS: LazyLock<Vec<ReleaseMap>> = LazyLock::new(|| {
    vec![
        ReleaseMap::parse(
            Pair { species: "Dmel", from_release: 5, to_release: 6 },
            "FlyBase Schroeder.Dmel_r5-to-r6_map.2014.6.12 (BSD-3-Clause, see data/coordinates/LICENSE.bulkfile-scripts)",
            DMEL_R5_TO_R6,
        )
        .expect("bundled Dmel r5 to r6 map parses"),
    ]
});

pub fn find(species: &str, from_release: u8, to_release: u8) -> Option<&'static ReleaseMap> {
    MAPS.iter().find(|m| {
        m.pair.species.eq_ignore_ascii_case(species)
            && m.pair.from_release == from_release
            && m.pair.to_release == to_release
    })
}

pub fn pairs() -> Vec<Pair> {
    MAPS.iter().map(|m| m.pair).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Mapped,
    Partial,
    Unmapped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Span {
    pub chromosome: String,
    pub start: u64,
    pub end: u64,
}

impl Span {
    pub fn length(&self) -> u64 {
        self.end - self.start + 1
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MappedBlock {
    pub from: Span,
    pub to: Span,
    pub strand_flipped: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Conversion {
    pub status: Status,
    pub to: Option<Span>,
    /// Bases in the input range.
    pub from_length: u64,
    /// Bases in the converted range; differs from `from_length` when the range spans a region of change.
    pub to_length: Option<u64>,
    pub strand_flipped: bool,
    pub regions_spanned: usize,
    pub blocks: Vec<MappedBlock>,
    pub notes: Vec<String>,
}

impl Conversion {
    fn unmapped(from_length: u64, note: String) -> Self {
        Self {
            status: Status::Unmapped,
            to: None,
            from_length,
            to_length: None,
            strand_flipped: false,
            regions_spanned: 0,
            blocks: Vec::new(),
            notes: vec![note],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub chromosome: String,
    pub start: u64,
    pub end: u64,
    pub is_range: bool,
}

/// Accepts `2L:1000`, `2L:1..1000`, `2L:1-1000`, `2L:1--1000`, `chr2L:1,000..2,000` (any case of `chr`).
pub fn parse_coordinate(input: &str) -> Result<Parsed, String> {
    let s = input.trim();
    if s.is_empty() || s.len() > 100 {
        return Err("expected chromosome:position or chromosome:start..end".into());
    }
    let (chr, rest) = s
        .split_once(':')
        .ok_or("expected chromosome:position or chromosome:start..end")?;
    let chr = chr.trim();
    // Byte-safe prefix strip: `get` returns None on a non-ASCII boundary instead of panicking.
    let chr = match chr.get(..3) {
        Some(p) if p.eq_ignore_ascii_case("chr") && chr.len() > 3 => &chr[3..],
        _ => chr,
    };
    if chr.is_empty() || !chr.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!("'{chr}' is not a scaffold name"));
    }
    let rest: String = rest
        .chars()
        .filter(|c| *c != ',' && !c.is_whitespace())
        .collect();
    let (a, b, is_range): (&str, &str, bool) = if let Some((a, b)) = rest.split_once("..") {
        (a, b, true)
    } else if let Some((a, b)) = rest.split_once("--") {
        (a, b, true)
    } else if let Some((a, b)) = rest.split_once('-') {
        (a, b, true)
    } else {
        (rest.as_str(), rest.as_str(), false)
    };
    let num = |t: &str| -> Result<u64, String> {
        let n: u64 = t.parse().map_err(|_| format!("'{t}' is not a position"))?;
        if n == 0 {
            Err("positions are 1-based".into())
        } else {
            Ok(n)
        }
    };
    let (mut start, mut end) = (num(a)?, num(b)?);
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    Ok(Parsed {
        chromosome: chr.to_string(),
        start,
        end,
        is_range,
    })
}

impl ReleaseMap {
    fn parse(pair: Pair, source: &'static str, text: &str) -> Result<Self, String> {
        let mut chrs: HashMap<String, Vec<Block>> = HashMap::new();
        for (n, line) in text.lines().enumerate() {
            if line.starts_with('#') || line.starts_with("from_chr") || line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() != 7 {
                return Err(format!("line {}: expected 7 columns", n + 1));
            }
            let p = |t: &str| {
                t.parse::<u64>()
                    .map_err(|_| format!("line {}: bad number {t}", n + 1))
            };
            let b = Block {
                from_start: p(f[1])?,
                from_end: p(f[2])?,
                to_chr: f[3].to_string(),
                to_start: p(f[4])?,
                to_end: p(f[5])?,
                minus: f[6] == "-",
            };
            if b.from_start > b.from_end
                || b.to_start > b.to_end
                || b.from_end - b.from_start != b.to_end - b.to_start
            {
                return Err(format!("line {}: inconsistent block", n + 1));
            }
            chrs.entry(f[0].to_string()).or_default().push(b);
        }
        for (chr, blocks) in chrs.iter_mut() {
            blocks.sort_by_key(|b| b.from_start);
            for w in blocks.windows(2) {
                if w[1].from_start <= w[0].from_end {
                    return Err(format!("{chr}: overlapping blocks at {}", w[1].from_start));
                }
            }
        }
        if chrs.is_empty() {
            return Err("empty map".into());
        }
        Ok(Self { pair, source, chrs })
    }

    pub fn chromosomes(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.chrs.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    fn block_for(blocks: &[Block], pos: u64) -> Option<&Block> {
        let i = blocks.partition_point(|b| b.from_start <= pos);
        let b = blocks.get(i.checked_sub(1)?)?;
        (pos <= b.from_end).then_some(b)
    }

    /// Blocks overlapping [start, end] on `chr`, in map order.
    fn overlapping(&self, chr: &str, start: u64, end: u64) -> &[Block] {
        let Some(blocks) = self.chrs.get(chr) else {
            return &[];
        };
        let lo = blocks.partition_point(|b| b.from_end < start);
        let hi = blocks.partition_point(|b| b.from_start <= end);
        &blocks[lo..hi.max(lo)]
    }

    /// Consecutive blocks land in map order on the target (ascending on `+`, descending on `-`). Only then
    /// can two converted ends describe one target range.
    fn collinear(blocks: &[&Block]) -> bool {
        blocks.windows(2).all(|w| {
            if w[0].minus {
                w[1].to_end < w[0].to_start
            } else {
                w[1].to_start > w[0].to_end
            }
        })
    }

    /// Convert one position. None when it lies in a region of change or on an unknown scaffold.
    pub fn position(&self, chr: &str, pos: u64) -> Option<(String, u64, bool)> {
        let b = Self::block_for(self.chrs.get(chr)?, pos)?;
        Some((b.to_chr.clone(), b.convert(pos), b.minus))
    }

    /// Convert a range. A single position is `range(chr, p, p)`.
    pub fn range(&self, chr: &str, start: u64, end: u64) -> Conversion {
        let from_length = end - start + 1;
        if !self.chrs.contains_key(chr) {
            return Conversion::unmapped(
                from_length,
                format!(
                    "{chr} is not a scaffold in the release {} map",
                    self.pair.from_release
                ),
            );
        }
        let overlapping = self.overlapping(chr, start, end);
        if overlapping.is_empty() {
            return Conversion::unmapped(
                from_length,
                "no mapped block covers this range; it lies in a region of change".into(),
            );
        }
        let strand_flipped = overlapping.iter().any(|b| b.minus);
        let regions_spanned = overlapping.len() - 1;
        let mut notes = Vec::new();
        let blocks: Vec<MappedBlock> = overlapping
            .iter()
            .take(MAX_BLOCKS_REPORTED)
            .map(|b| {
                let (fs, fe) = (start.max(b.from_start), end.min(b.from_end));
                let (ts, te) = (b.convert(fs), b.convert(fe));
                MappedBlock {
                    from: Span {
                        chromosome: chr.to_string(),
                        start: fs,
                        end: fe,
                    },
                    to: Span {
                        chromosome: b.to_chr.clone(),
                        start: ts.min(te),
                        end: ts.max(te),
                    },
                    strand_flipped: b.minus,
                }
            })
            .collect();
        if overlapping.len() > MAX_BLOCKS_REPORTED {
            notes.push(format!(
                "{} blocks overlap this range; the first {MAX_BLOCKS_REPORTED} are listed",
                overlapping.len()
            ));
        }
        let partial = |mut notes: Vec<String>, note: String| {
            notes.push(note);
            Conversion {
                status: Status::Partial,
                to: None,
                from_length,
                to_length: None,
                strand_flipped,
                regions_spanned,
                blocks: blocks.clone(),
                notes,
            }
        };
        match (self.position(chr, start), self.position(chr, end)) {
            (Some((sc, sp, sm)), Some((ec, ep, em))) if sc == ec && sm == em => {
                // Blocks that share the ends' scaffold and strand must be in order; interior blocks that land
                // elsewhere (FlyBase's converter ignores them too) are reported, not fatal.
                let same: Vec<&Block> = overlapping
                    .iter()
                    .filter(|b| b.to_chr == sc && b.minus == sm)
                    .collect();
                if !Self::collinear(&same) {
                    return partial(
                        notes,
                        "the overlapping blocks are not collinear in the target release; the range is returned block by block".into(),
                    );
                }
                let strays = overlapping.len() - same.len();
                let strand_flipped = sm;
                let to = Span {
                    chromosome: sc,
                    start: sp.min(ep),
                    end: sp.max(ep),
                };
                let to_length = to.length();
                if regions_spanned > 0 {
                    notes.push(format!("spans {regions_spanned} region(s) of change"));
                }
                if strays > 0 {
                    notes.push(format!(
                        "{strays} interior block(s) map to another scaffold or strand; see blocks"
                    ));
                }
                if to_length != from_length {
                    notes.push(format!(
                        "length changes from {from_length} to {to_length} bases across the region(s) of change"
                    ));
                }
                if to.chromosome != chr {
                    notes.push("different scaffold".into());
                }
                if strand_flipped {
                    notes.push(
                        "inversion: the block is reverse complemented in the target release".into(),
                    );
                }
                Conversion {
                    status: Status::Mapped,
                    to: Some(to),
                    from_length,
                    to_length: Some(to_length),
                    strand_flipped,
                    regions_spanned,
                    blocks: if regions_spanned > 0 {
                        blocks
                    } else {
                        Vec::new()
                    },
                    notes,
                }
            }
            (Some((sc, sp, _)), Some((ec, ep, _))) => partial(
                notes,
                format!(
                    "start maps to {sc}:{sp} and end to {ec}:{ep}; the range is returned block by block"
                ),
            ),
            (Some((sc, sp, _)), None) => partial(
                notes,
                format!("end is inside a region of change; start maps to {sc}:{sp}"),
            ),
            (None, Some((ec, ep, _))) => partial(
                notes,
                format!("start is inside a region of change; end maps to {ec}:{ep}"),
            ),
            (None, None) => partial(
                notes,
                format!(
                    "both ends are inside regions of change; {} mapped block(s) overlap the range",
                    overlapping.len()
                ),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r5() -> &'static ReleaseMap {
        find("Dmel", 5, 6).unwrap()
    }

    fn convert(input: &str) -> (Status, Option<Span>, bool) {
        let p = parse_coordinate(input).unwrap();
        let c = r5().range(&p.chromosome, p.start, p.end);
        (c.status, c.to, c.strand_flipped)
    }

    fn span(chr: &str, s: u64, e: u64) -> Option<Span> {
        Some(Span {
            chromosome: chr.into(),
            start: s,
            end: e,
        })
    }

    #[test]
    fn map_loads_with_expected_shape() {
        let m = r5();
        assert_eq!(
            m.chromosomes(),
            vec![
                "2L", "2LHet", "2R", "2RHet", "3L", "3LHet", "3R", "3RHet", "4", "U", "X", "XHet",
                "YHet"
            ]
        );
        assert_eq!(m.chrs.values().map(Vec::len).sum::<usize>(), 6621);
        assert_eq!(
            pairs(),
            vec![Pair {
                species: "Dmel",
                from_release: 5,
                to_release: 6
            }]
        );
        assert!(find("Dmel", 4, 5).is_none());
        assert!(find("dmel", 5, 6).is_some());
    }

    /// The nine cases from FlyBase's own converter test suite (bulkfile-scripts, 01_dmel_r5_to_r6_converter.t).
    #[test]
    fn flybase_converter_test_cases() {
        assert_eq!(
            convert("3RHet:1936890"),
            (Status::Mapped, span("3R", 3585370, 3585370), true)
        );
        assert_eq!(
            convert("3RHet:1964215"),
            (Status::Mapped, span("3R", 3523947, 3523947), true)
        );
        assert_eq!(
            convert("3RHet:1964215..1936890"),
            (Status::Mapped, span("3R", 3523947, 3585370), true)
        );
        assert_eq!(
            convert("2R:1..10000"),
            (Status::Mapped, span("2R", 4112496, 4122495), false)
        );
        assert_eq!(
            convert("ChrX:330..19020"),
            (Status::Mapped, span("X", 103943, 122633), false)
        );
        assert_eq!(
            convert("4:43943..50320"),
            (Status::Mapped, span("4", 23317, 29694), false)
        );
        assert_eq!(
            convert("3R:3,020,320"),
            (Status::Mapped, span("3R", 7194598, 7194598), false)
        );
        assert_eq!(
            convert("chr4:123020"),
            (Status::Mapped, span("4", 102394, 102394), false)
        );
        assert_eq!(
            convert("3RHet:1894569..1895349"),
            (Status::Mapped, span("3R", 3626910, 3627690), true)
        );
        let p = parse_coordinate("3RHet:1964215..1936890").unwrap();
        let c = r5().range(&p.chromosome, p.start, p.end);
        assert_eq!(c.regions_spanned, 5);
        assert_eq!(c.from_length, 27326);
        assert!(c.notes.iter().any(|n| n.contains("different scaffold")));
    }

    /// FlyBase anchor insertions (Anchors.Rel5.txt and Anchors.Rel6.tsv).
    #[test]
    fn anchor_insertions() {
        assert_eq!(
            convert("X:11749080"),
            (Status::Mapped, span("X", 11855047, 11855047), false)
        );
        assert_eq!(
            convert("X:10219961"),
            (Status::Mapped, span("X", 10325928, 10325928), false)
        );
        assert_eq!(
            convert("2R:8213881"),
            (Status::Mapped, span("2R", 12326376, 12326376), false)
        );
        assert_eq!(
            convert("2L:16310978"),
            (Status::Mapped, span("2L", 16310978, 16310978), false)
        );
    }

    /// Review focus 2.
    #[test]
    fn inverted_block_flips() {
        assert_eq!(
            convert("2LHet:236881"),
            (Status::Mapped, span("Y", 1865892, 1865892), true)
        );
        let (st, to, flipped) = convert("2LHet:236881..236900");
        assert_eq!(st, Status::Mapped);
        assert!(flipped);
        let to = to.unwrap();
        assert!(to.start <= to.end);
        assert_eq!((to.start, to.end), (1865873, 1865892));
        assert_eq!(
            convert("U:100"),
            (Status::Mapped, span("2R", 422984, 422984), false)
        );
    }

    /// Review focus 1: the 2L gaps are 21485539..21485639 and 22420242..22420342.
    #[test]
    fn start_in_gap_is_partial() {
        let p = parse_coordinate("2L:22420300..22420400").unwrap();
        let c = r5().range(&p.chromosome, p.start, p.end);
        assert_eq!(c.status, Status::Partial);
        assert!(c.to.is_none());
        assert_eq!((c.from_length, c.to_length), (101, None));
        assert_eq!(c.blocks.len(), 1);
        assert_eq!(
            c.blocks[0].from,
            Span {
                chromosome: "2L".into(),
                start: 22420343,
                end: 22420400
            }
        );
        assert_eq!(
            c.blocks[0].to,
            Span {
                chromosome: "2L".into(),
                start: 22527611,
                end: 22527668
            }
        );
        assert!(
            c.notes
                .iter()
                .any(|n| n.starts_with("start is inside a region of change"))
        );
        assert_eq!(convert("2L:21485600").0, Status::Unmapped);
        assert_eq!(
            convert("2L:21485500..21485700"),
            (Status::Mapped, span("2L", 21485500, 21485700), false)
        );
        let p = parse_coordinate("2L:21485500..21485700").unwrap();
        assert_eq!(r5().range(&p.chromosome, p.start, p.end).regions_spanned, 1);
    }

    /// A range whose ends both map but which straddles a 107 kb insertion in release 6: still `mapped`,
    /// with the length change stated, because the two blocks are collinear.
    #[test]
    fn gap_spanning_range_reports_lengths() {
        let p = parse_coordinate("2L:22420200..22420400").unwrap();
        let c = r5().range(&p.chromosome, p.start, p.end);
        assert_eq!(c.status, Status::Mapped);
        assert_eq!(c.to, span("2L", 22420200, 22527668));
        assert_eq!(c.from_length, 201);
        assert_eq!(c.to_length, Some(107469));
        assert_eq!(c.regions_spanned, 1);
        assert_eq!(c.blocks.len(), 2);
        assert!(
            c.notes
                .iter()
                .any(|n| n.contains("length changes from 201 to 107469"))
        );
    }

    /// Ends on different scaffolds or strands, or same-target blocks out of order, are never collapsed into
    /// one range; an interior block that lands elsewhere is noted (FlyBase's converter ignores it).
    #[test]
    fn non_collinear_blocks_are_partial() {
        // 2R:2560..2581 maps to 3R between two 2R blocks: the ends agree, so mapped with a stray-block note.
        let p = parse_coordinate("2R:2400..3100").unwrap();
        let c = r5().range(&p.chromosome, p.start, p.end);
        assert_eq!(c.status, Status::Mapped);
        assert_eq!(c.blocks.len(), 3);
        assert!(c.notes.iter().any(|n| n.contains("1 interior block(s)")));
        assert!(c.blocks.iter().any(|b| b.to.chromosome == "3R"));
        // An end on the stray block itself: start on 3R, end on 2R, partial.
        let p = parse_coordinate("2R:2570..3100").unwrap();
        assert_eq!(
            r5().range(&p.chromosome, p.start, p.end).status,
            Status::Partial
        );
        // 3RHet:1955714.. and 3RHet:1955399.. are both minus blocks on 3R in descending order: collinear, mapped.
        assert_eq!(convert("3RHet:1955700..1955720").0, Status::Mapped);
        // Same scaffold and strand at both ends but a reordered block between: synthetic map.
        let m = ReleaseMap::parse(
            Pair { species: "T", from_release: 1, to_release: 2 },
            "test",
            "A\t1\t100\tA\t1000\t1099\t+\nA\t101\t200\tA\t500\t599\t+\nA\t201\t300\tA\t1100\t1199\t+\n",
        )
        .unwrap();
        let c = m.range("A", 50, 250);
        assert_eq!(c.status, Status::Partial);
        assert!(c.notes.iter().any(|n| n.contains("not collinear")));
        assert_eq!(m.range("A", 50, 150).status, Status::Partial);
    }

    #[test]
    fn unknown_scaffold_and_parse_errors() {
        assert_eq!(convert("Y:100").0, Status::Unmapped);
        assert_eq!(convert("2L:999999999").0, Status::Unmapped);
        assert!(parse_coordinate("2L").is_err());
        assert!(parse_coordinate("2L:0").is_err());
        assert!(parse_coordinate("2L:abc").is_err());
        assert!(parse_coordinate(":100").is_err());
        assert!(parse_coordinate("2L:1..").is_err());
        assert!(
            parse_coordinate("é:100").is_err(),
            "non-ASCII scaffold is an error, not a panic"
        );
        assert!(parse_coordinate("chré:100").is_err());
        assert!(
            parse_coordinate("ch:100").is_ok(),
            "short names are not mistaken for a chr prefix"
        );
        assert_eq!(
            parse_coordinate("chr2L:2,000-1,000").unwrap(),
            Parsed {
                chromosome: "2L".into(),
                start: 1000,
                end: 2000,
                is_range: true
            }
        );
        assert_eq!(
            parse_coordinate(" X:5 ").unwrap(),
            Parsed {
                chromosome: "X".into(),
                start: 5,
                end: 5,
                is_range: false
            }
        );
    }
}
