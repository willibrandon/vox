//! Build script for the Vox binary crate.
//!
//! On Windows, embeds the application icon into the executable via the
//! Windows resource compiler so it appears in the taskbar and window title bar.

fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icons/app-icon.ico");
        res.set("FileDescription", "Vox");
        res.set("ProductName", "Vox");
        if let Err(err) = res.compile() {
            eprintln!("winresource failed: {err}");
        }
    }
}
