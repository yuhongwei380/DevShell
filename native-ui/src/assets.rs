use gpui_kit::{AssetSource, SharedString};
use std::borrow::Cow;

pub struct Assets;

// The native shell uses the same paths as the WebView sprite. Keep a single
// source of truth for the artwork while letting GPUI paint it as vector masks.
fn shell_icon(path: &str) -> Option<Vec<u8>> {
    let name = path.strip_prefix("shell/")?.strip_suffix(".svg")?;
    let sprite = include_str!("../assets/icons.svg");
    let marker = format!("<symbol id=\"i-{name}\"");
    let start = sprite.find(&marker)?;
    let symbol = &sprite[start..];
    let body = &symbol[symbol.find('>')? + 1..];
    let body = &body[..body.find("</symbol>")?];
    Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"white\" stroke-width=\"1.6\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{body}</svg>").into_bytes())
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        let system:Option<&'static [u8]>=match path{
            "system/almalinux.svg"=>Some(include_bytes!("../assets/system-logos/almalinux.svg")),
            "system/alpinelinux.svg"=>Some(include_bytes!("../assets/system-logos/alpinelinux.svg")),
            "system/archlinux.svg"=>Some(include_bytes!("../assets/system-logos/archlinux.svg")),
            "system/centos.svg"=>Some(include_bytes!("../assets/system-logos/centos.svg")),
            "system/debian.svg"=>Some(include_bytes!("../assets/system-logos/debian.svg")),
            "system/fedora.svg"=>Some(include_bytes!("../assets/system-logos/fedora.svg")),
            "system/kalilinux.svg"=>Some(include_bytes!("../assets/system-logos/kalilinux.svg")),
            "system/linux.svg"=>Some(include_bytes!("../assets/system-logos/linux.svg")),
            "system/linuxmint.svg"=>Some(include_bytes!("../assets/system-logos/linuxmint.svg")),
            "system/opensuse.svg"=>Some(include_bytes!("../assets/system-logos/opensuse.svg")),
            "system/redhat.svg"=>Some(include_bytes!("../assets/system-logos/redhat.svg")),
            "system/rockylinux.svg"=>Some(include_bytes!("../assets/system-logos/rockylinux.svg")),
            "system/suse.svg"=>Some(include_bytes!("../assets/system-logos/suse.svg")),
            "system/ubuntu.svg"=>Some(include_bytes!("../assets/system-logos/ubuntu.svg")),
            _=>None,
        };
        if let Some(bytes)=system{return Ok(Some(Cow::Borrowed(bytes)));}
        if let Some(svg) = shell_icon(path) {
            return Ok(Some(Cow::Owned(svg)));
        }
        if path == "devshell.svg" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../web/assets/devshell.svg"
            ))));
        }
        ely_gpui_component::Assets::before(gpui_kit::assets::Assets).load(path)
    }
    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        ely_gpui_component::Assets::before(gpui_kit::assets::Assets).list(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_icons_resolve_from_the_original_sprite() {
        for name in [
            "settings",
            "github",
            "cpu",
            "server",
            "terminal",
            "folder",
            "erase",
            "disconnect",
            "font-increase",
            "font-decrease",
            "history",
            "sun",
            "moon",
            "close",
            "plus",
            "upload",
            "up",
            "file",
            "search",
            "refresh",
            "lock",
            "network",
            "key",
            "help-book",
            "minimise",
            "maximise",
            "chevron",
        ] {
            let svg = shell_icon(&format!("shell/{name}.svg")).expect(name);
            assert!(
                String::from_utf8(svg)
                    .unwrap()
                    .contains("viewBox=\"0 0 24 24\"")
            );
        }
        assert!(shell_icon("shell/missing.svg").is_none());
    }
}
