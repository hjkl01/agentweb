use std::{env, fs, path::{Path, PathBuf}};

fn collect_assets(root: &Path, dir: &Path, assets: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|error| {
        panic!("Cannot read frontend build directory {}: {error}. Run npm run build first.", dir.display())
    });
    for entry in entries {
        let entry = entry.expect("failed to read frontend build entry");
        let path = entry.path();
        if path.is_dir() {
            collect_assets(root, &path, assets);
        } else if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
            assets.push(path.strip_prefix(root).expect("asset outside root").to_path_buf());
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../frontend/dist");
    if !root.join("index.html").is_file() {
        panic!(
            "Frontend build not found at {}. Run 'npm run build' in the frontend directory before compiling Agent Web.",
            root.display()
        );
    }

    println!("cargo:rerun-if-changed={}", root.display());
    let mut assets = Vec::new();
    collect_assets(&root, &root, &mut assets);
    assets.sort();

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let generated = out_dir.join("embedded_assets.rs");
    let mut source = String::from("pub static EMBEDDED_ASSETS: &[(&str, &[u8])] = &[\n");
    for relative in assets {
        let absolute = root.join(&relative);
        let name = relative.to_string_lossy().replace('\\\\', "/");
        source.push_str(&format!(
            "    ({:?}, include_bytes!({:?}) as &[u8]),\n",
            name,
            absolute.to_string_lossy()
        ));
    }
    source.push_str("];\n");
    fs::write(generated, source).expect("failed to write embedded frontend asset index");
}
