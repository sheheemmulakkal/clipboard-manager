//! Embedded SVG icons, recoloured at load time and cached as textures.
//!
//! The icons are simple 24×24 stroke drawings (`assets/icons/*.svg`) using
//! `currentColor`, which is replaced with a concrete colour before the SVG
//! is rasterised by gdk-pixbuf's librsvg loader. Rendering at 2× the display
//! size keeps them crisp on HiDPI screens.

use std::cell::RefCell;
use std::collections::HashMap;

use gdk_pixbuf::prelude::*;
use gdk4::subclass::prelude::ObjectSubclassIsExt;
use gtk4::prelude::WidgetExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    FileText,
    Type,
    Terminal,
    Code,
    Link,
    Shield,
    Mail,
    Folder,
    Palette,
    Image,
    Pin,
    PinFilled,
    Menu,
    X,
    Search,
    Copy,
    Trash,
    ArrowUp,
    Eye,
    Pencil,
    Tag,
    Pause,
    Play,
    Settings,
    Power,
    ChevronRight,
    ChevronLeft,
    Plus,
    Clipboard,
    Keyboard,
    Check,
}

impl Icon {
    #[cfg(test)]
    pub const ALL: &'static [Icon] = &[Icon::FileText, Icon::Type, Icon::Terminal, Icon::Code, Icon::Link, Icon::Shield, Icon::Mail, Icon::Folder, Icon::Palette, Icon::Image, Icon::Pin, Icon::PinFilled, Icon::Menu, Icon::X, Icon::Search, Icon::Copy, Icon::Trash, Icon::ArrowUp, Icon::Eye, Icon::Pencil, Icon::Tag, Icon::Pause, Icon::Play, Icon::Settings, Icon::Power, Icon::ChevronRight, Icon::ChevronLeft, Icon::Plus, Icon::Clipboard, Icon::Keyboard, Icon::Check];

    fn svg(self) -> &'static str {
        match self {
            Icon::FileText => include_str!("../../assets/icons/file-text.svg"),
            Icon::Type => include_str!("../../assets/icons/type.svg"),
            Icon::Terminal => include_str!("../../assets/icons/terminal.svg"),
            Icon::Code => include_str!("../../assets/icons/code.svg"),
            Icon::Link => include_str!("../../assets/icons/link.svg"),
            Icon::Shield => include_str!("../../assets/icons/shield.svg"),
            Icon::Mail => include_str!("../../assets/icons/mail.svg"),
            Icon::Folder => include_str!("../../assets/icons/folder.svg"),
            Icon::Palette => include_str!("../../assets/icons/palette.svg"),
            Icon::Image => include_str!("../../assets/icons/image.svg"),
            Icon::Pin => include_str!("../../assets/icons/pin.svg"),
            Icon::PinFilled => include_str!("../../assets/icons/pin-filled.svg"),
            Icon::Menu => include_str!("../../assets/icons/menu.svg"),
            Icon::X => include_str!("../../assets/icons/x.svg"),
            Icon::Search => include_str!("../../assets/icons/search.svg"),
            Icon::Copy => include_str!("../../assets/icons/copy.svg"),
            Icon::Trash => include_str!("../../assets/icons/trash.svg"),
            Icon::ArrowUp => include_str!("../../assets/icons/arrow-up.svg"),
            Icon::Eye => include_str!("../../assets/icons/eye.svg"),
            Icon::Pencil => include_str!("../../assets/icons/pencil.svg"),
            Icon::Tag => include_str!("../../assets/icons/tag.svg"),
            Icon::Pause => include_str!("../../assets/icons/pause.svg"),
            Icon::Play => include_str!("../../assets/icons/play.svg"),
            Icon::Settings => include_str!("../../assets/icons/settings.svg"),
            Icon::Power => include_str!("../../assets/icons/power.svg"),
            Icon::ChevronRight => include_str!("../../assets/icons/chevron-right.svg"),
            Icon::ChevronLeft => include_str!("../../assets/icons/chevron-left.svg"),
            Icon::Plus => include_str!("../../assets/icons/plus.svg"),
            Icon::Clipboard => include_str!("../../assets/icons/clipboard.svg"),
            Icon::Keyboard => include_str!("../../assets/icons/keyboard.svg"),
            Icon::Check => include_str!("../../assets/icons/check.svg"),
        }
    }
}

/// Icon shown in a row's tile for each kind of content.
pub fn for_kind(kind: crate::clipboard::kind::ContentKind) -> Icon {
    use crate::clipboard::kind::ContentKind::*;
    match kind {
        Text => Icon::Type,
        Url => Icon::Link,
        Email => Icon::Mail,
        Path => Icon::Folder,
        Shell => Icon::Terminal,
        Code => Icon::Code,
        Color => Icon::Palette,
        Secret => Icon::Shield,
        Image | Screenshot => Icon::Image,
    }
}

/// The icon's SVG source with `currentColor` replaced by `color`.
fn svg_for(icon: Icon, color: &str) -> String {
    icon.svg().replace("currentColor", color)
}

