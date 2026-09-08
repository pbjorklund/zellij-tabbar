// Adapted from zellij-vertical-tabs by Alex Lau. See NOTICE and LICENSE.

#[cfg(target_family = "wasm")]
use zellij_tabbar::plugin::{Host, Tabbar};
#[cfg(target_family = "wasm")]
use zellij_tile::prelude::*;

#[cfg(target_family = "wasm")]
#[derive(Default)]
struct ZellijHost;

#[cfg(target_family = "wasm")]
impl Host for ZellijHost {
    fn plugin_id(&mut self) -> u32 {
        get_plugin_ids().plugin_id
    }

    fn subscribe(&mut self, events: &[EventType]) {
        subscribe(events);
    }

    fn request_permissions(&mut self, permissions: &[PermissionType]) {
        request_permission(permissions);
    }

    fn set_selectable(&mut self, selectable: bool) {
        set_selectable(selectable);
    }

    fn switch_tab(&mut self, index: u32) {
        switch_tab_to(index);
    }

    fn render(&mut self, frame: &str) {
        print!("{frame}");
    }

    fn log(&mut self, message: &str) {
        eprintln!("{message}");
    }
}

#[cfg(target_family = "wasm")]
type State = Tabbar<ZellijHost>;
#[cfg(target_family = "wasm")]
register_plugin!(State);

#[cfg(not(target_family = "wasm"))]
fn main() {
    eprintln!("Build for wasm32-wasip1 and load zellij-tabbar.wasm inside Zellij.");
    std::process::exit(1);
}
