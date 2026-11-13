use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Mutex,
};
use tauri::{Manager, PhysicalPosition, PhysicalSize};

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Bounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

pub struct Geometry {
    path: PathBuf,
    bounds: Mutex<BTreeMap<String, Bounds>>,
    initialized: Mutex<BTreeSet<String>>,
}

impl Geometry {
    pub fn open(path: PathBuf) -> Self {
        let bounds = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            bounds: Mutex::new(bounds),
            initialized: Mutex::new(BTreeSet::new()),
        }
    }

    pub fn capture(&self, window: &tauri::Window) {
        // Initial native move/resize events must not overwrite a saved layout before restoration.
        if !self
            .initialized
            .lock()
            .map(|windows| windows.contains(window.label()))
            .unwrap_or(false)
        {
            return;
        }
        // Never replace the normal layout with minimized/maximized coordinates.
        if window.is_minimized().unwrap_or(true) || window.is_maximized().unwrap_or(true) {
            return;
        }
        if let (Ok(position), Ok(size), Ok(mut bounds)) = (
            window.outer_position(),
            window.inner_size(),
            self.bounds.lock(),
        ) {
            if size.width > 0 && size.height > 0 {
                bounds.insert(
                    window.label().to_string(),
                    Bounds {
                        x: position.x,
                        y: position.y,
                        width: size.width,
                        height: size.height,
                    },
                );
            }
        }
    }

    pub fn save(&self) {
        if let Ok(bounds) = self.bounds.lock() {
            if let Ok(bytes) = serde_json::to_vec(&*bounds) {
                if let Err(error) = std::fs::write(&self.path, bytes) {
                    eprintln!("窗口布局保存失败：{error}");
                }
            }
        }
    }

    pub fn restore(&self, window: &tauri::WebviewWindow) {
        self.restore_bounds(window);
        if let Ok(mut windows) = self.initialized.lock() {
            windows.insert(window.label().to_string());
        }
    }
    fn restore_bounds(&self, window: &tauri::WebviewWindow) {
        let saved = self
            .bounds
            .lock()
            .ok()
            .and_then(|bounds| bounds.get(window.label()).copied());
        let Some(mut saved) = saved else {
            return;
        };
        let Ok(monitors) = window.available_monitors() else {
            return;
        };
        // Pick the monitor containing the old origin; disconnected screens fall back to primary.
        let monitor = monitors
            .iter()
            .find(|monitor| {
                let area = monitor.work_area();
                saved.x >= area.position.x
                    && saved.y >= area.position.y
                    && i64::from(saved.x) < i64::from(area.position.x) + i64::from(area.size.width)
                    && i64::from(saved.y) < i64::from(area.position.y) + i64::from(area.size.height)
            })
            .cloned()
            .or_else(|| window.primary_monitor().ok().flatten());
        let Some(monitor) = monitor else {
            return;
        };
        let area = monitor.work_area();
        let scale = monitor.scale_factor();
        let (min_width, min_height) = if window.label() == "main" {
            (280., 200.)
        } else {
            (760., 500.)
        };
        saved.width = saved
            .width
            .max((min_width * scale) as u32)
            .min(area.size.width);
        saved.height = saved
            .height
            .max((min_height * scale) as u32)
            .min(area.size.height);
        saved.x = saved.x.clamp(
            area.position.x,
            area.position
                .x
                .saturating_add((area.size.width - saved.width) as i32),
        );
        saved.y = saved.y.clamp(
            area.position.y,
            area.position
                .y
                .saturating_add((area.size.height - saved.height) as i32),
        );
        let _ = window.set_size(PhysicalSize::new(saved.width, saved.height));
        let _ = window.set_position(PhysicalPosition::new(saved.x, saved.y));
    }
}

pub fn event(window: &tauri::Window, event: &tauri::WindowEvent) {
    let Some(state) = window.app_handle().try_state::<Geometry>() else {
        return;
    };
    match event {
        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => state.capture(window),
        tauri::WindowEvent::CloseRequested { .. } => {
            state.capture(window);
            state.save();
        }
        tauri::WindowEvent::Destroyed => {
            if let Ok(mut windows) = state.initialized.lock() {
                windows.remove(window.label());
            }
        }
        _ => {}
    }
}
