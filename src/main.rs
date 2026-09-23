fn main() {
    let result = magicodex::app::Options::parse()
        .map_err(std::io::Error::other)
        .and_then(|options| match options {
            Some(options) => magicodex::app::run(options),
            None => Ok(()),
        });
    if let Err(error) = result {
        eprintln!(
            "Magicodex：{}",
            magicodex::ui::safe_text(&error.to_string())
        );
        std::process::exit(1);
    }
}
