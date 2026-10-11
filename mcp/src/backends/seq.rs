/// Reverse complement of a DNA string; keeps case; unknown letters map to N.
pub fn reverse_complement(s: &str) -> String {
    s.chars()
        .rev()
        .map(|c| match c {
            'A' => 'T',
            'T' => 'A',
            'C' => 'G',
            'G' => 'C',
            'a' => 't',
            't' => 'a',
            'c' => 'g',
            'g' => 'c',
            'N' => 'N',
            'n' => 'n',
            _ => 'N',
        })
        .collect()
}

/// Join exon-like parts (chado fmin 0-based, fmax exclusive) cut from `genomic`, which starts at `genomic_fmin`.
/// Parts are sorted by fmin; the result is reverse complemented for strand -1.
pub fn splice(parts: &[(i64, i64)], genomic: &str, genomic_fmin: i64, strand: i8) -> String {
    let mut parts: Vec<(i64, i64)> = parts.to_vec();
    parts.sort_unstable();
    let mut out = String::new();
    for (fmin, fmax) in parts {
        let a = (fmin - genomic_fmin).max(0) as usize;
        let b = ((fmax - genomic_fmin).max(0) as usize).min(genomic.len());
        if a < b {
            out.push_str(&genomic[a..b]);
        }
    }
    if strand < 0 {
        reverse_complement(&out)
    } else {
        out
    }
}

pub fn fasta(header: &str, seq: &str) -> String {
    let mut s = format!(">{header}\n");
    for chunk in seq.as_bytes().chunks(60) {
        s.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        s.push('\n');
    }
    s
}

/// Convert a 1-based inclusive user range to chado (fmin 0-based, fmax exclusive).
pub fn to_chado(start: u64, end: u64) -> (i64, i64) {
    (start as i64 - 1, end as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revcomp() {
        assert_eq!(reverse_complement("ATGCn"), "nGCAT");
    }
    #[test]
    fn splice_plus_and_minus() {
        let g = "AAACCCGGGTTT";
        assert_eq!(splice(&[(100, 103), (106, 109)], g, 100, 1), "AAAGGG");
        assert_eq!(splice(&[(106, 109), (100, 103)], g, 100, -1), "CCCTTT");
    }
    #[test]
    fn fasta_wraps_at_60() {
        let s = fasta("x", &"A".repeat(130));
        assert_eq!(s.lines().count(), 4);
        assert_eq!(s.lines().nth(1).unwrap().len(), 60);
    }
    #[test]
    fn coords() {
        assert_eq!(to_chado(1, 10), (0, 10));
    }
}
