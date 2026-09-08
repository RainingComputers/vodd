#![allow(dead_code)]

pub fn string_field<'y>(entry: &'y yaml_rust2::Yaml, name: &str, context: &str) -> &'y str {
    entry[name]
        .as_str()
        .unwrap_or_else(|| panic!("{context}: {name} is missing or is not a string"))
}

pub fn integer_field(entry: &yaml_rust2::Yaml, name: &str, context: &str) -> usize {
    entry[name]
        .as_i64()
        .unwrap_or_else(|| panic!("{context}: {name} is missing or is not an integer")) as usize
}

pub fn documents(path: &str) -> yaml_rust2::Yaml {
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));

    yaml_rust2::YamlLoader::load_from_str(&text)
        .unwrap_or_else(|error| panic!("{path} is not valid YAML: {error}"))
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("{path} is empty"))
}
