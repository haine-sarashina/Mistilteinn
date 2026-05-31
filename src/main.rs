mod app;
mod browser;
mod css;
mod html;
mod layout;
mod network;
mod render;

#[cfg(test)]
mod tests {
    #[test]
    fn engine_smoke_test() {
        assert!(true, "Mistilteinn engine test suite initialized");
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Mistilteinn starting...");

    app::run();
}
