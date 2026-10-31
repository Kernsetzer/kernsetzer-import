//! Images embedded in a manuscript: extracted once per file, identified by
//! content (not extension). Format detection and header metadata live in
//! `setzer_model::asset`, shared with the project file.

use super::package::{MAX_MEDIA_BYTES, Package, Rel};
use crate::{ImportWarning, WarningDetail, WarningKind, assets};
use setzer_model::asset::{Asset, ImageFormat};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Media {
    pub assets: Vec<Asset>,
    /// Zip path → asset id (`None` if the image could not be used).
    by_path: HashMap<String, Option<String>>,
}

impl Media {
    /// The asset id for an image relationship, extracting it on first use.
    pub fn image(
        &mut self,
        pkg: &mut Package,
        rel: Option<&Rel>,
        linked: bool,
        para: usize,
        warnings: &mut Vec<ImportWarning>,
    ) -> Option<String> {
        let name = rel.map(|r| r.target.clone());
        let mut warn = |kind, msg: String| {
            warnings.push(ImportWarning {
                kind,
                location: para,
                section: None,
                message: format!("Paragraph {para}: {msg}"),
                detail: Some(WarningDetail::Image { name: name.clone() }),
            })
        };
        let Some(rel) = rel else {
            warn(
                WarningKind::ImageMissing,
                "an image is missing from the file.".into(),
            );
            return None;
        };
        if linked || rel.external {
            warn(
                WarningKind::ImageMissing,
                format!(
                    "an image is linked to an outside file ({}) instead of being part of the document. Insert the picture itself.",
                    rel.target
                ),
            );
            return None;
        }
        if let Some(id) = self.by_path.get(&rel.target) {
            return id.clone();
        }
        let id = match pkg.read_bytes(&rel.target, MAX_MEDIA_BYTES) {
            None => {
                warn(
                    WarningKind::ImageMissing,
                    "an image is missing from the file.".into(),
                );
                None
            }
            Some(Err(e)) => {
                warn(
                    WarningKind::ImageMissing,
                    format!("an image could not be read ({e})."),
                );
                None
            }
            Some(Ok(bytes)) => match ImageFormat::sniff(&bytes) {
                ImageFormat::Supported { ext, .. } => {
                    let id = assets::unique_id(&rel.target, ext, &self.assets);
                    self.assets.push(Asset::new(id.clone(), bytes));
                    Some(id)
                }
                ImageFormat::Unsupported(name) => {
                    warn(
                        WarningKind::ImageUnsupported,
                        format!(
                            "an image is in {name} format, which cannot be used in a book. Replace it with a PNG or JPEG."
                        ),
                    );
                    None
                }
            },
        };
        self.by_path.insert(rel.target.clone(), id.clone());
        id
    }
}
