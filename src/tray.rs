//! Programm-Icon und Tray-Symbol.
//!
//! Windows: Symbol im Infobereich (Doppelklick öffnet das Fenster, Menü mit
//! Dock/Overlay/Beenden); das Fenster wird direkt per Win32 ein- und
//! ausgeblendet, weil eframe bei verstecktem Fenster keine Frames zeichnet.
//! Andere Systeme: kein Tray (dort startet das Programm minimiert).

/// Programm-Icon: Sechseck (Host-Knoten) in Decker-Türkis auf Matrix-Indigo, RGBA.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let c = s / 2.0;
    let r_out = s * 0.47;
    let r_in = s * 0.36;
    let r_dot = s * 0.13;
    // Sechseck mit Spitze oben: Abstand zur Kante (Hexagon-Metrik)
    let hex = |x: f32, y: f32, r: f32| {
        let (dx, dy) = ((x - c).abs(), (y - c).abs());
        let k = 3f32.sqrt() / 2.0;
        dx <= r * k && dx * 0.5 + dy * k <= r * k
    };
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let px = if !hex(fx, fy, r_out) {
                [0, 0, 0, 0]
            } else if !hex(fx, fy, r_in) {
                [46, 230, 197, 255] // Rand: Türkis
            } else if ((fx - c).powi(2) + (fy - c).powi(2)).sqrt() <= r_dot {
                [255, 209, 102, 255] // Mitte: Gold
            } else {
                [14, 11, 38, 255] // Fläche: Indigo
            };
            out.extend_from_slice(&px);
        }
    }
    out
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use std::sync::Arc;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindow, SW_HIDE, SW_RESTORE, SW_SHOW,
    };

    /// Was die Menüpunkte tun (werden im Ereignis-Thread des Fensters aufgerufen).
    pub struct Actions {
        pub show: Box<dyn Fn() + Send + Sync>,
        pub open_dock: Box<dyn Fn() + Send + Sync>,
        pub open_overlay: Box<dyn Fn() + Send + Sync>,
        pub quit: Box<dyn Fn() + Send + Sync>,
    }

    pub struct Tray {
        _icon: TrayIcon,
    }

    impl Tray {
        pub fn new(actions: Actions) -> Result<Self, String> {
            let menu = Menu::new();
            let show = MenuItem::new("Fenster anzeigen", true, None);
            let dock = MenuItem::new("Bedien-Dock im Browser öffnen", true, None);
            let overlay = MenuItem::new("Overlay-Demo öffnen", true, None);
            let quit = MenuItem::new("Beenden", true, None);
            menu.append_items(&[
                &show,
                &dock,
                &overlay,
                &PredefinedMenuItem::separator(),
                &quit,
            ])
            .map_err(|e| e.to_string())?;

            let ids = (
                show.id().clone(),
                dock.id().clone(),
                overlay.id().clone(),
                quit.id().clone(),
            );
            let actions = Arc::new(actions);
            let a = actions.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if e.id == ids.0 {
                    (a.show)();
                } else if e.id == ids.1 {
                    (a.open_dock)();
                } else if e.id == ids.2 {
                    (a.open_overlay)();
                } else if e.id == ids.3 {
                    (a.quit)();
                }
            }));
            let a = actions.clone();
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| match e {
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
                | TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => (a.show)(),
                _ => {}
            }));

            let size = 32;
            let icon =
                Icon::from_rgba(super::icon_rgba(size), size, size).map_err(|e| e.to_string())?;
            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("Cyber Giveaway – Matrix-Run")
                .with_icon(icon)
                .with_menu_on_left_click(false)
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Self { _icon: tray })
        }
    }

    fn hwnd(h: isize) -> HWND {
        h as HWND
    }

    /// Fenster wieder anzeigen und nach vorne holen.
    pub fn show_window(h: isize) {
        if h == 0 {
            return;
        }
        unsafe {
            if IsIconic(hwnd(h)) != 0 {
                ShowWindow(hwnd(h), SW_RESTORE);
            } else {
                ShowWindow(hwnd(h), SW_SHOW);
            }
            SetForegroundWindow(hwnd(h));
        }
    }

    /// Fenster ausblenden (nur noch im Tray).
    pub fn hide_window(h: isize) {
        if h != 0 {
            unsafe {
                ShowWindow(hwnd(h), SW_HIDE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_shape() {
        let px = super::icon_rgba(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        let at = |x: usize, y: usize| &px[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(at(0, 0)[3], 0, "Ecke transparent");
        assert_eq!(at(16, 16), &[255, 209, 102, 255], "Mitte gold");
        assert_eq!(at(16, 2)[3], 255, "Spitze oben deckend");
    }
}
