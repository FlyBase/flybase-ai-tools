use crate::backends::cache::ResponseCache;
use crate::{
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, cv_term_link, report_link},
    report::{
        cvterm::{canonical_term_id, parse_term_id},
        for_id, jbrowse_link, section_error,
    },
    server::FlyBaseServer,
    tools::resolve::{normalize_fb_prefix, resolve_ids},
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    /// A FlyBase ID of a supported report type (see the tool description). Secondary and obsolete IDs are resolved first.
    pub id: String,
    /// Sections to return. Omit for the type's default summary set. Pass list_options=true to see valid names.
    pub sections: Option<Vec<String>>,
    /// Max items per list section (default 25, max 100).
    pub limit_per_section: Option<u32>,
    /// Rows to skip when requesting references, image annotations, or image regions alone. Use next_offset to continue.
    pub offset: Option<u32>,
    /// If true, return only the section names available for this ID's type.
    pub list_options: Option<bool>,
}

#[tool_router(router = tool_router_report, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "get_report",
        description = "Return the data shown on a FlyBase report page for an ID. Supported: genes, alleles, insertions, constructs, experimental tools (FBto), transcripts, polypeptides, aberrations, balancers, stocks, references, gene groups, disease models, cell lines, strains, libraries, vocabulary terms (GO, FBbt, FBcv, FBdv, FBsv, DOID, SO, MI, FBbi, CHEBI), clones, sequence features, natural transposons, chemicals, ortholog genes, split system combinations, plasmids, molecular segments, images, physical interactions. Choose sections to keep responses small; use list_options=true to see section names. To retrieve every available reference, request sections=[\"references\"] and repeat with the returned next_offset until null. For images, annotations and regions also support next_offset when requested alone. Other list sections return the first page only, up to limit_per_section items with has_more. For experimental tools, use the FBto ID. For other supported types, use resolve first if you only have a symbol."
    )]
    pub async fn get_report(
        &self,
        Parameters(p): Parameters<ReportParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "get_report";
        let started = std::time::Instant::now();
        let res = async {
            let key = ResponseCache::key(T, &p);
            self.caches
                .response
                .get_or_else(key, async {
                    let _permit = self.limits.light(T).await?;
                    self.report_impl(p).await
                })
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    /// Chado-backed report sections run concurrently on a single pooled connection.
    async fn report_impl(&self, p: ReportParams) -> ToolResult<CallToolResult> {
        const T: &str = "get_report";
        let input = normalize_fb_prefix(p.id.trim()).into_owned();
        let rt = for_id(&input).ok_or_else(|| ToolError::InvalidInput {
            tool: T,
            arg: "id",
            message: format!("'{input}' is not a FlyBase ID prefix this server knows"),
        })?;
        if p.list_options.unwrap_or(false) {
            return Ok(Envelope::new(T, &self.release)
                .summary(format!("{} report sections.", rt.name()))
                .field("type", json!(rt.name()))
                .field("default_sections", json!(rt.default_sections()))
                .field("optional_sections", json!(rt.optional_sections()))
                .finish());
        }
        let valid: Vec<&str> = rt
            .default_sections()
            .iter()
            .chain(rt.optional_sections())
            .copied()
            .collect();
        let wanted: Vec<String> = match &p.sections {
            Some(s) if !s.is_empty() => s.clone(),
            _ => rt
                .default_sections()
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };
        for w in &wanted {
            if !valid.contains(&w.as_str()) {
                return Err(ToolError::InvalidInput {
                    tool: T,
                    arg: "sections",
                    message: format!(
                        "unknown section '{w}' for {}; valid: {}",
                        rt.name(),
                        valid.join(", ")
                    ),
                });
            }
        }
        if p.offset.is_some()
            && !matches!(wanted.as_slice(), [section] if rt.offset_sections().contains(&section.as_str()))
        {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!(
                    "request one of these sections alone when paging: {}",
                    rt.offset_sections().join(", ")
                ),
            });
        }
        let (id, resolved_from) = if rt.resolves_feature_id() {
            // resolve_ids checks out and returns its own connection before ours is taken.
            let st = resolve_ids(&self.chado, T, std::slice::from_ref(&input))
                .await?
                .remove(0);
            let id = match st.status {
                "current" | "updated" => st.current_ids[0].clone(),
                "split" => {
                    return Err(ToolError::InvalidInput {
                        tool: T,
                        arg: "id",
                        message: format!(
                            "{input} was split into {}; pick one",
                            st.current_ids.join(", ")
                        ),
                    });
                }
                "obsolete" => {
                    return Err(ToolError::NotFound {
                        tool: T,
                        message: format!("{input} is obsolete with no replacement"),
                    });
                }
                _ => {
                    return Err(ToolError::NotFound {
                        tool: T,
                        message: format!("{input} is not a FlyBase ID; use resolve or search"),
                    });
                }
            };
            (id, (st.status == "updated").then(|| input.clone()))
        } else {
            (
                canonical_term_id(&input).unwrap_or_else(|| input.clone()),
                None,
            )
        };
        let limit = clamp_limit(p.limit_per_section);
        let ctx = self
            .report_context(T, id.clone(), limit, i64::from(p.offset.unwrap_or(0)))
            .await?;
        let resolved_from = resolved_from.or_else(|| (id != ctx.id).then_some(id));
        let id = &ctx.id;
        let futs = wanted.iter().map(|name| {
            let rt = &rt;
            let ctx = &ctx;
            async move { (name.clone(), rt.section(ctx, name).await) }
        });
        let (results, location) =
            futures::join!(futures::future::join_all(futs), rt.location(&ctx));
        // A section that fails with anything but NotFound becomes an error object so the rest of the report survives
        // (a timed-out optional section must not hide the identity). NotFound means the entity is missing from its
        // table, which is a whole-report answer.
        let mut sections = Map::new();
        let mut section_errors: Vec<String> = Vec::new();
        for (name, res) in results {
            match res {
                Ok(v) => {
                    sections.insert(name, v);
                }
                Err(e @ ToolError::NotFound { .. }) => return Err(e),
                Err(e) => {
                    tracing::warn!(tool = T, id = %id, section = %name, error = %e, "section failed");
                    sections.insert(name.clone(), section_error(&e));
                    section_errors.push(name);
                }
            }
        }
        let link = if parse_term_id(id).is_some() {
            cv_term_link(id)
        } else {
            report_link(id)
        };
        let mut env = Envelope::new(T, &self.release).link("report", link);
        match location {
            Ok(Some((chrom, fmin, fmax))) => {
                env = env.link("jbrowse", jbrowse_link(&chrom, fmin, fmax))
            }
            Ok(None) => {}
            Err(e) => {
                // The jbrowse link is not a section, but its failure is reported the same way so the caller can see it.
                tracing::warn!(tool = T, id = %id, error = %e, "location lookup failed; no jbrowse link");
                sections.insert("jbrowse".into(), section_error(&e));
                section_errors.push("jbrowse".into());
            }
        }
        let ident = sections.get("identity").cloned().unwrap_or(Value::Null);
        let label = match (ident["symbol"].as_str(), ident["name"].as_str()) {
            (Some(s), Some(n)) => format!(" ({s}, {n})"),
            (Some(s), None) => format!(" ({s})"),
            _ => String::new(),
        };
        let summary = if section_errors.is_empty() {
            format!(
                "{} report for {id}{label}: sections {}.",
                rt.name(),
                wanted.join(", ")
            )
        } else {
            format!(
                "{} report for {id}{label}: sections {}. Failed sections: {} (see their error objects).",
                rt.name(),
                wanted.join(", "),
                section_errors.join(", ")
            )
        };
        env = env
            .summary(summary)
            .field("id", json!(id))
            .field("type", json!(rt.name()))
            .field("sections", Value::Object(sections));
        if !section_errors.is_empty() {
            env = env.field("section_errors", json!(section_errors));
        }
        if let Some(from) = resolved_from {
            env = env.field("resolved_from", json!(from));
        }
        Ok(env.finish_with_truncation(|body| {
            for section in rt.offset_sections() {
                if let Some(page) = body["sections"].get_mut(section) {
                    repair_page(page, ctx.offset);
                }
            }
            repair_reported_interactions_page(body);
        }))
    }
}

