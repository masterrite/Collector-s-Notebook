// build.rs — embeds the taskbar/exe icon on Windows. Harmless no-op on other OSes.
fn main() {
    #[cfg(target_os = "windows")]
    {
        let icon = "assets/icons/Collectors-Notebook.ico";
        if std::path::Path::new(icon).exists() {
            let mut res = winres::WindowsResource::new();
            res.set_icon(icon);
            let _ = res.compile();
        }
    }
}
