// Hocuspanda's build of sherpa-onnx-sys (vendor/README.md): the static
// libraries of the archive scripts/prebuilt.sh fetched, checked against
// its SHA-256 and unpacked, in `SHERPA_ONNX_LIB_DIR`. Nothing is
// downloaded here; without the libraries the build stops and says how to
// get them.
use std::env;
use std::path::PathBuf;

const SHERPA_ONNX_STATIC_LIBS: &[&str] = &[
    "sherpa-onnx-c-api",
    "sherpa-onnx-core",
    "kaldi-decoder-core",
    "sherpa-onnx-kaldifst-core",
    "sherpa-onnx-fstfar",
    "sherpa-onnx-fst",
    "kaldi-native-fbank-core",
    "kissfft-float",
    "onnxruntime",
    "ssentencepiece_core",
    "utf8proc",
];

fn main() {
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_LIB_DIR");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    if env::var_os("DOCS_RS").is_some() {
        return;
    }
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "linux" || env::var_os("CARGO_FEATURE_SHARED").is_some() {
        panic!("this build of sherpa-onnx-sys links Linux's static libraries only");
    }
    let lib_dir = env::var_os("SHERPA_ONNX_LIB_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.join("libsherpa-onnx-c-api.a").is_file())
        .unwrap_or_else(|| {
            panic!(
                "no sherpa-onnx static libraries in SHERPA_ONNX_LIB_DIR ({:?}): \
                 run scripts/prebuilt.sh",
                env::var_os("SHERPA_ONNX_LIB_DIR")
            )
        });
    println!("cargo:rerun-if-changed={}", lib_dir.display());
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    for lib in SHERPA_ONNX_STATIC_LIBS {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    println!("cargo:rustc-link-lib=dylib=stdc++");
    println!("cargo:rustc-link-lib=dylib=m");
    println!("cargo:rustc-link-lib=dylib=pthread");
    println!("cargo:rustc-link-lib=dylib=dl");
}
