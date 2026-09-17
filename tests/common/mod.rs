#![allow(dead_code)]
#![allow(unused_imports)]

pub mod build;
pub mod process;
pub mod yaml;

pub use build::build_driver;
pub use build::compile_c;
pub use build::is_current;
pub use build::library_name;
pub use build::library_path_variable;
pub use build::link_arguments;
pub use build::macos_sdk;
pub use build::modified;
pub use build::newest;
pub use build::runtime_environment;
pub use process::run_with_timeout;
pub use yaml::documents;
pub use yaml::integer_field;
pub use yaml::string_field;

pub const VODD: &str = "vodd";
pub const HEADERS: &str = "tests/vendor/OpenCL-Headers";
pub const SPIRV_HEADERS: &str = "tests/vendor/SPIRV-Headers/include";
pub const REDIRECT: &str = "tests/support/include";
