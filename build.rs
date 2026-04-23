// build.rs: embed the switchboard UAC manifest + tray ICOs as Win32 resources.
//
// manifest/switchboard.rc carries:
//   - Resource id 1, type 24 (RT_MANIFEST) → switchboard.exe.manifest (requireAdministrator)
//   - Icon id 101 (IDI_TRAY_DARK)  → assets/icons/switchboard-dark.ico
//   - Icon id 102 (IDI_TRAY_LIGHT) → assets/icons/switchboard-light.ico
//
// id 101 is also the EXE icon Explorer shows (lowest-numbered ICON resource
// wins per Win32 convention). Light/dark runtime swap for the tray happens
// in src/main.rs via tray_icon::Icon::from_resource() — see src/theme.rs.
//
// embed-resource (rather than winres) so the build works under cargo-xwin
// in Docker for the aarch64-pc-windows-msvc cross-compile.
fn main() {
    embed_resource::compile("manifest/switchboard.rc", embed_resource::NONE);
    println!("cargo:rerun-if-changed=manifest/switchboard.rc");
    println!("cargo:rerun-if-changed=manifest/switchboard.exe.manifest");
    println!("cargo:rerun-if-changed=assets/icons/switchboard-dark.ico");
    println!("cargo:rerun-if-changed=assets/icons/switchboard-light.ico");
}
