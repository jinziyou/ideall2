use super::*;
use gpui::{Bounds, Pixels, Size, TitlebarOptions, WindowBounds, WindowOptions, point};

#[cfg(target_os = "linux")]
use x11rb::{connection::Connection as _, protocol::randr::ConnectionExt as _};

const MAIN_WINDOW_WIDTH: f32 = 1280.0;
const MAIN_WINDOW_HEIGHT: f32 = 820.0;
const MAIN_WINDOW_MIN_WIDTH: f32 = 920.0;
const MAIN_WINDOW_MIN_HEIGHT: f32 = 600.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MonitorGeometry {
    primary: bool,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

pub(super) fn desktop_window_options(cx: &App) -> WindowOptions {
    let preferred_size = size(px(MAIN_WINDOW_WIDTH), px(MAIN_WINDOW_HEIGHT));
    let preferred_min_size = size(px(MAIN_WINDOW_MIN_WIDTH), px(MAIN_WINDOW_MIN_HEIGHT));
    let primary_display = cx.primary_display();
    let display_id = primary_display.as_ref().map(|display| display.id());
    let (window_bounds, window_min_size) = primary_display
        .as_ref()
        .map(|display| {
            let display_bounds = display.bounds();
            let main_monitor_bounds =
                platform_main_monitor_bounds(display_bounds).unwrap_or(display_bounds);
            (
                WindowBounds::Windowed(centered_window_bounds(main_monitor_bounds, preferred_size)),
                preferred_min_size.min(&main_monitor_bounds.size),
            )
        })
        .unwrap_or_else(|| {
            (
                WindowBounds::centered(preferred_size, cx),
                preferred_min_size,
            )
        });

    WindowOptions {
        window_bounds: Some(window_bounds),
        titlebar: Some(TitlebarOptions {
            title: Some("ideall · 本地信息终端".into()),
            ..Default::default()
        }),
        display_id,
        window_min_size: Some(window_min_size),
        app_id: Some("com.jinziyou.ideall".into()),
        ..Default::default()
    }
}

fn centered_window_bounds(
    display_bounds: Bounds<Pixels>,
    preferred_size: Size<Pixels>,
) -> Bounds<Pixels> {
    let fitted_size = preferred_size.min(&display_bounds.size);
    Bounds::centered_at(display_bounds.center(), fitted_size)
}

fn monitor_bounds_from_x11(
    virtual_screen_bounds: Bounds<Pixels>,
    physical_screen_size: (u16, u16),
    monitors: &[MonitorGeometry],
) -> Option<Bounds<Pixels>> {
    let monitor = monitors
        .iter()
        .filter(|monitor| monitor.width > 0 && monitor.height > 0)
        .find(|monitor| monitor.primary)
        .or_else(|| {
            monitors
                .iter()
                .find(|monitor| monitor.width > 0 && monitor.height > 0)
        })?;
    let (physical_width, physical_height) = physical_screen_size;
    if physical_width == 0 || physical_height == 0 {
        return None;
    }
    let logical_per_physical_x =
        virtual_screen_bounds.size.width.as_f32() / f32::from(physical_width);
    let logical_per_physical_y =
        virtual_screen_bounds.size.height.as_f32() / f32::from(physical_height);
    if !logical_per_physical_x.is_finite()
        || !logical_per_physical_y.is_finite()
        || logical_per_physical_x <= 0.0
        || logical_per_physical_y <= 0.0
    {
        return None;
    }

    Some(Bounds::new(
        point(
            px(virtual_screen_bounds.origin.x.as_f32()
                + f32::from(monitor.x) * logical_per_physical_x),
            px(virtual_screen_bounds.origin.y.as_f32()
                + f32::from(monitor.y) * logical_per_physical_y),
        ),
        size(
            px(f32::from(monitor.width) * logical_per_physical_x),
            px(f32::from(monitor.height) * logical_per_physical_y),
        ),
    ))
}

#[cfg(target_os = "linux")]
fn platform_main_monitor_bounds(virtual_screen_bounds: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return None;
    }
    let (connection, screen_index) = x11rb::connect(None).ok()?;
    let screen = connection.setup().roots.get(screen_index)?;
    let reply = connection
        .randr_get_monitors(screen.root, true)
        .ok()?
        .reply()
        .ok()?;
    let monitors = reply
        .monitors
        .iter()
        .map(|monitor| MonitorGeometry {
            primary: monitor.primary,
            x: monitor.x,
            y: monitor.y,
            width: monitor.width,
            height: monitor.height,
        })
        .collect::<Vec<_>>();
    monitor_bounds_from_x11(
        virtual_screen_bounds,
        (screen.width_in_pixels, screen.height_in_pixels),
        &monitors,
    )
}

#[cfg(not(target_os = "linux"))]
fn platform_main_monitor_bounds(_virtual_screen_bounds: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x11_combined_desktop_uses_first_monitor_when_primary_is_missing() {
        let virtual_screen = Bounds::new(point(px(0.0), px(0.0)), size(px(3840.0), px(1080.0)));
        let monitors = [
            MonitorGeometry {
                primary: false,
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            MonitorGeometry {
                primary: false,
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ];

        let monitor = monitor_bounds_from_x11(virtual_screen, (3840, 1080), &monitors).unwrap();
        let window =
            centered_window_bounds(monitor, size(px(MAIN_WINDOW_WIDTH), px(MAIN_WINDOW_HEIGHT)));

        assert_eq!(monitor.size, size(px(1920.0), px(1080.0)));
        assert_eq!(window.origin, point(px(320.0), px(130.0)));
    }

    #[test]
    fn x11_combined_desktop_prefers_the_marked_primary_monitor() {
        let virtual_screen = Bounds::new(point(px(0.0), px(0.0)), size(px(3840.0), px(1080.0)));
        let monitors = [
            MonitorGeometry {
                primary: false,
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            MonitorGeometry {
                primary: true,
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ];

        let monitor = monitor_bounds_from_x11(virtual_screen, (3840, 1080), &monitors).unwrap();
        let window =
            centered_window_bounds(monitor, size(px(MAIN_WINDOW_WIDTH), px(MAIN_WINDOW_HEIGHT)));

        assert_eq!(monitor.origin, point(px(1920.0), px(0.0)));
        assert_eq!(window.origin, point(px(2240.0), px(130.0)));
    }

    #[test]
    fn x11_monitor_geometry_scales_to_gpui_logical_pixels() {
        let virtual_screen = Bounds::new(point(px(0.0), px(0.0)), size(px(3072.0), px(864.0)));
        let monitors = [MonitorGeometry {
            primary: true,
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        }];

        let monitor = monitor_bounds_from_x11(virtual_screen, (3840, 1080), &monitors).unwrap();

        assert_eq!(monitor.origin, point(px(1536.0), px(0.0)));
        assert_eq!(monitor.size, size(px(1536.0), px(864.0)));
    }

    #[test]
    fn main_window_fits_small_displays_without_leaving_the_screen() {
        let display = Bounds::new(point(px(100.0), px(200.0)), size(px(800.0), px(500.0)));

        let actual =
            centered_window_bounds(display, size(px(MAIN_WINDOW_WIDTH), px(MAIN_WINDOW_HEIGHT)));

        assert_eq!(actual, display);
    }
}