fn render_pixbuf(icon: Icon, color: &str, px: i32) -> Result<gdk_pixbuf::Pixbuf, glib::Error> {
    let loader = gdk_pixbuf::PixbufLoader::with_type("svg")?;
    loader.set_size(px, px);
    loader.write(svg_for(icon, color).as_bytes())?;
    loader.close()?;
    loader
        .pixbuf()
        .ok_or_else(|| glib::Error::new(glib::FileError::Failed, "svg loader returned no image"))
}

/// Rendered at this multiple of the display size, so icons stay sharp on
/// HiDPI screens; [`HiDpi`] reports the logical size to GTK.
const OVERSAMPLE: i32 = 2;

thread_local! {
    static CACHE: RefCell<HashMap<(Icon, String, i32), gdk4::Paintable>> = RefCell::new(HashMap::new());
}

/// Paintable of `icon` in `color` (a `#rrggbb` hex) with a logical size of
/// `px` × `px`.
pub fn paintable(icon: Icon, color: &str, px: i32) -> Option<gdk4::Paintable> {
    let key = (icon, color.to_string(), px);
    if let Some(p) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Some(p);
    }
    match render_pixbuf(icon, color, px * OVERSAMPLE) {
        Ok(pb) => {
            #[allow(deprecated)] // Texture::for_pixbuf is fine on GTK 4.6–4.14
            let tex = gdk4::Texture::for_pixbuf(&pb);
            let p: gdk4::Paintable = HiDpi::new(tex, px).upcast();
            CACHE.with(|c| c.borrow_mut().insert(key, p.clone()));
            Some(p)
        }
        Err(e) => {
            tracing::warn!("[icons] cannot render {icon:?}: {e} (is librsvg2-common installed?)");
            None
        }
    }
}

/// A `gtk4::Image` showing `icon` at `px` pixels.
pub fn image(icon: Icon, color: &str, px: i32) -> gtk4::Image {
    let img = match paintable(icon, color, px) {
        Some(p) => gtk4::Image::from_paintable(Some(&p)),
        None => gtk4::Image::new(),
    };
    img.set_pixel_size(px);
    // GtkImage scales a paintable to its allocation; centring keeps the
    // allocation at the natural (px × px) size.
    img.set_halign(gtk4::Align::Center);
    img.set_valign(gtk4::Align::Center);
    img
}

// ── HiDpi paintable ───────────────────────────────────────────────────────────

glib::wrapper! {
    /// A texture drawn at a smaller logical size than its pixel size.
    pub struct HiDpi(ObjectSubclass<imp::HiDpi>) @implements gdk4::Paintable;
}

impl HiDpi {
    fn new(texture: gdk4::Texture, logical_px: i32) -> Self {
        let obj: Self = glib::Object::new();
        let imp = obj.imp();
        *imp.texture.borrow_mut() = Some(texture);
        imp.size.set(logical_px);
        obj
    }
}

mod imp {
    use std::cell::{Cell, RefCell};

    use gdk4::prelude::*;
    use gdk4::subclass::prelude::*;

    #[derive(Default)]
    pub struct HiDpi {
        pub texture: RefCell<Option<gdk4::Texture>>,
        pub size:    Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HiDpi {
        const NAME: &'static str = "ClipboardManagerHiDpiIcon";
        type Type = super::HiDpi;
        type Interfaces = (gdk4::Paintable,);
    }

    impl ObjectImpl for HiDpi {}

    impl PaintableImpl for HiDpi {
        fn intrinsic_width(&self) -> i32 {
            self.size.get()
        }

        fn intrinsic_height(&self) -> i32 {
            self.size.get()
        }

        fn flags(&self) -> gdk4::PaintableFlags {
            gdk4::PaintableFlags::SIZE | gdk4::PaintableFlags::CONTENTS
        }

        fn snapshot(&self, snapshot: &gdk4::Snapshot, width: f64, height: f64) {
            if let Some(t) = self.texture.borrow().as_ref() {
                t.snapshot(snapshot, width, height);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_is_recoloured() {
        let svg = svg_for(Icon::Pin, "#abcdef");
        assert!(svg.contains("#abcdef"));
        assert!(!svg.contains("currentColor"));
    }

    #[test]
    fn kinds_have_distinct_icons() {
        use crate::clipboard::kind::ContentKind::*;
        assert_eq!(for_kind(Url), Icon::Link);
        assert_eq!(for_kind(Shell), Icon::Terminal);
        assert_eq!(for_kind(Secret), Icon::Shield);
        assert_eq!(for_kind(Text), Icon::Type);
        assert_ne!(for_kind(Code), for_kind(Text));
    }

    #[test]
    fn every_icon_renders() {
        for icon in Icon::ALL {
            let pb = render_pixbuf(*icon, "#ffffff", 32)
                .unwrap_or_else(|e| panic!("{icon:?}: {e}"));
            assert_eq!((pb.width(), pb.height()), (32, 32), "{icon:?}");
        }
    }
}
