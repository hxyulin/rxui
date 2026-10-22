//! System appearance and accessibility preference detection.
//!
//! winit reports the light/dark appearance on macOS and Windows. The remaining
//! values come from safe platform bindings: AppKit through objc2 on macOS, the
//! current user's registry on Windows, and the XDG desktop portal on Linux/BSD.
//! Every read is best effort; an unavailable value keeps its default.
use crate::{ColorScheme, SystemPreferences};
use astrelis_winit::winit::{event_loop::ActiveEventLoop, window::Theme as NativeTheme};

impl SystemPreferences {
    /// Reads the current system preferences, for custom hosts that own their event
    /// loop. `Application` keeps its own snapshot current; use
    /// `AppContext::system_preferences` there. On Linux/BSD this asks the XDG desktop
    /// portal over the session bus and waits at most a few hundred milliseconds.
    pub fn detect(event_loop: &ActiveEventLoop) -> Self {
        detect(event_loop.system_theme())
    }
}

/// Platform values with winit's appearance taking precedence where it has one.
pub(super) fn detect(system: Option<NativeTheme>) -> SystemPreferences {
    let mut preferences = platform::read();
    if let Some(theme) = system {
        preferences.color_scheme = Some(scheme(theme));
    }
    preferences
}
pub(super) fn scheme(theme: NativeTheme) -> ColorScheme {
    match theme {
        NativeTheme::Light => ColorScheme::Light,
        NativeTheme::Dark => ColorScheme::Dark,
    }
}
pub(super) fn native_theme(scheme: ColorScheme) -> NativeTheme {
    match scheme {
        ColorScheme::Light => NativeTheme::Light,
        ColorScheme::Dark => NativeTheme::Dark,
    }
}
/// Whether focus changes should re-read the platform. Linux pushes portal changes
/// from a watcher thread instead, so it skips the session-bus round trips.
pub(super) const REFRESH_ON_FOCUS: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// One sRGB channel in `0..=1` as the linear value RXUI colors use.
#[cfg(any(
    target_os = "windows",
    all(unix, not(any(target_os = "ios", target_os = "android")))
))]
fn srgb(channel: f64) -> f32 {
    let v = channel.clamp(0., 1.) as f32;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use crate::SystemPreferences;
    use objc2_app_kit::{NSColor, NSColorSpace, NSWorkspace};

    pub fn read() -> SystemPreferences {
        let workspace = NSWorkspace::sharedWorkspace();
        // The accent is a dynamic color; resolving it in sRGB gives its current value.
        let accent = NSColor::controlAccentColor()
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
            .map(|c| {
                [
                    super::srgb(c.redComponent()),
                    super::srgb(c.greenComponent()),
                    super::srgb(c.blueComponent()),
                    1.,
                ]
            });
        SystemPreferences {
            color_scheme: None,
            high_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
            reduce_motion: workspace.accessibilityDisplayShouldReduceMotion(),
            reduce_transparency: workspace.accessibilityDisplayShouldReduceTransparency(),
            accent_color: accent,
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use crate::SystemPreferences;
    use windows_registry::CURRENT_USER;

    fn dword(path: &str, name: &str) -> Option<u32> {
        CURRENT_USER.open(path).and_then(|k| k.get_u32(name)).ok()
    }
    fn string(path: &str, name: &str) -> Option<String> {
        CURRENT_USER
            .open(path)
            .and_then(|k| k.get_string(name))
            .ok()
    }
    pub fn read() -> SystemPreferences {
        // HIGHCONTRAST.dwFlags, stored as decimal text; HCF_HIGHCONTRASTON is bit 0.
        let high_contrast = string(r"Control Panel\Accessibility\HighContrast", "Flags")
            .and_then(|flags| flags.trim().parse::<u32>().ok())
            .is_some_and(|flags| flags & 1 != 0);
        // "Animation effects" off also turns off window minimize/maximize animation.
        let reduce_motion = string(r"Control Panel\Desktop\WindowMetrics", "MinAnimate")
            .is_some_and(|value| value.trim() == "0");
        let reduce_transparency = dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "EnableTransparency",
        ) == Some(0);
        // 0xAABBGGRR, as DwmGetColorizationColor's companion AccentColor value.
        let accent_color = dword(r"Software\Microsoft\Windows\DWM", "AccentColor").map(|v| {
            let channel = |shift: u32| super::srgb(f64::from((v >> shift) & 0xFF) / 255.);
            [channel(0), channel(8), channel(16), 1.]
        });
        SystemPreferences {
            color_scheme: None,
            high_contrast,
            reduce_motion,
            reduce_transparency,
            accent_color,
        }
    }
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
mod platform {
    use crate::{ColorScheme, SystemPreferences};
    use futures_lite::StreamExt;
    use std::time::Duration;
    use zbus::{OwnedVariant, Variant};

    const APPEARANCE: &str = "org.freedesktop.appearance";
    const GNOME: &str = "org.gnome.desktop.interface";

    async fn portal() -> zbus::Result<zbus::Proxy<'static>> {
        let connection = zbus::Connection::session().await?;
        zbus::Proxy::new(
            &connection,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
        )
        .await
    }
    async fn read_one(proxy: &zbus::Proxy<'_>, namespace: &str, key: &str) -> Option<OwnedVariant> {
        // ReadOne is portal version 2; Read wraps the value in another variant.
        match proxy
            .call::<_, _, OwnedVariant>("ReadOne", &(namespace, key))
            .await
        {
            Ok(value) => Some(value),
            Err(_) => proxy
                .call::<_, _, OwnedVariant>("Read", &(namespace, key))
                .await
                .ok(),
        }
    }
    fn unwrap<'v, 'a>(value: &'v Variant<'a>) -> &'v Variant<'a> {
        match value {
            Variant::Variant(inner) => unwrap(inner),
            value => value,
        }
    }
    /// Applies one portal setting; returns whether it is one RXUI tracks.
    pub(super) fn apply(
        preferences: &mut SystemPreferences,
        namespace: &str,
        key: &str,
        value: &Variant<'_>,
    ) -> bool {
        let value = unwrap(value);
        match (namespace, key, value) {
            (APPEARANCE, "color-scheme", Variant::U32(v)) => {
                preferences.color_scheme = match v {
                    1 => Some(ColorScheme::Dark),
                    2 => Some(ColorScheme::Light),
                    _ => None,
                };
            }
            (APPEARANCE, "contrast", Variant::U32(v)) => preferences.high_contrast = *v == 1,
            (APPEARANCE, "reduced-motion", Variant::U32(v)) => preferences.reduce_motion = *v == 1,
            (GNOME, "enable-animations", Variant::Bool(enabled)) => {
                preferences.reduce_motion = !enabled
            }
            (APPEARANCE, "accent-color", Variant::Structure(s)) => {
                let channels: Vec<f64> = s
                    .fields()
                    .iter()
                    .filter_map(|f| match unwrap(f) {
                        Variant::F64(v) => Some(*v),
                        _ => None,
                    })
                    .collect();
                // Out-of-range channels mean "no accent".
                preferences.accent_color = match channels[..] {
                    [r, g, b] if [r, g, b].iter().all(|c| (0. ..=1.).contains(c)) => {
                        Some([super::srgb(r), super::srgb(g), super::srgb(b), 1.])
                    }
                    _ => None,
                };
            }
            _ => return false,
        }
        true
    }
    async fn read_all(proxy: &zbus::Proxy<'_>) -> SystemPreferences {
        let mut preferences = SystemPreferences::default();
        // The portal value, when present, takes precedence over GNOME's setting.
        for (namespace, key) in [
            (GNOME, "enable-animations"),
            (APPEARANCE, "color-scheme"),
            (APPEARANCE, "contrast"),
            (APPEARANCE, "reduced-motion"),
            (APPEARANCE, "accent-color"),
        ] {
            if let Some(value) = read_one(proxy, namespace, key).await {
                apply(&mut preferences, namespace, key, &value);
            }
        }
        preferences
    }
    pub fn read() -> SystemPreferences {
        zbus::block_on(futures_lite::future::or(
            async {
                match portal().await {
                    Ok(proxy) => read_all(&proxy).await,
                    Err(_) => SystemPreferences::default(),
                }
            },
            async {
                async_io::Timer::after(Duration::from_millis(300)).await;
                SystemPreferences::default()
            },
        ))
    }
    /// Delivers the initial snapshot and every tracked portal change until `send`
    /// reports that the receiver is gone. Without a portal the thread just ends.
    pub fn watch(send: impl Fn(SystemPreferences) -> bool + Send + 'static) {
        let _ = std::thread::Builder::new()
            .name("rxui-portal-settings".into())
            .spawn(move || {
                zbus::block_on(async move {
                    let Ok(proxy) = portal().await else {
                        return;
                    };
                    let Ok(mut changes) = proxy.receive_signal("SettingChanged").await else {
                        return;
                    };
                    let mut preferences = read_all(&proxy).await;
                    if !send(preferences.clone()) {
                        return;
                    }
                    while let Some(message) = changes.next().await {
                        let Ok((namespace, key, value)) =
                            message
                                .body()
                                .deserialize::<(String, String, OwnedVariant)>()
                        else {
                            continue;
                        };
                        if apply(&mut preferences, &namespace, &key, &value)
                            && !send(preferences.clone())
                        {
                            return;
                        }
                    }
                })
            });
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    )
)))]
mod platform {
    pub fn read() -> crate::SystemPreferences {
        crate::SystemPreferences::default()
    }
}

/// Starts pushing portal changes on Linux/BSD; other platforms refresh on focus.
pub(super) fn watch(send: impl Fn(SystemPreferences) -> bool + Send + 'static) {
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ))]
    platform::watch(send);
    #[cfg(not(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    )))]
    drop(send);
}
