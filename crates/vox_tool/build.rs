//! Build script for the vox-tool CLI binary.
//!
//! On Windows, embeds the application icon into the executable so it
//! appears in Explorer and the taskbar.

fn main() {
    #[cfg(target_os = "windows")]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            return;
        }

        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icons/app-icon.ico");
        res.set("FileDescription", "Vox Tool");
        res.set("ProductName", "Vox Tool");
        if let Err(err) = res.compile() {
            eprintln!("winresource failed: {err}");
        }
    }
}
