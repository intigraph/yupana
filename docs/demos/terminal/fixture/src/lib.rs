pub fn parse(input: &str) -> &str {
    input
}

pub fn load(path: &str) -> &str {
    parse(path)
}

pub fn run() -> &'static str {
    load("config.toml")
}
