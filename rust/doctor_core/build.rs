fn main() {
    println!("cargo:rerun-if-env-changed=FASTAPI_DOCTOR_NATIVE_VERSION");
}
