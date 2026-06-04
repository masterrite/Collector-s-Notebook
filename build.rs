fn main() {
    slint_build::compile("ui/main.slint").unwrap();

    // On Windows, embed the application icon into the .exe.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/Collector.ico");
        res.compile().unwrap();
    }
}
