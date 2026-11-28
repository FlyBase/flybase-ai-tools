use super::{ReportType, SectionCtx, SectionSource, offset_page};
use crate::{error::ToolResult, output::report_link};
use serde_json::{Value, json};

pub struct Image;

#[async_trait::async_trait]
impl ReportType for Image {
    fn name(&self) -> &'static str {
        "image"
    }

    fn default_sections(&self) -> &'static [&'static str] {
        &["identity", "image_data", "annotations", "references"]
    }

    fn optional_sections(&self) -> &'static [&'static str] {
        &["regions"]
    }

    fn resolves_feature_id(&self) -> bool {
        false
    }

    fn offset_sections(&self) -> &'static [&'static str] {
        &["annotations", "regions", "references"]
    }

    async fn section(&self, ctx: &SectionCtx, name: &str) -> ToolResult<Value> {
        let SectionSource::ReleaseImage(image) = &ctx.source else {
            unreachable!("image report must have a release image source");
        };
        let id = &ctx.id;
        Ok(match name {
            "identity" => json!({
                "id": id,
                "name": image.name,
                "species": image.species,
                "created": image.created,
                "updated": image.updated,
            }),
            "image_data" => json!({
                "caption": image.caption,
                "caption_terms": image.caption_terms,
                "image_url": format!("https://flybase.org/images/FBim/{id}.png"),
                "thumbnail_url": format!("https://flybase.org/images/FBim/{id}.thumb.png"),
            }),
            "annotations" => page(ctx, &image.annotations, |a| {
                let mut item = json!(a);
                item["url"] = json!(report_link(&a.term_id));
                item
            }),
            "regions" => page(ctx, &image.regions, |region| json!(region)),
            "references" => page(ctx, &image.references, |r| {
                let mut item = json!(r);
                item["url"] = json!(report_link(&r.id));
                item
            }),
            _ => unreachable!("validated section"),
        })
    }
}

fn page<T>(ctx: &SectionCtx, rows: &[T], item: impl Fn(&T) -> Value) -> Value {
    let page_rows = rows
        .iter()
        .skip(ctx.offset as usize)
        .take(ctx.fetch as usize)
        .map(item)
        .collect();
    offset_page(page_rows, ctx.limit, ctx.offset)
}
