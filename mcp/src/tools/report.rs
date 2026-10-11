use crate::{
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, report_link},
    report::{SectionCtx, for_id, jbrowse_link, section_error},
    server::FlyBaseServer,
    tools::resolve::{normalize_fb_prefix, resolve_ids},
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    /// A FlyBase ID of a supported report type (see the tool description). Secondary and obsolete IDs are resolved first.
    pub id: String,
    /// Sections to return. Omit for the type's default summary set. Pass list_options=true to see valid names.
    pub sections: Option<Vec<String>>,
    /// Max items per list section (default 25, max 100).
    pub limit_per_section: Option<u32>,
    /// If true, return only the section names available for this ID's type.
    pub list_options: Option<bool>,
}

#[tool_router(router = tool_router_report, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "get_report",
        description = "Return the data shown on a FlyBase report page for an ID. Supported: genes, alleles, insertions, constructs, transcripts, polypeptides, stocks, references. Choose sections to keep responses small; use list_options=true to see section names. Use resolve first if you only have a symbol."
    )]
    pub async fn get_report(
        &self,
        Parameters(p): Parameters<ReportParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "get_report";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.report_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    /// Sections of one call run concurrently on the single connection checked out here (see `SectionCtx`).
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
        let limit = clamp_limit(p.limit_per_section);
        let ctx = SectionCtx {
            conn: self.chado.get(T).await?,
            tool: T,
            id: id.clone(),
            limit,
            fetch: limit + 1,
        };
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
        let mut env = Envelope::new(T, &self.release).link("report", report_link(&id));
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
        if st.status == "updated" {
            env = env.field("resolved_from", json!(input));
        }
        Ok(env.finish())
    }
}

#[cfg(test)]
mod tests {
    use crate::report::{PREFIX_LABELS, for_id};
    use crate::server::FlyBaseServer;

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
            let implemented = for_id(&format!("{prefix}000001")).is_some();
            assert_eq!(
                desc.contains(label),
                implemented,
                "description must mention '{label}' iff {prefix} is implemented"
            );
        }
    }
}
