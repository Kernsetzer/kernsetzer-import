//! Asset ids, shared by both importers.

use setzer_model::asset::Asset;

/// Largest image we import.
pub const MAX_IMAGE_BYTES: u64 = 100 * 1024 * 1024;

/// File name for an image's asset: the original base name (`/` or `\`
/// paths), sanitized, with the extension that matches the content, made
/// unique among `assets`.
pub fn unique_id(path: &str, ext: &str, assets: &[Asset]) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or("image");
    let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
    let clean: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = if clean.is_empty() {
        "image".into()
    } else {
        clean
    };
    let mut id = format!("{clean}.{ext}");
    let mut n = 2;
    while assets.iter().any(|a| a.id == id) {
        id = format!("{clean}-{n}.{ext}");
        n += 1;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_sanitized_and_unique() {
        let taken = vec![Asset::new("map.png", Vec::new())];
        assert_eq!(unique_id("img/Karte 1.jpeg", "jpg", &[]), "Karte_1.jpg");
        assert_eq!(unique_id(r"bilder\map.PNG", "png", &taken), "map-2.png");
        assert_eq!(unique_id(".png", "png", &[]), "image.png");
        assert!(Asset::is_valid_id(&unique_id("ü/ö ä", "gif", &[])));
    }
}
