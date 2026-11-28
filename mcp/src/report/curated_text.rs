/// Chado text uses FlyBase's Greek entity names. The website release displays their Unicode forms.
pub(super) fn curated_text(mut text: String) -> String {
    if text.contains('&') {
        for (entity, symbol) in [
            ("&Agr;", "Α"),
            ("&Bgr;", "Β"),
            ("&Ggr;", "Γ"),
            ("&Dgr;", "Δ"),
            ("&Egr;", "Ε"),
            ("&Zgr;", "Ζ"),
            ("&EEgr;", "Η"),
            ("&THgr;", "Θ"),
            ("&Igr;", "Ι"),
            ("&Kgr;", "Κ"),
            ("&Lgr;", "Λ"),
            ("&Mgr;", "Μ"),
            ("&Ngr;", "Ν"),
            ("&Xgr;", "Ξ"),
            ("&Ogr;", "Ο"),
            ("&Pgr;", "Π"),
            ("&Rgr;", "Ρ"),
            ("&Sgr;", "Σ"),
            ("&Tgr;", "Τ"),
            ("&Ugr;", "Υ"),
            ("&PHgr;", "Φ"),
            ("&KHgr;", "Χ"),
            ("&PSgr;", "Ψ"),
            ("&OHgr;", "Ω"),
            ("&agr;", "α"),
            ("&bgr;", "β"),
            ("&ggr;", "γ"),
            ("&dgr;", "δ"),
            ("&egr;", "ε"),
            ("&zgr;", "ζ"),
            ("&eegr;", "η"),
            ("&thgr;", "θ"),
            ("&igr;", "ι"),
            ("&kgr;", "κ"),
            ("&lgr;", "λ"),
            ("&mgr;", "μ"),
            ("&ngr;", "ν"),
            ("&xgr;", "ξ"),
            ("&ogr;", "ο"),
            ("&pgr;", "π"),
            ("&rgr;", "ρ"),
            ("&sgr;", "σ"),
            ("&tgr;", "τ"),
            ("&ugr;", "υ"),
            ("&phgr;", "φ"),
            ("&khgr;", "χ"),
            ("&psgr;", "ψ"),
            ("&ohgr;", "ω"),
        ] {
            if text.contains(entity) {
                text = text.replace(entity, symbol);
            }
        }
    }
    // A paired @ marks a linked symbol, which may itself contain colons.
    let mut rest = text.as_str();
    let mut result = String::with_capacity(rest.len());
    while let Some((prefix, marked)) = rest.split_once('@') {
        let Some((symbol, suffix)) = marked.split_once('@') else {
            break;
        };
        result.push_str(prefix);
        result.push_str(symbol);
        rest = suffix;
    }
    result.push_str(rest);
    result
        .replace("<up>", "[")
        .replace("</up>", "]")
        .replace("<down>", "[")
        .replace("</down>", "]")
}

#[cfg(test)]
mod tests {
    use super::curated_text;

    #[test]
    fn preserves_comparisons_and_colons_in_linked_symbols() {
        assert_eq!(
            curated_text("@snRNA:U6:96Ac@, @ry<up>+</up>@ < 5 &mgr;m".into()),
            "snRNA:U6:96Ac, ry[+] < 5 μm",
        );
        assert_eq!(curated_text("incomplete @link".into()), "incomplete @link");
    }
}