fn repair_page(page: &mut Value, offset: i64) {
    if let Some(items) = page["items"].as_array() {
        let shown = items.len() as i64;
        if page["count_shown"].as_i64() != Some(shown) {
            page["count_shown"] = json!(shown);
            page["has_more"] = json!(true);
            page["next_offset"] = json!(offset + shown);
        }
    }
}

fn repair_reported_interactions_page(body: &mut Value) {
    if let Some(page) = body.pointer_mut("/sections/reported_interactions")
        && let Some(items) = page["items"].as_array()
        && page["count_shown"].as_u64() != Some(items.len() as u64)
    {
        page["count_shown"] = json!(items.len());
        page["has_more"] = json!(true);
    }
}

#[cfg(test)]
mod tests {
    use crate::report::{PREFIX_LABELS, for_id};
    use crate::server::FlyBaseServer;

    #[test]
    fn byte_limited_reference_page_continues_after_last_returned_row() {
        use serde_json::json;
        let items = vec![json!({"id":"FBrf0000001","citation":"x".repeat(50_000)}); 10];
        let result = crate::output::Envelope::new("get_report", "test")
            .field("sections", json!({"references":{"items":items,"count_shown":10,"has_more":false,"next_offset":null}}))
            .finish_with_truncation(|body| super::repair_page(&mut body["sections"]["references"], 20));
        let body = result.structured_content.unwrap();
        let page = &body["sections"]["references"];
        let shown = page["items"].as_array().unwrap().len();
        assert!(shown > 0 && shown < 10);
        assert_eq!(page["count_shown"], shown);
        assert_eq!(page["has_more"], true);
        assert_eq!(page["next_offset"], 20 + shown);
    }

    /// The description names a type exactly when `for_id` dispatches its prefix (Plan 1 deferred minor).
    #[test]
    fn description_lists_exactly_the_implemented_types() {
        let tools = FlyBaseServer::all_tool_routers().list_all();
        let t = tools
            .iter()
            .find(|t| t.name == "get_report")
            .expect("get_report registered");
        let desc = t.description.as_deref().unwrap_or("");
        for (prefix, label) in PREFIX_LABELS {
            let suffix = if prefix.contains(':') {
                "000001"
            } else {
                "0000001"
            };
            let implemented = for_id(&format!("{prefix}{suffix}")).is_some();
            assert_eq!(
                desc.contains(label),
                implemented,
                "description must mention '{label}' iff {prefix} is implemented"
            );
        }
    }
}
