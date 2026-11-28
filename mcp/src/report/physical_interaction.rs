use super::{ReportType, SectionCtx, paged, reference_page, rows_to_json};
use crate::{backends::wiki::decode_entities, error::ToolResult, output::report_link};
use serde_json::{Value, json};

pub struct PhysicalInteraction;

#[async_trait::async_trait]
impl ReportType for PhysicalInteraction {
    fn name(&self) -> &'static str {
        "physical_interaction"
    }

    fn default_sections(&self) -> &'static [&'static str] {
        &["identity", "reported_interactions"]
    }

    fn optional_sections(&self) -> &'static [&'static str] {
        &["references"]
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        Ok(match name {
            "identity" => {
                let rows = ctx
                    .query("report/physical_interaction/identity", &[&ctx.id])
                    .await?;
                let row = rows
                    .first()
                    .ok_or_else(|| crate::error::ToolError::NotFound {
                        tool: ctx.tool,
                        message: format!("{} not found", ctx.id),
                    })?;
                let mut genes: Value = row.get("genes");
                for gene in genes.as_array_mut().expect("gene array") {
                    let id = gene["id"].as_str().expect("gene ID");
                    let report_url = report_link(id);
                    let network_url = format!(
                        "https://flybase.org/cgi-bin/get_interactions.html?items={id}&mode=ppi"
                    );
                    gene["report_url"] = json!(report_url);
                    gene["network_url"] = json!(network_url);
                    strip_field(gene, "symbol");
                }
                json!({
                    "id": ctx.id,
                    "interaction_types": row.get::<_, Vec<String>>("interaction_types"),
                    "genes": genes,
                })
            }
            "reported_interactions" => {
                let rows = ctx
                    .query(
                        "report/physical_interaction/reported_interactions",
                        &[&ctx.id, &ctx.fetch],
                    )
                    .await?;
                paged(
                    rows_to_json(&rows, |row| {
                        let mut participants: Value = row.get("participants");
                        for participant in participants.as_array_mut().expect("participant array") {
                            strip_field(participant, "name");
                            for field in ["reported_as", "notes"] {
                                for value in participant[field].as_array_mut().expect("text array")
                                {
                                    if let Some(text) = value.as_str() {
                                        *value = json!(plain_text(text));
                                    }
                                }
                            }
                        }
                        let mut comments: Value = row.get("comments");
                        for comment in comments.as_array_mut().expect("comment array") {
                            strip_field(comment, "text");
                        }
                        json!({
                            "record_id": row.get::<_, String>("uniquename"),
                            "interaction_type": row.get::<_, String>("interaction_type"),
                            "molecule_pair": row.get::<_, Option<String>>("molecule_pair"),
                            "reference_ids": row.get::<_, Vec<String>>("reference_ids"),
                            "assays": row.get::<_, Value>("assays"),
                            "participants": participants,
                            "comments": comments,
                            "cell_lines": row.get::<_, Value>("cell_lines"),
                            "collections": row.get::<_, Value>("collections"),
                            "sources": row.get::<_, Value>("sources"),
                        })
                    }),
                    ctx.limit,
                )
            }
            "references" => {
                return reference_page(ctx, "report/physical_interaction/references").await;
            }
            _ => unreachable!("validated report section"),
        })
    }
}

fn strip_field(item: &mut Value, field: &str) {
    if let Some(text) = item[field].as_str() {
        item[field] = json!(plain_text(text));
    }
}

/// Render the FlyBase inline notation while preserving literal comparison operators in experimental notes.
fn plain_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find(['@', '<', '&']) {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        match rest.as_bytes()[0] {
            b'@' => {
                if let Some(end) = rest[1..].find('@').map(|i| i + 1) {
                    let link = &rest[1..end];
                    // A FlyBase link can include an identifier before the display text.
                    let label = match link.split_once(':') {
                        Some((id, label))
                            if id.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
                                && !id.trim_end_matches(|c: char| c.is_ascii_digit()).eq(id) =>
                        {
                            label
                        }
                        _ => link,
                    };
                    out.push_str(&plain_text(label));
                    rest = &rest[end + 1..];
                    continue;
                }
            }
            b'<' => {
                if let Some((tag, replacement)) = [
                    ("<up>", "["),
                    ("</up>", "]"),
                    ("<down>", "["),
                    ("</down>", "]"),
                    ("<i>", ""),
                    ("</i>", ""),
                    ("<b>", ""),
                    ("</b>", ""),
                ]
                .into_iter()
                .find(|(tag, _)| rest.starts_with(tag))
                {
                    out.push_str(replacement);
                    rest = &rest[tag.len()..];
                    continue;
                }
            }
            b'&' => {
                if let Some(end) = rest.as_bytes().iter().take(12).position(|b| *b == b';') {
                    let entity = &rest[1..end];
                    if !entity.contains('&') {
                        if let Some(greek) = greek_entity(entity) {
                            out.push(greek);
                        } else {
                            out.push_str(&decode_entities(&rest[..=end]));
                        }
                        rest = &rest[end + 1..];
                        continue;
                    }
                }
            }
            _ => unreachable!(),
        }
        out.push(char::from(rest.as_bytes()[0]));
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

fn greek_entity(entity: &str) -> Option<char> {
    let key = entity.strip_suffix("gr")?;
    let upper = key.bytes().all(|b| b.is_ascii_uppercase());
    // ISO Greek entity names in FlyBase's symbol display, in Greek alphabet order.
    let letters = [
        "a", "b", "g", "d", "e", "z", "ee", "th", "i", "k", "l", "m", "n", "x", "o", "p", "r", "s",
        "t", "u", "ph", "kh", "ps", "oh",
    ];
    let index = letters
        .iter()
        .position(|letter| key.eq_ignore_ascii_case(letter))?;
    let alphabet = if upper {
        "ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ"
    } else {
        "αβγδεζηθικλμνξοπρστυφχψω"
    };
    alphabet.chars().nth(index)
}

#[cfg(test)]
mod tests {
    use super::plain_text;

    #[test]
    fn renders_flybase_markup_without_losing_literal_inequalities() {
        assert_eq!(
            plain_text("Only interactions at 21oC with Kd < 5000 nm were curated by FlyBase."),
            "Only interactions at 21oC with Kd < 5000 nm were curated by FlyBase."
        );
        assert_eq!(
            plain_text("score > 50; a single small peptide (<10 residues)."),
            "score > 50; a single small peptide (<10 residues)."
        );
        assert_eq!(
            plain_text(
                "@FBgn0000000:&agr;Tub<up>1</up>@, @mt:CoI@, &THgr; &gt; 2 & unknown &bogus;"
            ),
            "αTub[1], mt:CoI, Θ > 2 & unknown &bogus;"
        );
    }
}
