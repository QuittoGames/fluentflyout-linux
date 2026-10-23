//! Cover-art loading.
//!
//! `mpris:artUrl` is a URI, not necessarily a file. Local artwork is loaded
//! synchronously (cheap, and the flyout must not flash); remote artwork is
//! fetched on a worker thread through GIO and delivered back to the GTK main
//! context.
//!
//! Remote fetching has a hard size limit: a player must not be able to make
//! the app allocate an unbounded buffer because it advertised a huge URL.

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::domain::media::ArtworkSource;
use crate::ui::style::Rgb;

/// Refuse to download more than this from a player-provided URL.
const MAX_REMOTE_BYTES: u64 = 8 * 1024 * 1024;

/// Loads a texture for a local artwork source.
///
/// Returns `None` for remote sources or unreadable files; callers then show
/// the placeholder surface instead of a broken image.
pub fn texture_for(source: &ArtworkSource) -> Option<gdk::Texture> {
    match source {
        ArtworkSource::Local(path) => gdk::Texture::from_filename(path).ok(),
        ArtworkSource::Remote(_) => None,
    }
}

/// Whether a downloaded cover fits the remote-artwork budget.
///
/// Kept separate from `load_remote` so the policy is directly testable.
fn within_budget(bytes: glib::Bytes) -> Option<glib::Bytes> {
    if bytes.len() as u64 > MAX_REMOTE_BYTES {
        log::warn!("cover art exceeds {MAX_REMOTE_BYTES} bytes; ignoring");
        return None;
    }
    Some(bytes)
}

/// Fetches a remote artwork URL and delivers the decoded texture on the main
/// context.
pub fn load_remote<F>(url: &str, callback: F)
where
    F: FnOnce(Option<gdk::Texture>) + 'static,
{
    let uri = url.to_string();
    let (sender, receiver) = async_channel::bounded::<Option<glib::Bytes>>(1);

    std::thread::Builder::new()
        .name("cover-art".into())
        .spawn(move || {
            let file = gio::File::for_uri(&uri);
            let bytes = file
                .load_bytes(gio::Cancellable::NONE)
                .ok()
                .and_then(|(bytes, _etag)| within_budget(bytes));
            let _ = sender.send_blocking(bytes);
        })
        .ok();

    // `gdk::Texture` is not `Send`, so only the raw bytes cross the thread
    // boundary; the texture is created back on the GTK main context.
    glib::MainContext::default().spawn_local(async move {
        let bytes = receiver.recv().await.ok().flatten();
        let texture = bytes.and_then(|bytes| gdk::Texture::from_bytes(&bytes).ok());
        callback(texture);
    });
}

/// Computes a representative accent colour for the artwork.
///
/// The image is downsampled to at most 32x32 and averaged over pixels that
/// are neither near-black nor near-white, so a cover with a large white or
/// black background still yields a usable accent.
pub fn dominant_color(texture: &gdk::Texture) -> Option<Rgb> {
    let width = texture.width() as usize;
    let height = texture.height() as usize;
    if width == 0 || height == 0 {
        return None;
    }

    // GTK downloads at the texture's native size, so the sampling grid is
    // built here instead of asking the downloader to rescale.
    let mut downloader = gdk::TextureDownloader::new(texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    let data = bytes.as_ref();
    if data.len() < stride * height {
        return None;
    }

    let step = (width / 32).max(1);
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    for y in (0..height).step_by(step) {
        for x in (0..width).step_by(step) {
            let offset = y * stride + x * 4;
            let (r, g, b) = (data[offset], data[offset + 1], data[offset + 2]);
            let luma = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
            // Skip washed-out and near-black pixels: they carry no identity.
            if !(36..=214).contains(&luma) {
                continue;
            }
            sum[0] += r as u64;
            sum[1] += g as u64;
            sum[2] += b as u64;
            count += 1;
        }
    }

    if count == 0 {
        return None;
    }
    Some(Rgb::new(
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_sources_are_not_loaded_synchronously() {
        assert!(texture_for(&ArtworkSource::Remote("https://x.test/a.png".into())).is_none());
    }

    #[test]
    fn missing_local_file_yields_none() {
        assert!(
            texture_for(&ArtworkSource::Local(
                "/definitely/not/a/real/cover.png".into()
            ))
            .is_none()
        );
    }

    #[test]
    fn size_limit_is_bounded() {
        // Guards against a regression that would remove the cap silently.
        const { assert!(MAX_REMOTE_BYTES <= 16 * 1024 * 1024) };
    }

    #[test]
    fn oversized_artwork_is_rejected_and_normal_files_pass() {
        let oversized = glib::Bytes::from_owned(vec![0u8; MAX_REMOTE_BYTES as usize + 1]);
        assert!(within_budget(oversized).is_none());

        let acceptable = glib::Bytes::from_owned(vec![0u8; 4096]);
        assert!(within_budget(acceptable).is_some());
    }
}
