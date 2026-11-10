//! Native window glass on macOS (issue #166, spike). Everything else is a no-op.
//!
//! The window is transparent on macOS (`tauri.macos.conf.json`). When the person asks for it
//! (`JARVIS_NATIVE_GLASS=1`), the web view is put inside Apple's own glass: `NSGlassEffectView`
//! ("Liquid Glass") on macOS 26 and later, `NSVisualEffectView` blur on older macOS, both through the
//! `window-vibrancy` crate that Tauri already uses. The page then makes its backgrounds see-through
//! (`html[data-native-glass]`, `src/nativeGlass.css`). If neither effect can be applied the window
//! stays as it was and the page is not told, so nothing goes see-through for no reason.
//!
//! Must run on the main thread (the Tauri setup closure does). No webview permission is involved.

use std::sync::atomic::{AtomicBool, Ordering};

/// Set once an effect is on, so the page can be marked (also after a reload).
static APPLIED: AtomicBool = AtomicBool::new(false);

/// True once a native glass effect is applied to the window.
pub fn applied() -> bool {
    APPLIED.load(Ordering::Acquire)
}

/// How see-through Apple's glass is: the regular one, or the clearer variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassStyle {
    Regular,
    Clear,
}

/// `1` or `true` or `regular`: the regular glass. `clear`: the clearer one. Anything else, or
/// nothing: off.
fn style_of(value: Option<&str>) -> Option<GlassStyle> {
    match value.map(|v| v.trim().to_lowercase()).as_deref() {
        Some("1") | Some("true") | Some("regular") => Some(GlassStyle::Regular),
        Some("clear") => Some(GlassStyle::Clear),
        _ => None,
    }
}

/// The spike switch: `JARVIS_NATIVE_GLASS=1` (or `regular`, or `clear`). Off unless asked for.
pub fn requested() -> Option<GlassStyle> {
    style_of(std::env::var("JARVIS_NATIVE_GLASS").ok().as_deref())
}

/// Applies the glass to the window. Returns whether an effect is on.
#[cfg(target_os = "macos")]
pub fn apply(window: &tauri::WebviewWindow, style: GlassStyle) -> bool {
    use window_vibrancy::{
        LiquidGlassOptions, NSGlassEffectViewStyle, NSVisualEffectMaterial, apply_liquid_glass,
        apply_vibrancy,
    };
    let native = match style {
        GlassStyle::Regular => NSGlassEffectViewStyle::Regular,
        GlassStyle::Clear => NSGlassEffectViewStyle::Clear,
    };
    let glass = apply_liquid_glass(window, LiquidGlassOptions::new(native));
    let ok = match glass {
        Ok(()) => {
            tracing::info!(style = ?style, "glass: liquid glass applied");
            true
        }
        Err(e) => {
            tracing::info!(error = ?e, "glass: liquid glass not available, trying the older blur");
            match apply_vibrancy(
                window,
                NSVisualEffectMaterial::UnderWindowBackground,
                None,
                None,
            ) {
                Ok(()) => {
                    tracing::info!("glass: window blur applied");
                    true
                }
                Err(e) => {
                    tracing::warn!(error = ?e, "glass: no native effect could be applied");
                    false
                }
            }
        }
    };
    APPLIED.store(ok, Ordering::Release);
    ok
}

/// Other systems: no native glass.
#[cfg(not(target_os = "macos"))]
pub fn apply(_window: &tauri::WebviewWindow, _style: GlassStyle) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_off_unless_asked_for_and_the_value_picks_the_style() {
        assert_eq!(style_of(None), None);
        assert_eq!(style_of(Some("")), None);
        assert_eq!(style_of(Some("0")), None);
        assert_eq!(style_of(Some("yes please")), None);
        assert_eq!(style_of(Some("1")), Some(GlassStyle::Regular));
        assert_eq!(style_of(Some(" true ")), Some(GlassStyle::Regular));
        assert_eq!(style_of(Some("Regular")), Some(GlassStyle::Regular));
        assert_eq!(style_of(Some("CLEAR")), Some(GlassStyle::Clear));
    }

    #[test]
    fn nothing_is_applied_before_apply_runs() {
        assert!(!applied());
    }
}
