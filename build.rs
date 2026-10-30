use std::{env, path::PathBuf, process::Command};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=assets/icons.gresource.xml");
    println!("cargo:rerun-if-changed=assets/icons/scalable/apps/sdeck-plus.svg");
    println!("cargo:rerun-if-changed=assets/icons/scalable/actions/sdeck-link-symbolic.svg");
    println!("cargo:rerun-if-changed=assets/icons/scalable/actions/sdeck-remove-symbolic.svg");
    println!("cargo:rerun-if-changed=assets/icons/scalable/actions/sdeck-save-symbolic.svg");
    let output = PathBuf::from(env::var("OUT_DIR")?).join("icons.gresource");
    let status = Command::new("glib-compile-resources")
        .arg("--sourcedir=assets/icons")
        .arg("--target")
        .arg(output)
        .arg("assets/icons.gresource.xml")
        .status()?;
    if !status.success() {
        return Err(
            std::io::Error::other(format!("glib-compile-resources failed: {status}")).into(),
        );
    }
    Ok(())
}
