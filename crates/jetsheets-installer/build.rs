use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=JETSHEETS_BUNDLE_PATH");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR definido pelo Cargo"))
        .join("bundled.exe");
    if let Some(source) = env::var_os("JETSHEETS_BUNDLE_PATH") {
        let source = PathBuf::from(source);
        println!("cargo:rerun-if-changed={}", source.display());
        fs::copy(&source, &output).unwrap_or_else(|error| {
            panic!("Não foi possível incluir {}: {error}", source.display())
        });
    } else {
        fs::write(output, []).expect("não foi possível criar marcador de pacote vazio");
    }
}
