fn main() {
    // Rebuild when docs assets change (icon and logo used in the GUI)
    println!("cargo:rerun-if-changed=docs/code.ico");
    println!("cargo:rerun-if-changed=docs/logo_gui.png");
}
