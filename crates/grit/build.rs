//! Embeds the Windows icon and version metadata into `grit.exe`.

// The resource crate is only available when the host is Windows (release
// builds run on windows-latest); cross-building from another host skips it.
#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=../../packaging/grit.ico");

    // Build scripts run for every target: only touch the resource compiler
    // when the output is a Windows executable.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../packaging/grit.ico");
    // Windows shows these in Explorer, the taskbar and Task Manager; the
    // package name alone reads "grit".
    res.set("ProductName", "Grit");
    res.set("FileDescription", "Grit");
    res.set("InternalName", "grit");
    res.set("OriginalFilename", "grit.exe");
    res.set("CompanyName", "Raphaël C.");
    res.compile().expect("failed to compile the Windows resources");
}

#[cfg(not(windows))]
fn main() {}
