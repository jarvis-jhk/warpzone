//! Library tile art: portrait (2:3) images from SteamGridDB, a file the
//! player picked, or the generated capsule for games without artwork.
//! Descriptors carry only the SteamGridDB id; images are looked up at run
//! time and cached by [`warpzone_core::griddb::Cache`].

use crate::art;
use crate::settings::ArtChoice;
use eframe::egui::ColorImage;
use warpzone_core::griddb::{Cache, GridDb};
use warpzone_protocol::descriptor::GameDescriptor;

/// Texture width of a tile (sharp at 2× on a 180 px tile).
const TEX_W: u32 = 360;

/// Where the shown art came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Griddb,
    File,
    Generated,
}

/// Cut `img` to 2:3 around its centre (cover, like Steam does).
fn cover(img: image::DynamicImage, w: u32, h: u32) -> image::RgbaImage {
    let (iw, ih) = (img.width().max(1), img.height().max(1));
    let (cw, ch) = if iw * h > ih * w { (ih * w / h, ih) } else { (iw, iw * h / w) };
    let cropped = img.crop_imm((iw - cw) / 2, (ih - ch) / 2, cw.max(1), ch.max(1));
    cropped.resize_exact(w, h, image::imageops::FilterType::Triangle).to_rgba8()
}

fn to_color_image(img: image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

/// A tile texture from image bytes (any format the launcher reads).
pub fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    Some(to_color_image(cover(img, TEX_W, TEX_W * 3 / 2)))
}

fn generated_rgba(d: &GameDescriptor, w: u32, h: u32) -> image::RgbaImage {
    let name = d.steam.as_ref().and_then(|s| s.name.clone()).unwrap_or(d.name.clone());
    let (cw, chh, px) = art::render(&art::Spec { id: &d.id, name: &name, accent: &d.accent, motif: d.motif.as_deref().unwrap_or("waves") }, art::Kind::Capsule);
    let img = image::RgbaImage::from_raw(cw, chh, px).unwrap_or_default();
    image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
}

/// The generated portrait (with the game's name: there is no artwork).
pub fn generated(d: &GameDescriptor) -> ColorImage {
    to_color_image(generated_rgba(d, TEX_W, TEX_W * 3 / 2))
}

/// The bytes of the art the player chose, or SteamGridDB's best grid for the
/// game. `None`: the generated art is used. Blocks (network): run in a job.
fn bytes(d: &GameDescriptor, choice: Option<&ArtChoice>, key: &str) -> Option<(Vec<u8>, Source)> {
    let db = GridDb::new(Some(key));
    let cache = Cache::default();
    match choice {
        Some(ArtChoice::File { path }) => match std::fs::read(path) {
            Ok(b) => return Some((b, Source::File)),
            Err(e) => log::warn!("tile art {path}: {e}"),
        },
        Some(ArtChoice::Griddb { url, .. }) => match cache.image(&db, url) {
            Ok(b) => return Some((b, Source::Griddb)),
            Err(e) => log::warn!("tile art {url}: {e}"),
        },
        Some(ArtChoice::Generated) => return None,
        None => {}
    }
    let id = d.steamgriddb?;
    // Offline start: the cached best grid first, without waiting for the network.
    if let Some(g) = cache.cached_default(id)
        && let Some(b) = cache.cached_image(&g.url)
    {
        return Some((b, Source::Griddb));
    }
    let page = cache.grids(&db, id).map_err(|e| log::warn!("SteamGridDB {id}: {e}")).ok()?;
    let best = page.grids.first()?;
    cache.image(&db, &best.url).map_err(|e| log::warn!("SteamGridDB {}: {e}", best.url)).ok().map(|b| (b, Source::Griddb))
}

/// The tile for a game. Blocks (network on the first run): call from a job.
pub fn resolve(d: &GameDescriptor, choice: Option<&ArtChoice>, key: &str) -> (ColorImage, Source) {
    if let Some((b, src)) = bytes(d, choice, key)
        && let Some(img) = decode(&b)
    {
        return (img, src);
    }
    (generated(d), Source::Generated)
}

/// The tile art as a 600×900 PNG for Steam's portrait capsule, if the game
/// has real artwork (SteamGridDB or the player's file).
pub fn capsule_png(d: &GameDescriptor, choice: Option<&ArtChoice>, key: &str) -> Option<Vec<u8>> {
    let (b, _) = bytes(d, choice, key)?;
    let img = cover(image::load_from_memory(&b).ok()?, 600, 900);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_cuts_to_two_by_three() {
        // 22:31 grids (660×930) lose a little width, wide pictures a lot.
        for (w, h) in [(660, 930), (600, 900), (1920, 1080), (100, 400)] {
            let img = image::DynamicImage::new_rgba8(w, h);
            let out = cover(img, 360, 540);
            assert_eq!((out.width(), out.height()), (360, 540));
        }
    }

    #[test]
    fn decodes_png_and_rejects_junk() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(600, 900).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let img = decode(&png.into_inner()).unwrap();
        assert_eq!(img.size, [360, 540]);
        assert!(decode(b"not an image").is_none());
    }
}
